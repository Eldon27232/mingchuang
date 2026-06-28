//! 白名单 - 内置 + 用户
//!
//! **关键设计变更 (2026-06-27 修正)**: 之前内置了 60+ 项浏览器/网盘/通讯/同步盘
//! 都白名单, 但用户指出: 浏览器和网盘正是 PCDN 重灾区, 它们后台静默偷传才该报警,
//! 把它们无条件加白等于反逻辑。
//!
//! 新策略:
//!  - 内置白名单**只保留真系统进程 + 明窗自己** (没法误报且不可能 PCDN)
//!  - 浏览器/网盘/IDE/游戏平台/QQ 等**全部移出**, 走前台/可见性判定
//!  - 用户在用 (前台或可见窗口) = 不报警, 没窗口偷传 = 报警
//!
//! 用户可在 GUI 白名单页里手动加自己信任的进程。

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhitelistFile {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub entries: Vec<WhitelistEntry>,
}

fn default_version() -> u32 { 1 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhitelistEntry {
    pub image_name: String,
    #[serde(default)]
    pub signer_cn: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

pub struct MergedWhitelist {
    pub image_names: HashSet<String>,
}

pub fn sentry_dir() -> PathBuf {
    let local = std::env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    local.join("mingchuang").join("sentry")
}

/// 内置白名单 — **只留真系统进程 + 明窗自己**, 浏览器/网盘/通讯全砍
fn builtin_image_names() -> Vec<&'static str> {
    vec![
        // Windows Defender 实时扫描 (流量大但合法)
        "MsMpEng.exe", "NisSrv.exe",
        // Windows Update 系列 (合法且必要)
        "MoUsoCoreWorker.exe", "TiWorker.exe", "TrustedInstaller.exe",
        "wuauclt.exe", "WindowsUpdateBox.exe",
        // Delivery Optimization (Microsoft P2P 更新, 系统级)
        "DeliveryOptimization.exe", "BackgroundTransferHost.exe",
        // 系统搜索本地索引 (不联网, 但万一)
        "SearchHost.exe", "SearchIndexer.exe", "SearchApp.exe",
        // SmartScreen / 安全
        "SecurityHealthService.exe", "SecurityHealthSystray.exe", "smartscreen.exe",
        // Microsoft Store / 后台传输
        "MicrosoftEdgeUpdate.exe",
        // 明窗自己
        "mingchuang.exe", "mingchuang-sentry.exe", "明窗.exe",
        // svchost (太大,但杀了系统就挂)
        "svchost.exe", "csrss.exe", "lsass.exe", "wininit.exe", "services.exe",
        "smss.exe", "winlogon.exe", "dwm.exe", "fontdrvhost.exe",
    ]
}

pub fn load_merged() -> MergedWhitelist {
    let mut names: HashSet<String> = builtin_image_names()
        .iter()
        .map(|s| s.to_ascii_lowercase())
        .collect();

    let user_path = sentry_dir().join("user.json");
    if let Ok(txt) = std::fs::read_to_string(&user_path) {
        if let Ok(f) = serde_json::from_str::<WhitelistFile>(&txt) {
            for e in f.entries {
                names.insert(e.image_name.to_ascii_lowercase());
            }
        }
    }

    MergedWhitelist { image_names: names }
}

pub fn save_user(file: &WhitelistFile) -> Result<()> {
    let dir = sentry_dir();
    std::fs::create_dir_all(&dir).with_context(|| format!("创建 {dir:?} 失败"))?;
    let path = dir.join("user.json");
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(file)?;
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

pub fn load_user() -> WhitelistFile {
    let path = sentry_dir().join("user.json");
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(WhitelistFile {
            version: 1,
            entries: Vec::new(),
        })
}
