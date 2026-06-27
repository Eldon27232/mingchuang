//! 注册自定义 ProgId 到 HKCU\Software\Classes
//!
//! ProgId 格式: `KuakeFuckyou.<stem>`
//! 注册项:
//!   (默认) = "<stem> 文件 (kuake-fuckyou)"
//!   DefaultIcon\(默认) = "<exe>,0"
//!   shell\open\command\(默认) = "\"<exe>\" \"%1\""

use anyhow::{Context, Result};
use std::path::Path;
use windows_registry::CURRENT_USER;

pub fn register(exe_path: &Path) -> Result<String> {
    let stem = exe_path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow::anyhow!("无法取 exe 文件名"))?;
    let progid = format!("KuakeFuckyou.{stem}");
    let base = format!("Software\\Classes\\{progid}");

    let root = CURRENT_USER.create(&base).with_context(|| format!("创建 {base} 失败"))?;
    let _ = root.set_string("", &format!("{stem} 文件 (kuake-fuckyou)"));

    let icon = CURRENT_USER.create(&format!("{base}\\DefaultIcon"))?;
    let _ = icon.set_string("", &format!("{},0", exe_path.display()));

    let cmd = CURRENT_USER.create(&format!("{base}\\shell\\open\\command"))?;
    let _ = cmd.set_string("", &format!("\"{}\" \"%1\"", exe_path.display()));

    Ok(progid)
}
