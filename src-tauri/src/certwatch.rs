//! 证书链监控 — 两个独立能力, sentry daemon 周期调用
//!
//! 1. CA 证书安装监控 (`scan_ca_and_diff`)
//!    枚举「受信任根证书」库 (机器 / 用户 / 组策略), 和 baseline 比对,
//!    出现新根证书就告警。流氓软件 / MITM 代理装根证书是拦截 HTTPS 的前提。
//!
//! 2. Claude 链路 MITM 检测 (`check_claude_tls`)
//!    直连 api.anthropic.com / claude.ai, 抓取对端**真实证书链**, 用内置的
//!    公共根 (webpki-roots, 与系统证书库无关) 独立校验。若校验不过, 说明
//!    有个只装在本机证书库里的流氓根在中间拦截 → 告警, 并解析出拦截方 CA 名字。
//!
//! 两者都复用 inspection::ChangeEvent, 走同一套 toast + 事件日志管线。

use crate::inspection::{ChangeEvent, ChangeKind};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ============ 受信任根证书库枚举 (CryptoAPI) ============

use std::ffi::c_void;
use windows::Win32::Security::Cryptography::{
    CertCloseStore, CertCreateCertificateContext, CertEnumCertificatesInStore,
    CertFreeCertificateContext, CertGetCertificateContextProperty, CertGetNameStringW,
    CertOpenStore, CERT_CONTEXT, CERT_NAME_ISSUER_FLAG, CERT_NAME_SIMPLE_DISPLAY_TYPE,
    CERT_OPEN_STORE_FLAGS, CERT_QUERY_ENCODING_TYPE, CERT_SHA1_HASH_PROP_ID,
    CERT_STORE_PROV_SYSTEM_W, CERT_STORE_READONLY_FLAG, CERT_SYSTEM_STORE_CURRENT_USER,
    CERT_SYSTEM_STORE_LOCAL_MACHINE, CERT_SYSTEM_STORE_LOCATION_SHIFT, X509_ASN_ENCODING,
};

/// 组策略「本机」根证书库位置 (location id 8 左移 16 位), windows crate 未直接导出此常量
const CERT_SYSTEM_STORE_LM_GROUP_POLICY: u32 = 8u32 << CERT_SYSTEM_STORE_LOCATION_SHIFT;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct CaCertEntry {
    /// SHA1 指纹 (大写 hex), 证书唯一标识
    pub thumbprint: String,
    /// 证书主体显示名
    pub subject: String,
    /// 来源库: "机器" / "用户" / "组策略"
    pub store: String,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn hex_upper(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02X}"));
    }
    s
}

/// 从 DER 证书里取主体名 (issuer=true 取签发者名)。用于解析 TLS 抓到的对端证书。
pub fn cert_name_from_der(der: &[u8], issuer: bool) -> Option<String> {
    unsafe {
        let ctx = CertCreateCertificateContext(X509_ASN_ENCODING, der);
        if ctx.is_null() {
            return None;
        }
        let name = cert_name_string(ctx as *const CERT_CONTEXT, issuer);
        let _ = CertFreeCertificateContext(Some(ctx as *const CERT_CONTEXT));
        if name.is_empty() {
            None
        } else {
            Some(name)
        }
    }
}

/// 读证书上下文的主体/签发者显示名
unsafe fn cert_name_string(ctx: *const CERT_CONTEXT, issuer: bool) -> String {
    let flags: u32 = if issuer { CERT_NAME_ISSUER_FLAG } else { 0 };
    // 先探长度 (返回值含结尾 null)
    let len = CertGetNameStringW(ctx, CERT_NAME_SIMPLE_DISPLAY_TYPE, flags, None, None);
    if len <= 1 {
        return String::new();
    }
    let mut buf = vec![0u16; len as usize];
    let got = CertGetNameStringW(
        ctx,
        CERT_NAME_SIMPLE_DISPLAY_TYPE,
        flags,
        None,
        Some(buf.as_mut_slice()),
    );
    let end = got.saturating_sub(1) as usize;
    String::from_utf16_lossy(&buf[..end.min(buf.len())])
}

