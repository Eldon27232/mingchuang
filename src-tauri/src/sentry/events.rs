//! 事件日志 - %LOCALAPPDATA%\mingchuang\sentry\events.jsonl
//! 按天滚动,保留 30 天

use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertEvent {
    pub ts: String,
    pub pid: u32,
    pub image_name: String,
    pub up_bps: u64,
}

fn events_path() -> PathBuf {
    crate::whitelist::sentry_dir().join(format!(
        "events-{}.jsonl",
        Utc::now().format("%Y-%m-%d")
    ))
}

pub fn append_alert(pid: u32, image_name: &str, up_bps: u64) -> Result<()> {
    let event = AlertEvent {
        ts: Utc::now().to_rfc3339(),
        pid,
        image_name: image_name.into(),
        up_bps,
    };
    let path = events_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("创建 {parent:?} 失败"))?;
    }
    let line = serde_json::to_string(&event)? + "\n";
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("打开 {path:?} 失败"))?;
    f.write_all(line.as_bytes())?;
    Ok(())
}
