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

pub mod presets;
pub mod progid;

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
    pub extensions_set: Vec<String>,
    pub extensions_failed: Vec<(String, String)>,
    pub userchoice_cleared: Vec<String>,
}

pub fn list_presets() -> Vec<AssocPreset> {
    presets::all()
}

/// 把 extensions 的默认打开方式设给 exe
pub fn set_app_defaults(exe_path: &str, extensions: &[String]) -> Result<AssocResult> {
    let exe = PathBuf::from(exe_path);
    if !exe.is_file() {
        return Err(anyhow::anyhow!("exe 不存在: {exe_path}"));
    }
    let progid = progid::register(&exe).context("注册 ProgId 失败")?;

    let mut extensions_set = Vec::new();
    let mut extensions_failed = Vec::new();
    let mut userchoice_cleared = Vec::new();

    for raw_ext in extensions {
        let ext = normalize_ext(raw_ext);
        match associate_ext(&ext, &progid) {
            Ok(cleared_uc) => {
                extensions_set.push(ext.clone());
                if cleared_uc {
                    userchoice_cleared.push(ext);
                }
            }
            Err(e) => {
                extensions_failed.push((ext, format!("{e:#}")));
            }
        }
    }

    notify_shell_assoc_changed();

    Ok(AssocResult {
        exe: exe.display().to_string(),
        progid,
        extensions_set,
        extensions_failed,
        userchoice_cleared,
    })
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
    // 默认值类型 REG_NONE / 空, 这里用空字符串
    let _ = key.set_string(progid, "");

    // 2. 清现有 UserChoice(让系统从 OpenWithProgids 选)
    let uc_path = format!("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\FileExts\\{ext}\\UserChoice");
    let mut cleared = false;
    if CURRENT_USER.open(&uc_path).is_ok() {
        if CURRENT_USER.remove_tree(&uc_path).is_ok() {
            cleared = true;
        }
    }

    Ok(cleared)
}

/// 通知 Shell 关联已变 — 用 PowerShell 调 SHChangeNotify 比 P/Invoke 简单
fn notify_shell_assoc_changed() {
    let _ = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            r#"Add-Type -Namespace W -Name S -MemberDefinition '[DllImport("shell32.dll")] public static extern void SHChangeNotify(int e, int f, IntPtr i, IntPtr j);'; [W.S]::SHChangeNotify(0x08000000, 0, [IntPtr]::Zero, [IntPtr]::Zero)"#,
        ])
        .output();
}

#[allow(dead_code)]
fn _ensure_path(_: &Path) {}
