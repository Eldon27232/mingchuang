//! 用户的"我把这些扩展名给了哪个 app"分配清单, 持久化到磁盘
//!
//! %LOCALAPPDATA%\mingchuang\assoc-assignments.json
//!
//! - 每次用户点"设为默认"会 merge 到这里
//! - 主窗口顶部"恢复我的设置"按钮调 apply_all_from_manifest 重新应用所有
//!   (应对软件升级/Windows 更新等导致关联被覆盖的情况)

use crate::fileassoc;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AssocManifest {
    /// app_key -> { display_name, exe_path, extensions }
    pub apps: Vec<AssocApp>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssocApp {
    pub key: String,
    pub display_name: String,
    pub exe_path: String,
    pub extensions: Vec<String>,
}

fn manifest_path() -> PathBuf {
    let local = std::env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    local.join("mingchuang").join("assoc-assignments.json")
}

pub fn load() -> AssocManifest {
    let p = manifest_path();
    if !p.is_file() {
        return AssocManifest::default();
    }
    std::fs::read_to_string(&p)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save(m: &AssocManifest) -> Result<()> {
    let p = manifest_path();
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("创建 {parent:?} 失败"))?;
    }
    let json = serde_json::to_string_pretty(m)?;
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, json).with_context(|| format!("写 {tmp:?} 失败"))?;
    std::fs::rename(&tmp, &p).with_context(|| format!("重命名 {p:?} 失败"))?;
    Ok(())
}

/// 把某个 app 的扩展名设置 merge 到 manifest: 该 app 的 extensions 全替换, 其他 app
/// 若有重复 ext 则**从它们身上移除**(冲突让最新分配胜出)。
pub fn upsert_app(app: AssocApp) -> Result<AssocManifest> {
    let mut m = load();
    let new_exts: std::collections::HashSet<String> =
        app.extensions.iter().cloned().collect();
    // 从其他 app 移除冲突的 ext
    for other in m.apps.iter_mut() {
        if other.key != app.key {
            other.extensions.retain(|e| !new_exts.contains(e));
        }
    }
    // 清理 extensions 全空的 app
    m.apps.retain(|a| !a.extensions.is_empty() || a.key == app.key);
    // upsert
    if let Some(existing) = m.apps.iter_mut().find(|a| a.key == app.key) {
        *existing = app;
    } else {
        m.apps.push(app);
    }
    save(&m)?;
    Ok(m)
}

pub fn remove_app(key: &str) -> Result<AssocManifest> {
    let mut m = load();
    m.apps.retain(|a| a.key != key);
    save(&m)?;
    Ok(m)
}

/// 把 manifest 里所有 app 的 extensions 都重新写到系统关联(一键恢复用)
pub fn apply_all() -> Result<ApplyAllResult> {
    let m = load();
    let mut applied = 0;
    let mut failed = Vec::new();
    for app in &m.apps {
        if app.extensions.is_empty() {
            continue;
        }
        match fileassoc::set_app_defaults(&app.exe_path, &app.extensions) {
            Ok(r) => {
                applied += r.extensions_set.len();
                for (ext, why) in r.extensions_failed {
                    failed.push((app.display_name.clone(), ext, why));
                }
            }
            Err(e) => {
                failed.push((app.display_name.clone(), String::new(), format!("{e:#}")));
            }
        }
    }
    Ok(ApplyAllResult {
        total_apps: m.apps.len(),
        applied_extensions: applied,
        failed,
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct ApplyAllResult {
    pub total_apps: usize,
    pub applied_extensions: usize,
    /// (display_name, ext, reason)
    pub failed: Vec<(String, String, String)>,
}
