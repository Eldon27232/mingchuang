mod action;
mod commands;
mod elevation;
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running kuake-fuckyou");
}
