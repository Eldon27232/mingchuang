mod action;
mod ai;
mod commands;
mod elevation;
mod fileassoc;
mod govern;
mod inventory;
mod profile;
mod sentry_client;
mod snapshot;
mod sys_cmd;
mod whitelist;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .invoke_handler(tauri::generate_handler![
            commands::check_elevation,
            commands::relaunch_as_admin,
            commands::fileassoc_detect_installed_apps,
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
            commands::fileassoc_get_manifest,
            commands::fileassoc_upsert_app,
            commands::fileassoc_remove_app,
            commands::fileassoc_apply_all,
            commands::fileassoc_open_settings,
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
            // Sentry 后台监控
            commands::sentry_get_status,
            commands::sentry_enable_autostart,
            commands::sentry_disable_autostart,
            commands::sentry_start_now,
            commands::sentry_pause,
            commands::sentry_resume,
            commands::sentry_stop,
            commands::sentry_list_events,
            commands::sentry_get_whitelist,
            commands::sentry_whitelist_add,
            commands::sentry_whitelist_remove,
        ])
        .run(tauri::generate_context!())
        .expect("error while running kuake-fuckyou");
}
