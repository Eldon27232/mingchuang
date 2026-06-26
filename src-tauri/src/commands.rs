use crate::action::{self, ActionPlan, ExecResult};
use crate::inventory::namespace::{scan_pc_namespace_items, PcNamespaceItem};
use crate::profile::{load_profiles, Profile};
use crate::snapshot::{self, SnapshotManifest};

#[tauri::command]
pub fn list_profiles() -> Result<Vec<Profile>, String> {
    load_profiles().map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn scan_pc_namespace() -> Result<Vec<PcNamespaceItem>, String> {
    scan_pc_namespace_items().map_err(|e| format!("{e:#}"))
}

/// dry-run: 返回某画像所有动作的预览
#[tauri::command]
pub fn dry_run_profile(profile_id: String) -> Result<Vec<ActionPlan>, String> {
    let profiles = load_profiles().map_err(|e| format!("{e:#}"))?;
    let p = profiles
        .into_iter()
        .find(|p| p.id == profile_id)
        .ok_or_else(|| format!("画像未找到: {profile_id}"))?;
    Ok(action::plan_profile(&p))
}

/// 执行某画像的某个动作 (按 action_index)
#[tauri::command]
pub fn execute_profile_action(profile_id: String, action_index: usize) -> Result<ExecResult, String> {
    let profiles = load_profiles().map_err(|e| format!("{e:#}"))?;
    let p = profiles
        .into_iter()
        .find(|p| p.id == profile_id)
        .ok_or_else(|| format!("画像未找到: {profile_id}"))?;
    Ok(action::execute_action(&p, action_index))
}

#[tauri::command]
pub fn list_snapshots() -> Result<Vec<SnapshotManifest>, String> {
    snapshot::list_snapshots().map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn restore_snapshot(snapshot_id: String) -> Result<(), String> {
    action::restore_snapshot(&snapshot_id).map_err(|e| format!("{e:#}"))
}
