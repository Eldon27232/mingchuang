mod action;
mod ai;
mod commands;
mod elevation;
mod fileassoc;
mod govern;
mod inventory;
mod profile;
mod snapshot;
mod whitelist;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::check_elevation,
            commands::list_profiles,
            commands::scan_pc_namespace,
            commands::dry_run_profile,
            commands::execute_profile_action,
            commands::list_snapshots,
            commands::restore_snapshot,
            // 治理场景
            commands::govern_scan_pc_namespace,
            commands::govern_clean_pc_namespace,
            commands::govern_scan_keepalive,
            commands::govern_stop_keepalive,
            commands::govern_scan_shortcuts,
            commands::govern_clean_shortcuts,
            // 默认打开方式
            commands::fileassoc_list_presets,
            commands::fileassoc_set_app_defaults,
            // AI
            commands::ai_get_config,
            commands::ai_set_config,
            commands::ai_create_session,
            commands::ai_list_sessions,
            commands::ai_delete_session,
            commands::ai_get_session,
            commands::ai_send_message,
            commands::ai_approve_pending,
            commands::ai_abort_session,
            commands::ai_retry_last,
            commands::ai_edit_user_message,
        ])
        .run(tauri::generate_context!())
        .expect("error while running kuake-fuckyou");
}