/// 枚举一个系统证书库位置下的 "Root" 库
fn scan_one_store(location: u32, label: &str) -> Vec<CaCertEntry> {
    let mut out = Vec::new();
    let store_name = wide("Root");
    unsafe {
        let flags = CERT_OPEN_STORE_FLAGS(location | CERT_STORE_READONLY_FLAG.0);
        let handle = CertOpenStore(
            CERT_STORE_PROV_SYSTEM_W,
            CERT_QUERY_ENCODING_TYPE(0),
            None,
            flags,
            Some(store_name.as_ptr() as *const c_void),
        );
        let Ok(handle) = handle else {
            return out;
        };

        let mut ctx = CertEnumCertificatesInStore(handle, None);
        while !ctx.is_null() {
            let ctx_const = ctx as *const CERT_CONTEXT;
            // 指纹
            let mut hash = [0u8; 20];
            let mut hlen = hash.len() as u32;
            let got = CertGetCertificateContextProperty(
                ctx_const,
                CERT_SHA1_HASH_PROP_ID,
                Some(hash.as_mut_ptr() as *mut c_void),
                &mut hlen,
            );
            let thumbprint = if got.is_ok() {
                hex_upper(&hash[..hlen as usize])
            } else {
                String::new()
            };
            let subject = cert_name_string(ctx_const, false);
            if !thumbprint.is_empty() {
                out.push(CaCertEntry {
                    thumbprint,
                    subject,
                    store: label.to_string(),
                });
            }
            // 枚举下一个 (会自动 free 上一个 ctx, 不能手动 free)
            ctx = CertEnumCertificatesInStore(handle, Some(ctx_const));
        }
        let _ = CertCloseStore(Some(handle), 0);
    }
    out
}

/// 扫描全部受信任根证书库 (机器 + 用户 + 组策略)
pub fn scan_root_stores() -> Vec<CaCertEntry> {
    let mut out = Vec::new();
    out.extend(scan_one_store(CERT_SYSTEM_STORE_LOCAL_MACHINE, "机器"));
    out.extend(scan_one_store(CERT_SYSTEM_STORE_CURRENT_USER, "用户"));
    out.extend(scan_one_store(CERT_SYSTEM_STORE_LM_GROUP_POLICY, "组策略"));
    out
}

// ============ CA baseline (自己的文件, 不和 inspection 混) ============

/// sentry 共享目录 %LOCALAPPDATA%\mingchuang\sentry (和 sentry_client/whitelist 保持一致)
fn sentry_dir() -> PathBuf {
    let local = std::env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    local.join("mingchuang").join("sentry")
}

fn ca_baseline_path() -> PathBuf {
    sentry_dir().join("ca-baseline.json")
}

pub fn load_ca_baseline() -> Option<Vec<CaCertEntry>> {
    let txt = std::fs::read_to_string(ca_baseline_path()).ok()?;
    serde_json::from_str(&txt).ok()
}

fn save_ca_baseline(entries: &[CaCertEntry]) -> Result<()> {
    let path = ca_baseline_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("创建 {parent:?} 失败"))?;
    }
    let json = serde_json::to_string_pretty(entries)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

pub fn reset_ca_baseline() -> Result<()> {
    let path = ca_baseline_path();
    if path.exists() {
        std::fs::remove_file(&path).with_context(|| format!("删除 {path:?} 失败"))?;
    }
    Ok(())
}

