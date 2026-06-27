//! 桌面/开始菜单快捷方式扫描
//!
//! 用 Win32 IShellLinkW COM 解析 .lnk target,**不再起 PowerShell 子进程**,
//! 桌面 30+ 个 .lnk 扫描 < 50ms。

use crate::profile::{load_profiles, Profile};
use anyhow::{Context, Result};
use serde::Serialize;
use std::path::PathBuf;
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Storage::FileSystem::WIN32_FIND_DATAW;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED, STGM_READ,
};
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink, SLGP_RAWPATH};

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

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 用 COM 读 .lnk 目标。调用方需自己 CoInitialize(批处理时只 init 一次)
unsafe fn read_lnk_with_com(path: &str) -> Result<String> {
    let shell_link: IShellLinkW =
        CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).context("CoCreateInstance")?;
    let persist: IPersistFile = windows::core::Interface::cast(&shell_link).context("cast IPersistFile")?;
    let path_w = to_wide(path);
    persist.Load(PCWSTR(path_w.as_ptr()), STGM_READ).context("IPersistFile::Load")?;

    let mut buf = vec![0u16; 2048];
    let mut wfd = WIN32_FIND_DATAW::default();
    shell_link
        .GetPath(&mut buf, &mut wfd, SLGP_RAWPATH.0 as u32)
        .context("IShellLink::GetPath")?;
    let _ = PWSTR(buf.as_mut_ptr()); // silence unused
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Ok(String::from_utf16_lossy(&buf[..len]))
}

fn match_profile(target: &str, file_name: &str, profiles: &[Profile]) -> Option<(String, String)> {
    let target_lc = target.to_ascii_lowercase();
    let target_name = std::path::Path::new(target)
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let lnk_name_lc = file_name.to_ascii_lowercase();

    for p in profiles {
        for pn in &p.fingerprints.process_names {
            let pn_lc = pn.to_ascii_lowercase();
            if target_name == pn_lc {
                return Some((p.id.clone(), p.name.clone()));
            }
        }
        for kw in &p.fingerprints.install_path_keywords {
            if target_lc.contains(&kw.to_ascii_lowercase()) {
                return Some((p.id.clone(), p.name.clone()));
            }
        }
        for st in &p.fingerprints.shortcut_targets {
            if target_name == st.to_ascii_lowercase() {
                return Some((p.id.clone(), p.name.clone()));
            }
        }
        // 兜底: 用画像 name 做 .lnk 文件名包含匹配 (例如 "酷狗音乐.lnk" → 酷狗画像)
        if !p.name.is_empty() && lnk_name_lc.contains(&p.name.to_ascii_lowercase()) {
            return Some((p.id.clone(), p.name.clone()));
        }
    }
    None
}

pub fn scan_all() -> Result<Vec<ShortcutItem>> {
    let profiles = load_profiles().unwrap_or_default();
    let mut out = Vec::new();

    unsafe {
        let _hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        // 不管成功失败都尝试调 COM, 如果是 RPC_E_CHANGED_MODE 也无碍 (有人在外面 init 过了)

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
                let path_str = path.display().to_string();
                let target = read_lnk_with_com(&path_str).unwrap_or_default();
                let matched = match_profile(&target, &name, &profiles);
                out.push(ShortcutItem {
                    path: path_str,
                    name,
                    target,
                    matched_profile_id: matched.as_ref().map(|m| m.0.clone()),
                    matched_profile_name: matched.map(|m| m.1),
                });
            }
        }

        CoUninitialize();
    }

    Ok(out)
}

pub fn scan_rogue() -> Result<Vec<ShortcutItem>> {
    Ok(scan_all()?
        .into_iter()
        .filter(|i| i.matched_profile_id.is_some())
        .collect())
}
