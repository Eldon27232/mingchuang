//! 动作引擎: dry-run / execute / restore
//!
//! 流程:
//!   1. plan(profile, action) -> ActionPlan       (dry-run, 描述将做什么)
//!   2. execute(plan) -> ExecResult               (先建快照, 再执行)
//!   3. restore(snapshot_id) -> Result            (从快照恢复)
//!
//! 当前支持: `reg-delete`。其他 kind 走 unsupported 分支并提示。

pub mod reg;

use crate::profile::{Action, Profile};
use crate::snapshot::{self, SnapshotManifest};
use crate::whitelist;
use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ActionPlan {
    pub profile_id: String,
    pub action_index: usize,
    pub kind: String,
    pub target: String,
    pub reason: String,
    pub elevate: bool,
    /// 是否被白名单拒绝
    pub blocked: bool,
    pub blocked_reason: Option<String>,
    /// 人话摘要: "将删除 X 个键 + Y 个值" 这种
    pub will_change: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExecResult {
    pub plan: ActionPlan,
    pub snapshot_id: Option<String>,
    pub success: bool,
    pub error: Option<String>,
}

/// dry-run: 单个 action 的 plan
pub fn plan_action(profile: &Profile, action_index: usize, action: &Action) -> ActionPlan {
    let mut plan = ActionPlan {
        profile_id: profile.id.clone(),
        action_index,
        kind: action.kind.clone(),
        target: action.target.clone(),
        reason: action.reason.clone(),
        elevate: action.elevate,
        blocked: false,
        blocked_reason: None,
        will_change: String::new(),
    };
    match action.kind.as_str() {
        "reg-delete" => {
            if let Some(reason) = whitelist::is_registry_protected(&action.target) {
                plan.blocked = true;
                plan.blocked_reason = Some(format!("白名单护栏命中 (受保护前缀: {reason})"));
                plan.will_change = "(被白名单拒绝, 不会执行)".into();
                return plan;
            }
            let Some((hive, path)) = whitelist::split_hive(&action.target) else {
                plan.blocked = true;
                plan.blocked_reason = Some(format!("无法识别注册表 hive: {}", action.target));
                plan.will_change = "(目标格式错误, 不会执行)".into();
                return plan;
            };
            match reg::snapshot_path_count(hive, &path) {
                Ok((keys, vals)) => {
                    if keys == 0 && vals == 0 {
                        plan.will_change = "目标键不存在, 跳过 (no-op)".into();
                    } else {
                        plan.will_change = format!("将删除 {keys} 个键 + {vals} 个值 (含子树, 已快照)");
                    }
                }
                Err(e) => {
                    plan.will_change = format!("预览读取失败: {e:#}");
                }
            }
        }
        other => {
            plan.blocked = true;
            plan.blocked_reason = Some(format!("kind={other} 尚未实现 (本轮仅 reg-delete)"));
            plan.will_change = "(未实现的动作类型, 不会执行)".into();
        }
    }
    plan
}

/// dry-run 整个 profile
pub fn plan_profile(profile: &Profile) -> Vec<ActionPlan> {
    profile
        .actions
        .iter()
        .enumerate()
        .map(|(i, a)| plan_action(profile, i, a))
        .collect()
}

/// execute: 走快照 → 执行
pub fn execute_action(profile: &Profile, action_index: usize) -> ExecResult {
    let action = match profile.actions.get(action_index) {
        Some(a) => a,
        None => {
            return ExecResult {
                plan: ActionPlan {
                    profile_id: profile.id.clone(),
                    action_index,
                    kind: String::new(),
                    target: String::new(),
                    reason: String::new(),
                    elevate: false,
                    blocked: true,
                    blocked_reason: Some("action_index 越界".into()),
                    will_change: String::new(),
                },
                snapshot_id: None,
                success: false,
                error: Some("action_index 越界".into()),
            };
        }
    };
    let plan = plan_action(profile, action_index, action);
    if plan.blocked {
        return ExecResult {
            plan: plan.clone(),
            snapshot_id: None,
            success: false,
            error: plan.blocked_reason.clone(),
        };
    }

    match action.kind.as_str() {
        "reg-delete" => match exec_reg_delete(profile, action_index, action, &plan) {
            Ok(snap_id) => ExecResult {
                plan,
                snapshot_id: Some(snap_id),
                success: true,
                error: None,
            },
            Err(e) => ExecResult {
                plan,
                snapshot_id: None,
                success: false,
                error: Some(format!("{e:#}")),
            },
        },
        other => ExecResult {
            plan,
            snapshot_id: None,
            success: false,
            error: Some(format!("kind={other} 尚未实现")),
        },
    }
}

fn exec_reg_delete(profile: &Profile, action_index: usize, action: &Action, plan: &ActionPlan) -> Result<String> {
    let (hive, path) = whitelist::split_hive(&action.target).ok_or_else(|| anyhow!("解析 hive 失败"))?;
    // 1. 拍快照
    let snap_id = snapshot::new_snapshot_id();
    let dir = snapshot::create_snapshot_dir(&snap_id)?;
    let subtree = reg::read_subtree(hive, &path).context("读子树失败")?;
    let reg_path = dir.join("registry.json");
    let json = serde_json::to_string_pretty(&subtree).context("序列化子树")?;
    std::fs::write(&reg_path, json).with_context(|| format!("写快照文件: {reg_path:?}"))?;
    // 2. 写 manifest
    let manifest = SnapshotManifest {
        id: snap_id.clone(),
        created_at: Utc::now(),
        profile_id: Some(profile.id.clone()),
        action_index: Some(action_index),
        action_kind: action.kind.clone(),
        action_target: action.target.clone(),
        action_reason: action.reason.clone(),
        restored_at: None,
        artifacts: vec!["registry.json".into()],
    };
    snapshot::write_manifest(&dir, &manifest)?;
    // 3. 真删
    let _ = plan; // 编译保留
    reg::delete_path(hive, &path)?;
    Ok(snap_id)
}

/// 从快照还原
pub fn restore_snapshot(snapshot_id: &str) -> Result<()> {
    let dir = snapshot::snapshots_root().join(snapshot_id);
    let mut manifest = snapshot::read_manifest(&dir)?;
    if manifest.restored_at.is_some() {
        return Err(anyhow!("此快照已被还原过, 拒绝重复操作"));
    }
    match manifest.action_kind.as_str() {
        "reg-delete" => {
            let reg_path = dir.join("registry.json");
            let txt = std::fs::read_to_string(&reg_path).with_context(|| format!("读 {reg_path:?}"))?;
            let subtree: crate::snapshot::reg::RegSubtree = serde_json::from_str(&txt).context("解析 subtree")?;
            crate::snapshot::reg::write_subtree(&subtree).context("写回子树")?;
        }
        other => return Err(anyhow!("还原 kind={other} 尚未实现")),
    }
    manifest.restored_at = Some(Utc::now());
    snapshot::write_manifest(&dir, &manifest)?;
    Ok(())
}
