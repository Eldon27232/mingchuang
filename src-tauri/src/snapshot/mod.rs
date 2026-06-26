//! 操作快照与还原引擎 (P0 安全地基)
//!
//! 每个破坏性动作执行前自动创建快照, 写到 `%LOCALAPPDATA%\kuake-fuckyou\snapshots\<id>\`
//! 目录结构:
//!   snapshots/
//!   ├── <snapshot-id>/
//!   │   ├── manifest.json     元数据 + 动作描述 + 子文件清单
//!   │   └── registry.json     注册表子树 (reg-delete / reg-set 用)
//!
//! id 用 RFC3339 时间戳 + 6 位随机后缀, 排序即时间序。

pub mod reg;

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
    /// 是否已经被还原 (避免重复 restore)
    #[serde(default)]
    pub restored_at: Option<DateTime<Utc>>,
    /// 子文件列表 (相对路径)
    #[serde(default)]
    pub artifacts: Vec<String>,
}

/// 快照根目录
pub fn snapshots_root() -> PathBuf {
    let local = std::env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    local.join("kuake-fuckyou").join("snapshots")
}

/// 生成快照 id (排序即时间序)
pub fn new_snapshot_id() -> String {
    // chrono 提供时间, 但禁用 Math.random — 这里用进程内单调计数器避免重名碰撞
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let now = Utc::now();
    let stamp = now.format("%Y%m%dT%H%M%S%.3fZ");
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{stamp}-{n:04x}")
}

/// 创建一个快照目录
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
