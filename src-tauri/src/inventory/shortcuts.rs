//! 桌面/开始菜单快捷方式扫描
//!
//! 扫 4 个标准位置:
//!   - 公共桌面    %PUBLIC%\Desktop
//!   - 用户桌面    %USERPROFILE%\Desktop
//!   - 公共开始菜单 %ProgramData%\Microsoft\Windows\Start Menu\Programs
//!   - 用户开始菜单 %APPDATA%\Microsoft\Windows\Start Menu\Programs
//!
//! 取 .lnk 文件的 target exe (用 PowerShell WScript.Shell 简化, 避免 COM 模板)
//! 匹配画像 process_names / shortcut_targets, 决定是否流氓。

use crate::profile::{load_profiles, Profile};
use anyhow::Result;
use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize)]
pub struct ShortcutItem {
    pub path: String,
    pub name: String,
    pub target: String,
    pub matched_profile_id: Option<String>,
    pub matched_profile_name: Option<String>,
}

fn shortcut_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(u) = std::env::var("USERPROFILE") {
        out.push(PathBuf::from(&u).join("Desktop"));
    }
    if let Ok(p) = std::env::var("PUBLIC") {
        out.push(PathBuf::from(&p).join("Desktop"));
    }
    if let Ok(a) = std::env::var("APPDATA") {
        out.push(PathBuf::from(&a).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    if let Ok(pd) = std::env::var("ProgramData") {
        out.push(PathBuf::from(&pd).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    out
}

fn resolve_lnk_target(path: &PathBuf) -> Option<String> {
    // 用 PowerShell + WScript.Shell COM 解析快捷方式
    let cmd = format!(
        r#"$s = New-Object -ComObject WScript.Shell; ($s.CreateShortcut('{}')).TargetPath"#,
        path.display().to_string().replace('\'', "''")
    );
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", &cmd])
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

fn match_profile(target: &str, profiles: &[Profile]) -> Option<(String, String)> {
    let target_lc = target.to_ascii_lowercase();
    let target_name = std::path::Path::new(target)
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();

    for p in profiles {
        // 匹配 process_names
        for pn in &p.fingerprints.process_names {
            if target_name == pn.to_ascii_lowercase() {
                return Some((p.id.clone(), p.name.clone()));
            }
        }
        // 匹配 install_path_keywords
        for kw in &p.fingerprints.install_path_keywords {
            if target_lc.contains(&kw.to_ascii_lowercase()) {
                return Some((p.id.clone(), p.name.clone()));
            }
        }
        // 匹配 shortcut_targets
        for st in &p.fingerprints.shortcut_targets {
            if target_name == st.to_ascii_lowercase() {
                return Some((p.id.clone(), p.name.clone()));
            }
        }
    }
    None
}

pub fn scan_all() -> Result<Vec<ShortcutItem>> {
    let profiles = load_profiles().unwrap_or_default();
    let mut out = Vec::new();
    for dir in shortcut_dirs() {
        if !dir.is_dir() { continue; }
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if ext != "lnk" { continue; }
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            let target = resolve_lnk_target(&path).unwrap_or_default();
            let matched = match_profile(&target, &profiles);
            out.push(ShortcutItem {
                path: path.display().to_string(),
                name,
                target,
                matched_profile_id: matched.as_ref().map(|m| m.0.clone()),
                matched_profile_name: matched.map(|m| m.1),
            });
        }
    }
    Ok(out)
}

/// 只返回命中流氓画像的快捷方式
pub fn scan_rogue() -> Result<Vec<ShortcutItem>> {
    Ok(scan_all()?
        .into_iter()
        .filter(|i| i.matched_profile_id.is_some())
        .collect())
}
