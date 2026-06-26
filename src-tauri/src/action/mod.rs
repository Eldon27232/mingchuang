//! 动作引擎 — 支持 reg-delete / service-stop / service-disable / process-kill / task-disable

pub mod process;
pub mod reg;
pub mod service;
pub mod task;

use crate::elevation;
use crate::profile::{Action, Profile};
use crate::snapshot::{
    self,
    service::{self as svc_snap, ServiceSnapshot},
    SnapshotManifest,
};
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
    pub blocked: bool,
    pub blocked_reason: Option<String>,
    pub will_change: String,
    /// 是否可逆(process-kill 等不可逆)
    pub reversible: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExecResult {
    pub plan: ActionPlan,
    pub snapshot_id: Option<String>,
    pub success: bool,
    pub error: Option<String>,
}

const REVERSIBLE_KINDS: &[&str] = &["reg-delete", "service-stop", "service-disable", "task-disable"];
const SUPPORTED_KINDS: &[&str] = &[
    "reg-delete",
    "service-stop",
    "service-disable",
    "task-disable",
    "process-kill",
];

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
        reversible: REVERSIBLE_KINDS.contains(&action.kind.as_str()),
    };

    // 提权护栏: 需要 elevate 的动作, 必须当前进程已提权
    if action.elevate {
        if !elevation::check_elevation().is_elevated {
            plan.blocked = true;
            plan.blocked_reason = Some("此动作需管理员权限。请右键 → 以管理员身份运行 重启本工具。".into());
            plan.will_change = "(未提权, 不会执行)".into();
            return plan;
        }
    }

    if !SUPPORTED_KINDS.contains(&action.kind.as_str()) {
        plan.blocked = true;
        plan.blocked_reason = Some(format!("kind={} 尚未实现", action.kind));
        plan.will_change = "(未实现的动作类型)".into();
        return plan;
    }

    match action.kind.as_str() {
        "reg-delete" => plan_reg_delete(&mut plan, action),
        "service-stop" => plan_service_stop(&mut plan, action),
        "service-disable" => plan_service_disable(&mut plan, action),
        "task-disable" => plan_task_disable(&mut plan, action),
        "process-kill" => plan_process_kill(&mut plan, action),
        _ => {} // 已被 SUPPORTED_KINDS 拦
    }
    plan
}

fn plan_reg_delete(plan: &mut ActionPlan, action: &Action) {
    if let Some(reason) = whitelist::is_registry_protected(&action.target) {
        plan.blocked = true;
        plan.blocked_reason = Some(format!("白名单护栏命中: {reason}"));
        plan.will_change = "(被白名单拒绝)".into();
        return;
    }
    let Some((hive, path)) = whitelist::split_hive(&action.target) else {
        plan.blocked = true;
        plan.blocked_reason = Some(format!("无法识别 hive: {}", action.target));
        plan.will_change = "(目标格式错误)".into();
        return;
    };
    match reg::snapshot_path_count(hive, &path) {
        Ok((keys, vals)) => {
            plan.will_change = if keys == 0 && vals == 0 {
                "目标键不存在, 跳过 (no-op)".into()
            } else {
                format!("将删除 {keys} 个键 + {vals} 个值 (含子树, 自动快照)")
            };
        }
        Err(e) => plan.will_change = format!("预览读取失败: {e:#}"),
    }
}

fn plan_service_stop(plan: &mut ActionPlan, action: &Action) {
    match svc_snap::snapshot(&action.target) {
        Ok(s) => {
            plan.will_change = format!(
                "停止服务 {} (当前 {} / 启动类型 {})",
                action.target, s.original_state, s.original_start_type
            );
        }
        Err(e) => {
            plan.will_change = format!("(无法读取服务状态: {e:#})");
        }
    }
}

fn plan_service_disable(plan: &mut ActionPlan, action: &Action) {
    match svc_snap::snapshot(&action.target) {
        Ok(s) => {
            plan.will_change = format!(
                "改启动类型为 Disabled (原 {})", s.original_start_type
            );
            if s.original_start_type.eq_ignore_ascii_case("Disabled") {
                plan.will_change = "服务已 Disabled, 跳过 (no-op)".into();
            }
        }
        Err(e) => plan.will_change = format!("(无法读取服务配置: {e:#})"),
    }
}

