//! 注册表动作的薄包装 — 复用 snapshot::reg 的底层
//!
//! 设计原因: action 层只需要 (read 子树, count, delete) 三个高层操作。
//! 完整的 read_subtree / write_subtree 实现在 snapshot::reg。

use crate::snapshot::reg::{self as snap_reg, RegSubtree};
use anyhow::Result;

/// dry-run 预览: 返回 (键数, 值数)
pub fn snapshot_path_count(hive: &str, path: &str) -> Result<(usize, usize)> {
    let tree = snap_reg::read_subtree(hive, path)?;
    Ok(snap_reg::count_subtree(&tree))
}

pub fn read_subtree(hive: &str, path: &str) -> Result<RegSubtree> {
    snap_reg::read_subtree(hive, path)
}

pub fn delete_path(hive: &str, path: &str) -> Result<()> {
    snap_reg::delete_subtree(hive, path)
}
