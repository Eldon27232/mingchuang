//! 开机自启动 - 写 HKCU Run 键
//!
//! 不用 Service / Task Scheduler / Startup 文件夹:
//!  - Service: Session 0 隔离, toast 弹不到桌面
//!  - Task Scheduler: 创建需管理员 + 弹 UAC
//!  - Startup 文件夹: 用户可见易误删
//!
//! HKCU Run: 零 UAC, 用户在"任务管理器→启动"自己能看到能关掉。

use anyhow::{anyhow, Context, Result};
use windows_registry::CURRENT_USER;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "MingchuangSentry";

pub fn register() -> Result<()> {
    let exe = std::env::current_exe().context("取当前 exe 路径")?;
    let cmd = format!("\"{}\" --autostart", exe.display());
    let key = CURRENT_USER
        .create(RUN_KEY)
        .map_err(|e| anyhow!("打开 Run 键失败: {e}"))?;
    key.set_string(VALUE_NAME, &cmd)
        .map_err(|e| anyhow!("写自启动键失败: {e}"))?;
    Ok(())
}

pub fn unregister() -> Result<()> {
    let key = CURRENT_USER
        .create(RUN_KEY)
        .map_err(|e| anyhow!("打开 Run 键失败: {e}"))?;
    // 忽略不存在错误
    let _ = key.remove_value(VALUE_NAME);
    Ok(())
}

#[allow(dead_code)]
pub fn is_registered() -> bool {
    CURRENT_USER
        .open(RUN_KEY)
        .and_then(|k| k.get_string(VALUE_NAME))
        .is_ok()
}
