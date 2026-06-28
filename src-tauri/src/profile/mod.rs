//! 画像档案模型与加载器
//!
//! 消费仓库根 `profiles/*.json`。开发期从相对路径找,发布期下一轮做嵌入资源。

use anyhow::{Context, Result};
use include_dir::{include_dir, Dir};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 编译期把仓库根 profiles/ 嵌入二进制 — release 装机后即可用, 不再依赖路径回溯
static EMBEDDED_PROFILES: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../profiles");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub category: String,
    pub severity: String,
    #[serde(default)]
    pub tested_on: Option<serde_json::Value>,
    #[serde(default)]
    pub fingerprints: Fingerprints,
    #[serde(default)]
    pub actions: Vec<Action>,
    #[serde(default)]
    pub verify: Vec<serde_json::Value>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Fingerprints {
    // 强指纹(v0.2 演化新增,优先级最高)
    /// 代码签名主体 CN 名 — 国产流氓最稳定的指纹
    #[serde(default)]
    pub code_sign_subjects: Vec<String>,
    /// MSI ProductCode {GUID}
    #[serde(default)]
    pub msi_product_code: Option<String>,
    /// MSI UpgradeCode {GUID}
    #[serde(default)]
    pub msi_upgrade_code: Option<String>,
    /// Shell 命名空间 / 右键扩展 CLSID
    #[serde(default)]
    pub clsids: Vec<String>,

    // 路径/名称类指纹
    #[serde(default)]
    pub install_paths: Vec<String>,
    /// 模糊安装目录关键字(如 `\7654Browser\`),命中即识别
    #[serde(default)]
    pub install_path_keywords: Vec<String>,
    #[serde(default)]
    pub process_names: Vec<String>,
    #[serde(default)]
    pub service_names: Vec<String>,
    #[serde(default)]
    pub task_names: Vec<String>,
    #[serde(default)]
    pub registry_keys: Vec<String>,
    #[serde(default)]
    pub shortcut_targets: Vec<String>,

    // 网络/行为(为 P2 hosts-block 等动作预留)
    /// 已知上报/广告域名
    #[serde(default)]
    pub report_domains: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Action {
    pub kind: String,
    pub target: String,
    pub reason: String,
    #[serde(default)]
    pub elevate: bool,
    #[serde(default)]
    pub rollback: Option<serde_json::Value>,
}

/// 找 `profiles/` 目录:开发期从 src-tauri/ 往上回溯,发布期暂沿用此逻辑(下一轮做嵌入)。
fn profiles_dir() -> PathBuf {
    // 优先从可执行文件所在目录回溯
    let mut search_roots: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(p) = exe.parent() {
            search_roots.push(p.to_path_buf());
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        search_roots.push(cwd);
    }
    // 编译期 CARGO_MANIFEST_DIR 兜底(开发期最稳)
    search_roots.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")));

    for mut dir in search_roots {
        for _ in 0..6 {
            let candidate = dir.join("profiles");
            if candidate.is_dir() {
                return candidate;
            }
            match dir.parent() {
                Some(parent) => dir = parent.to_path_buf(),
                None => break,
            }
        }
    }
    PathBuf::from("profiles")
}

pub fn load_profiles() -> Result<Vec<Profile>> {
    let mut out = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();

    // 1. 优先磁盘:%LOCALAPPDATA%\mingchuang\profiles 给用户覆盖/扩展用
    let user_dir = user_profiles_dir();
    if user_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&user_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) != Some("json") { continue; }
                let Ok(txt) = std::fs::read_to_string(&path) else { continue; };
                match serde_json::from_str::<Profile>(&txt) {
                    Ok(p) => { seen_ids.insert(p.id.clone()); out.push(p); }
                    Err(e) => eprintln!("用户画像解析失败 {path:?}: {e}"),
                }
            }
        }
    }

    // 2. 开发期: 仓库根 profiles/ (CARGO_MANIFEST_DIR 兜底,只 dev 用)
    #[cfg(debug_assertions)]
    {
        let dev_dir = profiles_dir();
        if dev_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&dev_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|s| s.to_str()) != Some("json") { continue; }
                    let Ok(txt) = std::fs::read_to_string(&path) else { continue; };
                    if let Ok(p) = serde_json::from_str::<Profile>(&txt) {
                        if seen_ids.insert(p.id.clone()) { out.push(p); }
                    }
                }
            }
        }
    }

    // 3. 编译期嵌入: release 包后唯一可靠的画像来源
    for f in EMBEDDED_PROFILES.files() {
        if f.path().extension().and_then(|e| e.to_str()) != Some("json") { continue; }
        let Ok(txt) = std::str::from_utf8(f.contents()) else { continue; };
        match serde_json::from_str::<Profile>(txt) {
            Ok(p) => { if seen_ids.insert(p.id.clone()) { out.push(p); } }
            Err(e) => eprintln!("嵌入画像解析失败 {:?}: {e}", f.path()),
        }
    }

    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

/// 用户可放自己的画像到这里, 优先级最高(覆盖嵌入版)
fn user_profiles_dir() -> PathBuf {
    let local = std::env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    local.join("mingchuang").join("profiles")
}
