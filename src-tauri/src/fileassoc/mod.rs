//! 文件关联管理 - 把一组扩展名的默认打开方式设给某个 exe
//!
//! MVP 实现(2026-06-27):
//! 1. 注册自定义 ProgId 到 HKCU\Software\Classes\kuake-fuckyou.<stem>
//! 2. 把该 ProgId 写入每个 ext 的 OpenWithProgids
//! 3. 清除现有 UserChoice(让系统下次从 OpenWithProgids 选)
//! 4. SHChangeNotify(SHCNE_ASSOCCHANGED) 通知 Shell 刷新
//!
//! 局限:UserChoice 的精确哈希锁定下一轮再做(打包 SetUserFTA 或自写哈希)。
//! 本机实测 reg 写入完成后,首次打开此类文件 Windows 会弹"打开方式",选我们 ProgId 即生效。

pub mod detect;
pub mod manifest;
pub mod presets;
pub mod progid;
pub mod userchoice;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
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
    /// 这些扩展名 OpenWithProgids + Hash 双写都成功(真正强制锁定)
    pub extensions_set: Vec<String>,
    pub extensions_failed: Vec<(String, String)>,
    /// 这些扩展名 OpenWithProgids 成功但 UserChoice Hash 写入被 Windows 拒(算法可能本机版本不兼容,
    /// 需要兜底走系统设置)
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

    // 去重 + 校验
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
        // 第一步: 写 OpenWithProgids (基础, 让我们的应用出现在"打开方式"列表里)
        match associate_ext(&ext, &progid) {
            Ok(_) => {}
            Err(e) => {
                extensions_failed.push((ext.clone(), format!("OpenWithProgids: {e:#}")));
                continue;
            }
        }
        // 第二步: 算 UserChoice Hash 强制锁定 — 这才是真正生效的关键
        match userchoice::force_set_user_choice(&ext, &progid) {
            Ok(_) => {
                extensions_set.push(ext.clone());
            }
            Err(e) => {
                eprintln!("[fileassoc] UserChoice hash for {ext} failed: {e:#}");
                extensions_need_manual.push(ext);
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

fn is_valid_ext(ext: &str) -> bool {
    if !ext.starts_with('.') { return false; }
    let body = &ext[1..];
    if body.is_empty() || body.len() > 16 { return false; }
    body.chars().all(|c| c.is_ascii_alphanumeric())
}

/// 标准化扩展名为 `.xxx`(小写)
fn normalize_ext(ext: &str) -> String {
    let trimmed = ext.trim().to_ascii_lowercase();
    if trimmed.starts_with('.') { trimmed } else { format!(".{trimmed}") }
}

fn associate_ext(ext: &str, progid: &str) -> Result<bool> {
    // 1. 把 ProgId 写进 HKCU\Software\Classes\<ext>\OpenWithProgids
    let owpid_path = format!("Software\\Classes\\{ext}\\OpenWithProgids");
    let key = CURRENT_USER.create(&owpid_path).with_context(|| format!("创建 {owpid_path} 失败"))?;
    key.set_string(progid, "")
        .with_context(|| format!("写 OpenWithProgids[{progid}] 失败"))?;

    // 2. 清现有 UserChoice (失败不影响,主路径是 OpenWithProgids)
    let uc_path = format!(
        "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\FileExts\\{ext}\\UserChoice"
    );
    let mut cleared = false;
    if CURRENT_USER.open(&uc_path).is_ok() {
        if CURRENT_USER.remove_tree(&uc_path).is_ok() {
            cleared = true;
        }
    }

    Ok(cleared)
}

/// 通知 Shell 关联已变 — 用 windows crate 直接调 SHChangeNotify, 不起子进程
fn notify_shell_assoc_changed() {
    unsafe {
        use windows::Win32::UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_IDLIST};
        SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None);
    }
}

#[allow(dead_code)]
fn _ensure_path(_: &Path) {}