/// 扫根证书库, 和 baseline 比, 新增的根证书 emit 告警。首次建基准不告警。
pub fn scan_ca_and_diff() -> Vec<ChangeEvent> {
    let curr = scan_root_stores();
    let result = match load_ca_baseline() {
        None => {
            let summary = format!("已建立根证书基准: 共 {} 张受信任根证书", curr.len());
            eprintln!("[sentry] {summary}");
            vec![ChangeEvent {
                ts: chrono::Utc::now(),
                kind: ChangeKind::BaselineEstablished,
                category: "ca_cert".into(),
                label: summary,
                detail: "首次开启 CA 证书监控, 把当前受信任根证书当基准。以后只有新装的根证书才告警。".into(),
            }]
            .into_iter()
            .filter(|_| false) // 首次不产告警, 只落基准
            .collect()
        }
        Some(base) => {
            let base_thumbs: std::collections::HashSet<&str> =
                base.iter().map(|e| e.thumbprint.as_str()).collect();
            curr.iter()
                .filter(|e| !base_thumbs.contains(e.thumbprint.as_str()))
                .map(|e| ChangeEvent {
                    ts: chrono::Utc::now(),
                    kind: ChangeKind::Added,
                    category: "ca_cert".into(),
                    label: format!("发现新装的根证书: {}", e.subject),
                    detail: format!(
                        "来源[{}] 指纹 {}。若非你主动安装, 可能是流氓软件/中间人代理装的, 会让它能解密你的 HTTPS 流量。",
                        e.store, e.thumbprint
                    ),
                })
                .collect()
        }
    };
    if let Err(e) = save_ca_baseline(&curr) {
        eprintln!("[sentry] 保存 CA baseline 失败: {e:#}");
    }
    result
}

// ============ Claude 链路 TLS 抓链 + 公共根独立校验 ============

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::client::WebPkiServerVerifier;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, ClientConnection, DigitallySignedStruct, RootCertStore, SignatureScheme};
use std::io::Write;
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;

/// 要监控的 Claude 相关端点
const CLAUDE_HOSTS: &[&str] = &["api.anthropic.com", "claude.ai"];

/// 抓链用的"全放行"校验器: 只为完成握手拿到对端证书, 真正的信任判断交给公共根校验器
#[derive(Debug)]
struct CaptureVerifier;

impl ServerCertVerifier for CaptureVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }
    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::RSA_PKCS1_SHA256,
            SignatureScheme::RSA_PKCS1_SHA384,
            SignatureScheme::RSA_PKCS1_SHA512,
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::ECDSA_NISTP384_SHA384,
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::RSA_PSS_SHA384,
            SignatureScheme::RSA_PSS_SHA512,
            SignatureScheme::ED25519,
        ]
    }
}

/// 一次检查的结果
enum TlsCheckOutcome {
    /// 证书链能过公共根校验 → 干净
    Clean,
    /// 握手成功但公共根校验不过 → 被拦截, 附拦截方 CA 名
    Intercepted { interceptor: String, leaf_subject: String },
    /// 连不上 / 握手失败 (网络问题), 不作为告警
    Unreachable(String),
}

