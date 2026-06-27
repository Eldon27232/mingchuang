use crate::action::{self, ActionPlan, ExecResult};
use crate::ai::agent::{self, ApprovalDecision, Session};
use crate::ai::config::{self as ai_config, AiConfig};
use crate::elevation::{self, ElevationStatus};
use crate::fileassoc::detect::InstalledApp;
use crate::fileassoc::manifest::{self as assoc_manifest, AssocApp, AssocManifest, ApplyAllResult};
use crate::fileassoc::{self, AssocPreset, AssocResult};
use crate::govern::{self, ScenarioRunResult, ScenarioStats};
use crate::inventory::namespace::{scan_pc_namespace_items, PcNamespaceItem};
use crate::profile::{load_profiles, Profile};
use crate::snapshot::{self, SnapshotManifest};

#[tauri::command]
pub fn check_elevation() -> ElevationStatus {
    elevation::check_elevation()
}

#[tauri::command]
pub fn relaunch_as_admin() -> Result<(), String> {
    elevation::relaunch_as_admin()
}

#[tauri::command]
pub fn fileassoc_detect_installed_apps() -> Vec<InstalledApp> {
    fileassoc::detect::detect_installed()
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

// ============ 治理场景 ============

#[tauri::command]
pub fn govern_scan_pc_namespace() -> Result<ScenarioStats, String> {
    govern::scan_pc_namespace().map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn govern_clean_pc_namespace() -> Result<ScenarioRunResult, String> {
    govern::clean_pc_namespace().map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn govern_scan_keepalive() -> Result<ScenarioStats, String> {
    govern::scan_keepalive_services().map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn govern_stop_keepalive() -> Result<ScenarioRunResult, String> {
    govern::stop_keepalive_services().map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn govern_scan_shortcuts() -> Result<ScenarioStats, String> {
    govern::scan_rogue_shortcuts().map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn govern_clean_shortcuts() -> Result<ScenarioRunResult, String> {
    govern::clean_rogue_shortcuts().map_err(|e| format!("{e:#}"))
}

// ============ 默认打开方式 ============

#[tauri::command]
pub fn fileassoc_list_presets() -> Vec<AssocPreset> {
    fileassoc::list_presets()
}

#[tauri::command]
pub fn fileassoc_set_app_defaults(
    exe_path: String,
    extensions: Vec<String>,
) -> Result<AssocResult, String> {
    fileassoc::set_app_defaults(&exe_path, &extensions).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn fileassoc_get_manifest() -> AssocManifest {
    assoc_manifest::load()
}

#[tauri::command]
pub fn fileassoc_upsert_app(app: AssocApp) -> Result<AssocManifest, String> {
    assoc_manifest::upsert_app(app).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn fileassoc_remove_app(key: String) -> Result<AssocManifest, String> {
    assoc_manifest::remove_app(&key).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn fileassoc_apply_all() -> Result<ApplyAllResult, String> {
    assoc_manifest::apply_all().map_err(|e| format!("{e:#}"))
}

// ============ AI ============

#[tauri::command]
pub fn ai_get_config() -> AiConfig {
    ai_config::redact(&ai_config::load())
}

#[tauri::command]
pub fn ai_set_config(cfg: AiConfig) -> Result<(), String> {
    let mut to_save = cfg.clone();
    // 脱敏检测要严格,否则用户粘贴"sk-test-...abc" 这种含 ... 的真实 key 会被丢
    // 我们的 redact 格式固定: 长度 13、第 6-8 位是"...", 用这个特征
    let is_redacted = to_save.api_key == "****"
        || (to_save.api_key.chars().count() == 13
            && to_save.api_key.chars().skip(6).take(3).collect::<String>() == "...");
    if is_redacted {
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
pub fn ai_list_sessions() -> Vec<agent::SessionSummary> {
    agent::list_persisted_sessions()
}

#[tauri::command]
pub fn ai_delete_session(session_id: String) -> Result<(), String> {
    agent::delete_session(&session_id).map_err(|e| format!("{e:#}"))
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

#[tauri::command]
pub fn ai_abort_session(session_id: String) {
    agent::abort_session(&session_id);
}

#[tauri::command]
pub async fn ai_retry_last(session_id: String) -> Result<(), String> {
    agent::retry_last(&session_id)
        .await
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub async fn ai_edit_user_message(
    session_id: String,
    msg_index: usize,
    new_content: String,
) -> Result<(), String> {
    agent::edit_user_message(&session_id, msg_index, new_content)
        .await
        .map_err(|e| format!("{e:#}"))
}
