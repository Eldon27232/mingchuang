//! 文件关联管理 - 把一组扩展名的默认打开方式设给某个 exe
//!
//! 当前实现 (2026-06-28, sidecar 方案):
//! 1. 注册自定义 ProgId 到 HKCU\Software\Classes\KuakeFuckyou.<stem>
//! 2. 把该 ProgId 写入每个 ext 的 OpenWithProgids (让"打开方式"菜单里出现)
//! 3. 调 PS-SFTA (Set-FTA) 强制写 UserChoice + 微软认可的 Hash, 真正锁定默认
//! 4. 回读 UserChoice 验证成功; 失败才回退到"need_manual"
//! 5. SHChangeNotify 通知 Shell 刷新
//!
//! 历史:
//! - 73558d6 自己 port hash 算法 → 算错, Win11 把整个 UserChoice 清掉, 用户原关联丢失
//! - 1eaa94b 紧急退回 OpenWithProgids + 手动兜底 (用户明确否决: 不接受手动)
//! - 本 commit 接入 PS-SFTA, 全自动且不再自算 hash

pub mod detect;
pub mod manifest;
pub mod presets;
pub mod progid;
pub mod sfta;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use windows_registry::CURRENT_USER;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssocPreset {
    pub id: String,
    pub label: String,
    pub extensions: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AssocResult {
    pub exe: String,
    pub progid: String,
    /// OpenWithProgids + UserChoice Hash 双写都成功, 真正强制锁定
    pub extensions_set: Vec<String>,
    pub extensions_failed: Vec<(String, String)>,
    /// OpenWithProgids 成功但 Set-FTA 失败 (极少数情况, 如 UCPD.sys 保护 http/.pdf),
    /// 需要兜底走系统设置
    pub extensions_need_manual: Vec<String>,
}

pub fn list_presets() -> Vec<AssocPreset> {
    presets::all()
}

/// 把 extensions 的默认打开方式设给 exe
pub fn set_app_defaults(exe_path: &str, extensions: &[String]) -> Result<AssocResult> {
    if extensions.is_empty() {
        return Err(anyhow::anyhow!("没选任何扩展名"));
    }
    if exe_path.contains('"') {
        return Err(anyhow::anyhow!("exe 路径含双引号, 拒绝处理: {exe_path}"));
    }
    let exe = PathBuf::from(exe_path);
    if !exe.is_file() {
        return Err(anyhow::anyhow!("exe 不存在: {exe_path}"));
    }
    let progid = progid::register(&exe).context("注册 ProgId 失败")?;

    let mut extensions_set = Vec::new();
    let mut extensions_failed = Vec::new();
    let mut extensions_need_manual = Vec::new();

    let mut seen = std::collections::HashSet::new();
    for raw_ext in extensions {
        let ext = normalize_ext(raw_ext);
        if !is_valid_ext(&ext) {
            extensions_failed.push((ext.clone(), "格式不对(应是 .xxx)".into()));
            continue;
        }
        if !seen.insert(ext.clone()) {
            continue;
        }

        // 1) OpenWithProgids: 把 ProgId 加进 Windows "打开方式" 列表
        if let Err(e) = write_open_with_progids(&ext, &progid) {
            extensions_failed.push((ext.clone(), format!("OpenWithProgids: {e:#}")));
            continue;
        }

        // 2) Set-FTA: 强制锁定 UserChoice (核心)
        match sfta::force_set_user_choice(&ext, &progid) {
            Ok(()) => extensions_set.push(ext.clone()),
            Err(e) => {
                // OpenWithProgids 已成功, UserChoice 没锁住 → 兜底引导手动
                // (大概率是 UCPD.sys 保护的 http/https/.pdf, 或 PowerShell 不可用)
                eprintln!("[sfta] {ext}: {e:#}");
                extensions_need_manual.push(ext.clone());
            }
        }
    }

    notify_shell_assoc_changed();

    Ok(AssocResult {
        exe: exe.display().to_string(),
        progid,
        extensions_set,
        extensions_failed,
        extensions_need_manual,
    })
}

fn write_open_with_progids(ext: &str, progid: &str) -> Result<()> {
    let owpid_path = format!("Software\\Classes\\{ext}\\OpenWithProgids");
    let key = CURRENT_USER
        .create(&owpid_path)
        .with_context(|| format!("创建 {owpid_path} 失败"))?;
    key.set_string(progid, "")
        .with_context(|| format!("写 OpenWithProgids[{progid}] 失败"))?;
    Ok(())
}

fn is_valid_ext(ext: &str) -> bool {
    if !ext.starts_with('.') {
        return false;
    }
    let body = &ext[1..];
    if body.is_empty() || body.len() > 16 {
        return false;
    }
    body.chars().all(|c| c.is_ascii_alphanumeric())
}

/// 标准化扩展名为 `.xxx`(小写)
fn normalize_ext(ext: &str) -> String {
    let trimmed = ext.trim().to_ascii_lowercase();
    if trimmed.starts_with('.') {
        trimmed
    } else {
        format!(".{trimmed}")
    }
}

/// 通知 Shell 关联已变 — 直接调 SHChangeNotify
fn notify_shell_assoc_changed() {
    unsafe {
        use windows::Win32::UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_IDLIST};
        SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None);
    }
}
