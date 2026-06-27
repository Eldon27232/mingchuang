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
    let stem_raw = exe_path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow::anyhow!("无法取 exe 文件名"))?;
    // ProgId 不能含 ASCII 控制字符和空格
    let stem: String = stem_raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect();
    if stem.is_empty() {
        return Err(anyhow::anyhow!("exe 文件名清理后为空: {stem_raw}"));
    }
    let progid = format!("KuakeFuckyou.{stem}");
    let base = format!("Software\\Classes\\{progid}");

    let root = CURRENT_USER
        .create(&base)
        .with_context(|| format!("创建 {base} 失败"))?;
    root.set_string("", &format!("{stem_raw} 文件 (kuake-fuckyou)"))
        .context("写 ProgId 显示名失败")?;

    let icon = CURRENT_USER
        .create(&format!("{base}\\DefaultIcon"))
        .context("创建 DefaultIcon 失败")?;
    icon.set_string("", &format!("{},0", exe_path.display()))
        .context("写 DefaultIcon 失败")?;

    let cmd = CURRENT_USER
        .create(&format!("{base}\\shell\\open\\command"))
        .context("创建 shell\\open\\command 失败")?;
    cmd.set_string("", &format!("\"{}\" \"%1\"", exe_path.display()))
        .context("写打开命令失败")?;

    Ok(progid)
}
