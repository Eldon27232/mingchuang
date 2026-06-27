//! 操作快照与还原引擎 (P0 安全地基, v2 支持多种 payload kind)
//!
//! 目录结构:
//!   %LOCALAPPDATA%\kuake-fuckyou\snapshots\<id>\
//!   ├── manifest.json     元数据
//!   └── payload.<ext>     按动作类型: registry.json / service.json / task.xml / process.json

pub mod file;
pub mod reg;
pub mod service;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotManifest {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub profile_id: Option<String>,
    pub action_index: Option<usize>,
    pub action_kind: String,
    pub action_target: String,
    pub action_reason: String,
    /// payload 文件名(相对 snapshot 目录)
    pub payload_file: Option<String>,
    /// 是否可还原 (process-kill 等不可逆动作为 false)
    #[serde(default = "default_true")]
    pub restorable: bool,
    /// 已被还原过的时间
    #[serde(default)]
    pub restored_at: Option<DateTime<Utc>>,
}

fn default_true() -> bool {
    true
}

pub fn snapshots_root() -> PathBuf {
    let local = std::env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    local.join("kuake-fuckyou").join("snapshots")
}

pub fn new_snapshot_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let now = Utc::now();
    let stamp = now.format("%Y%m%dT%H%M%S%.3fZ");
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{stamp}-{n:04x}")
}

pub fn create_snapshot_dir(id: &str) -> Result<PathBuf> {
    let dir = snapshots_root().join(id);
    std::fs::create_dir_all(&dir).with_context(|| format!("创建快照目录失败: {dir:?}"))?;
    Ok(dir)
}

pub fn write_manifest(dir: &Path, m: &SnapshotManifest) -> Result<()> {
    let path = dir.join("manifest.json");
    let json = serde_json::to_string_pretty(m).context("序列化 manifest 失败")?;
    std::fs::write(&path, json).with_context(|| format!("写 manifest 失败: {path:?}"))?;
    Ok(())
}

pub fn read_manifest(dir: &Path) -> Result<SnapshotManifest> {
    let path = dir.join("manifest.json");
    let txt = std::fs::read_to_string(&path).with_context(|| format!("读 manifest 失败: {path:?}"))?;
    serde_json::from_str(&txt).context("解析 manifest 失败")
}

/// 列出所有快照, 按 created_at 倒序
pub fn list_snapshots() -> Result<Vec<SnapshotManifest>> {
    let root = snapshots_root();
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&root)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            match read_manifest(&entry.path()) {
                Ok(m) => out.push(m),
                Err(e) => eprintln!("skip {:?}: {e:#}", entry.path()),
            }
        }
    }
    out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(out)
}
