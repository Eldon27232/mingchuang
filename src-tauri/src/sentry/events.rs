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
    write_jsonl(&event)
}

/// 巡检/偷改告警 event — 跟 PCDN AlertEvent 共用同一个 JSONL, 用 kind 字段区分
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectionLogEntry {
    pub ts: String,
    pub kind: String,
    pub category: String,
    pub label: String,
    pub detail: String,
}

pub fn append_inspection(ev: &kuake_fuckyou_lib::inspection::ChangeEvent) -> Result<()> {
    let entry = InspectionLogEntry {
        ts: ev.ts.to_rfc3339(),
        kind: match ev.kind {
            kuake_fuckyou_lib::inspection::ChangeKind::Added => "added".into(),
            kuake_fuckyou_lib::inspection::ChangeKind::UserChoiceLost => "userchoice_lost".into(),
        },
        category: ev.category.clone(),
        label: ev.label.clone(),
        detail: ev.detail.clone(),
    };
    write_jsonl(&entry)
}

fn write_jsonl<T: Serialize>(item: &T) -> Result<()> {
    let path = events_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("创建 {parent:?} 失败"))?;
    }
    let line = serde_json::to_string(item)? + "\n";
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("打开 {path:?} 失败"))?;
    f.write_all(line.as_bytes())?;
    Ok(())
}