fn ring_provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// 直连一个 host, 抓证书链, 用公共根独立校验
fn check_one_host(host: &str) -> TlsCheckOutcome {
    let provider = ring_provider();

    let config = match ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
    {
        Ok(b) => b
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(CaptureVerifier))
            .with_no_client_auth(),
        Err(e) => return TlsCheckOutcome::Unreachable(format!("TLS 配置失败: {e}")),
    };

    let server_name = match ServerName::try_from(host.to_string()) {
        Ok(n) => n,
        Err(e) => return TlsCheckOutcome::Unreachable(format!("host 无效: {e}")),
    };

    let mut conn = match ClientConnection::new(Arc::new(config), server_name.clone()) {
        Ok(c) => c,
        Err(e) => return TlsCheckOutcome::Unreachable(format!("TLS 初始化失败: {e}")),
    };

    let mut sock = match TcpStream::connect((host, 443)) {
        Ok(s) => s,
        Err(e) => return TlsCheckOutcome::Unreachable(format!("连不上: {e}")),
    };
    let _ = sock.set_read_timeout(Some(Duration::from_secs(8)));
    let _ = sock.set_write_timeout(Some(Duration::from_secs(8)));

    // 驱动握手: 写一个最小请求
    let mut tls = rustls::Stream::new(&mut conn, &mut sock);
    let req = format!("HEAD / HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
    if let Err(e) = tls.write_all(req.as_bytes()) {
        return TlsCheckOutcome::Unreachable(format!("握手失败: {e}"));
    }
    let _ = tls.flush();

    // 拿对端真实证书链
    let chain: Vec<CertificateDer<'static>> = match conn.peer_certificates() {
        Some(certs) if !certs.is_empty() => certs.iter().map(|c| c.clone().into_owned()).collect(),
        _ => return TlsCheckOutcome::Unreachable("没拿到对端证书".into()),
    };

    // 用内置公共根 (与系统证书库无关) 独立校验
    let mut roots = RootCertStore::empty();
    roots
        .roots
        .extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let verifier = match WebPkiServerVerifier::builder_with_provider(Arc::new(roots), provider)
        .build()
    {
        Ok(v) => v,
        Err(e) => return TlsCheckOutcome::Unreachable(format!("校验器构建失败: {e}")),
    };

    let (leaf, intermediates) = chain.split_first().unwrap();
    let now = UnixTime::now();
    match verifier.verify_server_cert(leaf, intermediates, &server_name, &[], now) {
        Ok(_) => TlsCheckOutcome::Clean,
        Err(_) => {
            // 校验不过 = 链不到公共根 = 被本机某个流氓根拦截
            let leaf_subject =
                cert_name_from_der(leaf.as_ref(), false).unwrap_or_else(|| "(未知)".into());
            let interceptor =
                cert_name_from_der(leaf.as_ref(), true).unwrap_or_else(|| "(未知签发者)".into());
            TlsCheckOutcome::Intercepted {
                interceptor,
                leaf_subject,
            }
        }
    }
}

/// 检查所有 Claude 端点, 被拦截的 emit 告警
pub fn check_claude_tls() -> Vec<ChangeEvent> {
    let mut events = Vec::new();
    for host in CLAUDE_HOSTS {
        match check_one_host(host) {
            TlsCheckOutcome::Clean => {
                eprintln!("[sentry] Claude 链路 {host}: 证书链干净 (过公共根校验)");
            }
            TlsCheckOutcome::Unreachable(why) => {
                eprintln!("[sentry] Claude 链路 {host}: 跳过 ({why})");
            }
            TlsCheckOutcome::Intercepted {
                interceptor,
                leaf_subject,
            } => {
                events.push(ChangeEvent {
                    ts: chrono::Utc::now(),
                    kind: ChangeKind::Added,
                    category: "claude_tls".into(),
                    label: format!("Claude 链路 {host} 疑似被中间人拦截"),
                    detail: format!(
                        "{host} 返回的证书 (主体 {leaf_subject}) 不是由公共 CA 签发, 而是由「{interceptor}」签发。\
                         这说明有个装在你本机证书库里的根在解密你和 Claude 的通信。检查『CA 证书监控』里最近新增的根证书。"
                    ),
                });
            }
        }
    }
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 功能1冒烟: 真实枚举本机受信任根证书, 验证 CryptoAPI 解析通
    #[test]
    fn ca_scan_finds_real_roots() {
        let roots = scan_root_stores();
        println!("本机受信任根证书: {} 张", roots.len());
        for r in roots.iter().take(5) {
            println!("  [{}] {} — {}", r.store, r.thumbprint, r.subject);
        }
        // 任何正常 Windows 都内置几十张根证书, 且应能解析出指纹和主体名
        assert!(roots.len() > 5, "根证书数量异常, CryptoAPI 枚举可能失败");
        assert!(
            roots.iter().any(|r| r.thumbprint.len() == 40 && !r.subject.is_empty()),
            "应至少有一张证书解析出 40位SHA1指纹 + 非空主体名"
        );
    }

    /// 功能2冒烟(需联网, 默认跳过): 手动跑 `cargo test --lib -- --ignored claude`
    #[test]
    #[ignore]
    fn claude_tls_check_runs() {
        let events = check_claude_tls();
        println!("Claude 链路告警数: {}", events.len());
        for e in &events {
            println!("  {} — {}", e.label, e.detail);
        }
    }
}
