//! 共享文件 IPC - state.json (sentry 写, GUI 读) + control.json (GUI 写, sentry 读)

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SentryState {
    pub started_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_alert_at: Option<DateTime<Utc>>,
    pub paused_until: Option<DateTime<Utc>>,
    pub alerts_total: u64,
    pub monitored_pids: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ControlFile {
    /// GUI 写入: 暂停到何时
    #[serde(default)]
    pub paused_until: Option<DateTime<Utc>>,
    /// GUI 写入: 请求停止
    #[serde(default)]
    pub stop_requested: bool,
}

pub fn state_path() -> PathBuf {
    crate::whitelist::sentry_dir().join("state.json")
}

pub fn control_path() -> PathBuf {
    crate::whitelist::sentry_dir().join("control.json")
}

pub fn write_state(s: &SentryState) -> Result<()> {
    let path = state_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("创建 {parent:?} 失败"))?;
    }
    let json = serde_json::to_string_pretty(s)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

pub fn read_control() -> ControlFile {
    let path = control_path();
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}
