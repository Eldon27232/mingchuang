use crate::action::{self, ActionPlan, ExecResult};
use crate::ai::agent::{self, ApprovalDecision, Session};
use crate::ai::config::{self as ai_config, AiConfig};
use crate::elevation::{self, ElevationStatus};
use crate::inventory::namespace::{scan_pc_namespace_items, PcNamespaceItem};
use crate::profile::{load_profiles, Profile};
use crate::snapshot::{self, SnapshotManifest};

#[tauri::command]
pub fn check_elevation() -> ElevationStatus {
    elevation::check_elevation()
}

#[tauri::command]
pub fn list_profiles() -> Result<Vec<Profile>, String> {
    load_profiles().map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn scan_pc_namespace() -> Result<Vec<PcNamespaceItem>, String> {
    scan_pc_namespace_items().map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn dry_run_profile(profile_id: String) -> Result<Vec<ActionPlan>, String> {
    let profiles = load_profiles().map_err(|e| format!("{e:#}"))?;
    let p = profiles
        .into_iter()
        .find(|p| p.id == profile_id)
        .ok_or_else(|| format!("画像未找到: {profile_id}"))?;
    Ok(action::plan_profile(&p))
}

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

// ---------- AI ----------

#[tauri::command]
pub fn ai_get_config() -> AiConfig {
    ai_config::redact(&ai_config::load())
}

#[tauri::command]
pub fn ai_set_config(cfg: AiConfig) -> Result<(), String> {
    // 如果传入 api_key 是脱敏值(含 ...), 保留原 key 不动
    let mut to_save = cfg.clone();
    if to_save.api_key.contains("...") || to_save.api_key == "****" {
        let existing = ai_config::load();
        to_save.api_key = existing.api_key;
    }
    ai_config::save(&to_save).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn ai_create_session() -> String {
    agent::create_session()
}

#[tauri::command]
pub fn ai_get_session(session_id: String) -> Option<Session> {
    agent::get_session(&session_id)
}

#[tauri::command]
pub async fn ai_send_message(session_id: String, message: String) -> Result<(), String> {
    agent::send_user_message(&session_id, message)
        .await
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub async fn ai_approve_pending(
    session_id: String,
    decision: ApprovalDecision,
) -> Result<(), String> {
    agent::approve_pending(&session_id, decision)
        .await
        .map_err(|e| format!("{e:#}"))
}
