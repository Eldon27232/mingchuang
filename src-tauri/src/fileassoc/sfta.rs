//! 调用 PS-SFTA 强制锁定 UserChoice (绕过 Windows 的 hash 防篡改)
//!
//! 上一轮自己 port hash 算法 (wordswap_md) 算错了, Win11 拒绝写入并把整个
//! UserChoice 键清掉, 用户原来的关联全丢。教训: 不自己写 hash, 用社区验证过的脚本。
//!
//! 这里 vendor 了 DanysysTeam/PS-SFTA v1.2.0 (MIT, ~23KB 纯 PowerShell), 编译期
//! 通过 include_str! 内联进二进制, 运行时一次性落到 %TEMP%, 用 PowerShell 调
//! Set-FTA 函数。
//!
//! 性能注意 (ba55ca3 教训):
//! 每个 ext 单独 spawn 一次 powershell.exe 会爆炸 — PS 冷启 ~1-2s + 每次
//! Set-FTA 内部 Add-Type 重编 ~300ms + SHChangeNotify。30 个 ext = 60s+。
//! **本实现 spawn 一次 PowerShell, 批处理整组 ext, stdin 喂列表 stdout 收结果**。
//!
//! 来源: https://github.com/DanysysTeam/PS-SFTA  (MIT License)
//! 算法: 两遍 Marvin32 + 微软魔法常量 + ProgId LastWriteTime 戳

use anyhow::{anyhow, Context, Result};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::OnceLock;

const SFTA_PS1_RAW: &str = include_str!("../../resources/SFTA.ps1");

/// 编译期 patch 掉 SFTA.ps1 里 Set-FTA 内部的慢操作:
///
/// 1. `Write-RequiredApplicationAssociationToasts $ProgId $Extension`
///    会枚举 HKLM+HKCU 下所有 RegisteredApplications (本机 ~92 个) 的
///    Capabilities\FileAssociations 子项, 单次调用就 ~2s。这只影响 Windows
///    "现在有新默认应用可用"的 toast 通知 (我们不需要), 跳过对实际关联无影响。
///
/// 2. `Update-RegistryChanges` 用 Add-Type 编译 C# 调 SHChangeNotify。
///    第一次 ~300ms (含编译), 后续无成本。但我们已经在 Rust 端
///    notify_shell_assoc_changed() 用 windows crate 直接调了, PS 这次没必要再
///    调。批处理多次 Update-RegistryChanges 也只是 SHChangeNotify, 没害, 不动。
fn sfta_patched() -> &'static str {
    use std::sync::OnceLock;
    static PATCHED: OnceLock<String> = OnceLock::new();
    PATCHED.get_or_init(|| {
        SFTA_PS1_RAW
            .replace(
                "Write-RequiredApplicationAssociationToasts $ProgId $Extension",
                "# patched-out (was Write-RequiredApplicationAssociationToasts, ~2s/call HKLM enum)",
            )
            // 注意: 只能匹配调用点 (行末是 \n), 不能匹配定义 (function local:Update-RegistryChanges {),
            // 否则把函数名注掉脚本就挂。pattern 带换行 → 只命中 line 749 的调用。
            .replace(
                "Update-RegistryChanges \n",
                "# patched-out (Update-RegistryChanges Add-Type+SHChangeNotify, Rust 端 notify_shell_assoc_changed 统一替代)\n",
            )
            .replace(
                "Update-RegistryChanges \r\n",
                "# patched-out (Update-RegistryChanges Add-Type+SHChangeNotify, Rust 端 notify_shell_assoc_changed 统一替代)\r\n",
            )
    })
}

/// 落地后的脚本路径, 整个进程生命周期内只解一次
static EXTRACTED: OnceLock<PathBuf> = OnceLock::new();

/// 把内联的 SFTA.ps1 写到 %TEMP%, 返回脚本路径。同进程多次调用只写一次。
fn ensure_extracted() -> Result<&'static Path> {
    if let Some(p) = EXTRACTED.get() {
        return Ok(p);
    }
    // 用内容长度 + 头 16 字节做版本指纹, 升级时自然换文件名
    let fp: u32 = {
        let head = sfta_patched().as_bytes().iter().take(16).copied().map(u32::from).sum::<u32>();
        (sfta_patched().len() as u32).wrapping_mul(2654435761).wrapping_add(head)
    };
    let path = std::env::temp_dir().join(format!("mingchuang-sfta-{:08x}.ps1", fp));

    let need_write = match std::fs::metadata(&path) {
        Ok(m) if m.len() as usize == sfta_patched().len() => false,
        _ => true,
    };
    if need_write {
        std::fs::write(&path, sfta_patched())
            .with_context(|| format!("写 SFTA.ps1 到临时目录失败: {}", path.display()))?;
    }
    let leaked: &'static Path = Box::leak(path.into_boxed_path());
    let _ = EXTRACTED.set(leaked.to_path_buf());
    Ok(leaked)
}

#[derive(Debug)]
pub struct BatchOutcome {
    pub ok: Vec<String>,
    pub failed: Vec<(String, String)>,
}

