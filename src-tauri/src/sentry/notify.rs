//! Windows toast 通知 - Phase 1 简单展示(无 actionable callback)
//!
//! 用 notify-rust crate。Phase 2 改 winrt + COM callback 实现按钮回传。

use anyhow::Result;

pub fn show_toast(title: &str, body: &str) -> Result<()> {
    notify_rust::Notification::new()
        .summary(title)
        .body(body)
        .appname("明窗")
        .show()
        .map_err(|e| anyhow::anyhow!("toast 失败: {e}"))?;
    Ok(())
}
