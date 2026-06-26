mod commands;
mod inventory;
mod profile;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::list_profiles,
            commands::scan_pc_namespace,
        ])
        .run(tauri::generate_context!())
        .expect("error while running kuake-fuckyou");
}