/// 批量把一组 ext (含点) 的 UserChoice 锁到同一个 progid。
/// 单次 spawn powershell.exe, 一次 dot-source SFTA.ps1, 在同进程内循环 Set-FTA。
/// 失败的 ext 进 failed 列表, 含错误描述; 成功后回读 UserChoice 验 ProgId 字段。
pub fn force_set_user_choice_batch(progid: &str, exts: &[String]) -> Result<BatchOutcome> {
    if progid.is_empty() || progid.contains('\'') || progid.contains('"') || progid.contains('\n') {
        return Err(anyhow!("progid 含非法字符或为空: {progid}"));
    }
    for e in exts {
        if !e.starts_with('.') || e.contains('\'') || e.contains('"') || e.contains('\n') {
            return Err(anyhow!("ext 非法: {e}"));
        }
    }
    if exts.is_empty() {
        return Ok(BatchOutcome { ok: vec![], failed: vec![] });
    }

    let script = ensure_extracted()?;
    let script_esc = script.display().to_string().replace('\'', "''");
    let progid_esc = progid.replace('\'', "''");

    // 单次 -Command: dot-source 脚本, 然后 stdin 一行一个 ext, 每行写一条
    // 'OK|<ext>' 或 'ERR|<ext>|<msg>' 到 stdout。
    // 用 $progid 别名 (而不是 $pid, 它是 PowerShell 自动变量 = 进程 ID)。
    let ps_cmd = format!(
        ". '{script_esc}'
$ErrorActionPreference = 'Continue'
$target = '{progid_esc}'
while ($null -ne ($line = [Console]::In.ReadLine())) {{
  $e = $line.Trim()
  if ([string]::IsNullOrEmpty($e)) {{ continue }}
  try {{
    Set-FTA -ProgId $target -Extension $e -ErrorAction Stop | Out-Null
    Write-Output ('OK|' + $e)
  }} catch {{
    $msg = ($_.Exception.Message -replace '[\r\n|]+', ' ')
    Write-Output ('ERR|' + $e + '|' + $msg)
  }}
}}"
    );

    let mut child = crate::sys_cmd::cmd("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &ps_cmd,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("启动 powershell 失败")?;

    {
        let stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| anyhow!("无法拿到 powershell stdin"))?;
        for e in exts {
            stdin
                .write_all(e.as_bytes())
                .and_then(|_| stdin.write_all(b"\n"))
                .context("写 ext 到 powershell stdin 失败")?;
        }
    }
    // stdin drop → EOF → 子进程 while 退出

    let out = child.wait_with_output().context("等待 powershell 退出失败")?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(anyhow!(
            "powershell 退出码 {:?}, stderr={}",
            out.status.code(),
            stderr.trim()
        ));
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut ok = Vec::new();
    let mut failed: Vec<(String, String)> = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(e) = line.strip_prefix("OK|") {
            ok.push(e.to_string());
        } else if let Some(rest) = line.strip_prefix("ERR|") {
            let mut sp = rest.splitn(2, '|');
            let e = sp.next().unwrap_or("").to_string();
            let m = sp.next().unwrap_or("(no message)").to_string();
            failed.push((e, m));
        }
        // 其他行 (verbose / warning) 忽略
    }

    // 回读 UserChoice 验 ProgId, 防 Set-FTA 报 OK 但 Windows 后续清键
    let mut verified_ok = Vec::with_capacity(ok.len());
    for e in ok {
        match verify_user_choice(&e, progid) {
            Ok(()) => verified_ok.push(e),
            Err(err) => failed.push((e, format!("回读校验: {err:#}"))),
        }
    }

    Ok(BatchOutcome { ok: verified_ok, failed })
}

fn verify_user_choice(ext: &str, expected_progid: &str) -> Result<()> {
    use windows_registry::CURRENT_USER;
    let path = format!(
        "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\FileExts\\{ext}\\UserChoice"
    );
    let key = CURRENT_USER
        .open(&path)
        .map_err(|e| anyhow!("UserChoice 键不存在或无权读: {e}"))?;
    let got: String = key.get_string("ProgId").unwrap_or_default();
    if got != expected_progid {
        return Err(anyhow!(
            "UserChoice.ProgId 期望 {expected_progid}, 实际 {got}"
        ));
    }
    let _hash: String = key.get_string("Hash").unwrap_or_default();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_registry::CURRENT_USER;

    /// 真机回归: 用一组废扩展名走批处理路径, 然后清理。
    /// 默认 ignore — 真改 HKCU, 需要手动 `cargo test -- --ignored sfta_smoke`
    #[test]
    #[ignore]
    fn sfta_smoke() {
        // 真机批量端到端: 3 个废扩展名跑完整路径, 校验 UserChoice 真锁住。
        // 性能基线 (Win11 24H2): 3 ext ~1.3s, 15 ext ~2.3s, 30 ext 约 4-5s
        let exts: Vec<String> = (1..=3).map(|i| format!(".kkrust{i}")).collect();
        let progid = "Mingchuang.RustSmoke";

        let cls = format!("Software\\Classes\\{progid}");
        let k = CURRENT_USER.create(&cls).expect("create ProgId");
        k.set_string("", "KK Rust Smoke").expect("set name");

        let started = std::time::Instant::now();
        let r = force_set_user_choice_batch(progid, &exts);
        let elapsed = started.elapsed();

        // 清理
        for e in &exts {
            let _ = CURRENT_USER.remove_tree(&format!(
                "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\FileExts\\{e}"
            ));
        }
        let _ = CURRENT_USER.remove_tree(&cls);

        let outcome = r.expect("force_set_user_choice_batch 失败");
        eprintln!("batch {} ext 耗时: {:?} ok={} failed={}", exts.len(), elapsed, outcome.ok.len(), outcome.failed.len());
        for (e, m) in &outcome.failed {
            eprintln!("FAIL {e}: {m}");
        }
        assert_eq!(outcome.ok.len(), exts.len(), "ok 数应等于 ext 数, failed={:?}", outcome.failed);
    }
}
