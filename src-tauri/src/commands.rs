use crate::inventory::namespace::{scan_pc_namespace_items, PcNamespaceItem};
use crate::profile::{load_profiles, Profile};

#[tauri::command]
pub fn list_profiles() -> Result<Vec<Profile>, String> {
    load_profiles().map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn scan_pc_namespace() -> Result<Vec<PcNamespaceItem>, String> {
    scan_pc_namespace_items().map_err(|e| format!("{e:#}"))
}