fn plan_task_disable(plan: &mut ActionPlan, action: &Action) {
    if !task::task_exists(&action.target) {
        plan.will_change = "计划任务不存在, 跳过 (no-op)".into();
        return;
    }
    plan.will_change = format!("禁用计划任务 {} (XML 已快照)", action.target);
}

fn plan_process_kill(plan: &mut ActionPlan, action: &Action) {
    let n = process::preview_count(&action.target);
    plan.will_change = if n == 0 {
        format!("无 {} 进程在跑, 跳过 (no-op)", action.target)
    } else {
        format!("⚠ 将杀掉 {} 个 {} 进程 (不可逆)", n, action.target)
    };
}

pub fn plan_profile(profile: &Profile) -> Vec<ActionPlan> {
    profile
        .actions
        .iter()
        .enumerate()
        .map(|(i, a)| plan_action(profile, i, a))
        .collect()
}

pub fn execute_action(profile: &Profile, action_index: usize) -> ExecResult {
    let action = match profile.actions.get(action_index) {
        Some(a) => a,
        None => {
            return blocked_result(profile, action_index, "action_index 越界");
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

    let dispatch = match action.kind.as_str() {
        "reg-delete" => exec_reg_delete(profile, action_index, action),
        "service-stop" => exec_service_stop(profile, action_index, action),
        "service-disable" => exec_service_disable(profile, action_index, action),
        "task-disable" => exec_task_disable(profile, action_index, action),
        "process-kill" => exec_process_kill(profile, action_index, action),
        other => Err(anyhow!("kind={other} 尚未实现")),
    };

    match dispatch {
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
    }
}

fn blocked_result(profile: &Profile, action_index: usize, reason: &str) -> ExecResult {
    ExecResult {
        plan: ActionPlan {
            profile_id: profile.id.clone(),
            action_index,
            kind: String::new(),
            target: String::new(),
            reason: String::new(),
            elevate: false,
            blocked: true,
            blocked_reason: Some(reason.into()),
            will_change: String::new(),
            reversible: false,
        },
        snapshot_id: None,
        success: false,
        error: Some(reason.into()),
    }
}

// ---------- 各 kind 的执行 ----------

fn make_manifest(
    id: &str,
    profile: &Profile,
    action_index: usize,
    action: &Action,
    payload_file: Option<&str>,
    restorable: bool,
) -> SnapshotManifest {
    SnapshotManifest {
        id: id.into(),
        created_at: Utc::now(),
        profile_id: Some(profile.id.clone()),
        action_index: Some(action_index),
        action_kind: action.kind.clone(),
        action_target: action.target.clone(),
        action_reason: action.reason.clone(),
        payload_file: payload_file.map(String::from),
        restorable,
        restored_at: None,
    }
}

fn exec_reg_delete(profile: &Profile, action_index: usize, action: &Action) -> Result<String> {
    let (hive, path) = whitelist::split_hive(&action.target).ok_or_else(|| anyhow!("hive 解析失败"))?;
    let snap_id = snapshot::new_snapshot_id();
    let dir = snapshot::create_snapshot_dir(&snap_id)?;
    let subtree = reg::read_subtree(hive, &path)?;
    let json = serde_json::to_string_pretty(&subtree)?;
    std::fs::write(dir.join("registry.json"), json)?;
    snapshot::write_manifest(
        &dir,
        &make_manifest(&snap_id, profile, action_index, action, Some("registry.json"), true),
    )?;
    reg::delete_path(hive, &path)?;
    Ok(snap_id)
}

fn exec_service_stop(profile: &Profile, action_index: usize, action: &Action) -> Result<String> {
    let snap_id = snapshot::new_snapshot_id();
    let dir = snapshot::create_snapshot_dir(&snap_id)?;
    let snap = svc_snap::snapshot(&action.target).context("服务快照失败")?;
    let json = serde_json::to_string_pretty(&snap)?;
    std::fs::write(dir.join("service.json"), json)?;
    snapshot::write_manifest(
        &dir,
        &make_manifest(&snap_id, profile, action_index, action, Some("service.json"), true),
    )?;
    service::stop_service(&action.target)?;
    Ok(snap_id)
}

fn exec_service_disable(profile: &Profile, action_index: usize, action: &Action) -> Result<String> {
    let snap_id = snapshot::new_snapshot_id();
    let dir = snapshot::create_snapshot_dir(&snap_id)?;
    let snap = svc_snap::snapshot(&action.target).context("服务快照失败")?;
    let json = serde_json::to_string_pretty(&snap)?;
    std::fs::write(dir.join("service.json"), json)?;
    snapshot::write_manifest(
        &dir,
        &make_manifest(&snap_id, profile, action_index, action, Some("service.json"), true),
    )?;
    service::disable_service(&action.target)?;
    Ok(snap_id)
}

fn exec_task_disable(profile: &Profile, action_index: usize, action: &Action) -> Result<String> {
    let snap_id = snapshot::new_snapshot_id();
    let dir = snapshot::create_snapshot_dir(&snap_id)?;
    let xml = task::query_xml(&action.target).context("查询任务 XML 失败")?;
    std::fs::write(dir.join("task.xml"), xml)?;
    snapshot::write_manifest(
        &dir,
        &make_manifest(&snap_id, profile, action_index, action, Some("task.xml"), true),
    )?;
    task::disable_task(&action.target)?;
    Ok(snap_id)
}

fn exec_process_kill(profile: &Profile, action_index: usize, action: &Action) -> Result<String> {
    let snap_id = snapshot::new_snapshot_id();
    let dir = snapshot::create_snapshot_dir(&snap_id)?;
    let (attempts, success, killed) = process::kill_by_name(&action.target)?;
    let json = serde_json::to_string_pretty(&serde_json::json!({
        "attempts": attempts,
        "success": success,
        "killed": killed,
    }))?;
    std::fs::write(dir.join("process.json"), json)?;
    snapshot::write_manifest(
        &dir,
        &make_manifest(&snap_id, profile, action_index, action, Some("process.json"), false),
    )?;
    Ok(snap_id)
}

// ---------- restore ----------

pub fn restore_snapshot(snapshot_id: &str) -> Result<()> {
    let dir = snapshot::snapshots_root().join(snapshot_id);
    let mut manifest = snapshot::read_manifest(&dir)?;
    if manifest.restored_at.is_some() {
        return Err(anyhow!("此快照已被还原过, 拒绝重复"));
    }
    if !manifest.restorable {
        return Err(anyhow!("此动作不可逆 (kind={}), 无法还原", manifest.action_kind));
    }
    match manifest.action_kind.as_str() {
        "reg-delete" => {
            let txt = std::fs::read_to_string(dir.join("registry.json"))?;
            let tree: crate::snapshot::reg::RegSubtree = serde_json::from_str(&txt)?;
            crate::snapshot::reg::write_subtree(&tree)?;
        }
        "service-stop" => {
            let txt = std::fs::read_to_string(dir.join("service.json"))?;
            let snap: ServiceSnapshot = serde_json::from_str(&txt)?;
            if snap.original_state == "Running" {
                service::start_service(&snap.name)?;
            }
        }
        "service-disable" => {
            let txt = std::fs::read_to_string(dir.join("service.json"))?;
            let snap: ServiceSnapshot = serde_json::from_str(&txt)?;
            let st = service::parse_start_type(&snap.original_start_type)
                .ok_or_else(|| anyhow!("未知 start_type: {}", snap.original_start_type))?;
            service::set_start_type(&snap.name, st)?;
        }
        "task-disable" => {
            task::enable_task(&manifest.action_target)?;
        }
        other => return Err(anyhow!("还原 kind={other} 尚未实现")),
    }
    manifest.restored_at = Some(Utc::now());
    snapshot::write_manifest(&dir, &manifest)?;
    Ok(())
}
