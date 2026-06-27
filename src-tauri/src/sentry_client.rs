//! GUI 进程内的 sentry 客户端 - 读 sentry 写出的共享文件 + 操作 control.json
//!
//! 不与 sentry 进程做实时 IPC, 完全通过文件交换。

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

fn sentry_dir() -> PathBuf {
    let local = std::env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    local.join("mingchuang").join("sentry")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SentryState {
    pub started_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_alert_at: Option<DateTime<Utc>>,
    pub paused_until: Option<DateTime<Utc>>,
    pub alerts_total: u64,
    pub monitored_pids: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ControlFile {
    #[serde(default)]
    pub paused_until: Option<DateTime<Utc>>,
    #[serde(default)]
    pub stop_requested: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SentryStatus {
    pub installed: bool,
    pub autostart_enabled: bool,
    pub running: bool,
    pub state: Option<SentryState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertEvent {
    pub ts: String,
    pub pid: u32,
    pub image_name: String,
    pub up_bps: u64,
}

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

// ============ 状态 ============

const RUNNING_THRESHOLD_SECS: i64 = 10; // state 在 10s 内更新过 = 运行中

pub fn get_status() -> SentryStatus {
    let exe = sentry_exe_path();
    let installed = exe.is_some();
    let autostart_enabled = is_autostart_registered();
    let state = read_state();
    let running = state.as_ref().map(|s| {
        let secs = (Utc::now() - s.updated_at).num_seconds();
        secs >= 0 && secs <= RUNNING_THRESHOLD_SECS
    }).unwrap_or(false);
    SentryStatus { installed, autostart_enabled, running, state }
}

fn read_state() -> Option<SentryState> {
    let p = sentry_dir().join("state.json");
    std::fs::read_to_string(&p).ok().and_then(|t| serde_json::from_str(&t).ok())
}

fn write_control(c: &ControlFile) -> Result<()> {
    let dir = sentry_dir();
    std::fs::create_dir_all(&dir).with_context(|| format!("创建 {dir:?} 失败"))?;
    let path = dir.join("control.json");
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(c)?)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

fn read_control() -> ControlFile {
    let p = sentry_dir().join("control.json");
    std::fs::read_to_string(&p).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

// ============ 事件 ============

pub fn list_recent_events(limit: usize) -> Vec<AlertEvent> {
    let mut out = Vec::new();
    let dir = sentry_dir();
    if !dir.is_dir() { return out; }
    let Ok(entries) = std::fs::read_dir(&dir) else { return out; };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name().and_then(|n| n.to_str())
                .map(|n| n.starts_with("events-") && n.ends_with(".jsonl"))
                .unwrap_or(false)
        })
        .collect();
    files.sort();
    files.reverse();
    for f in files {
        let Ok(txt) = std::fs::read_to_string(&f) else { continue; };
        for line in txt.lines().rev() {
            if line.trim().is_empty() { continue; }
            if let Ok(ev) = serde_json::from_str::<AlertEvent>(line) {
                out.push(ev);
                if out.len() >= limit { return out; }
            }
        }
    }
    out
}

// ============ 白名单 ============

pub fn read_user_whitelist() -> WhitelistFile {
    let p = sentry_dir().join("user.json");
    std::fs::read_to_string(&p).ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(WhitelistFile { version: 1, entries: Vec::new() })
}

pub fn write_user_whitelist(f: &WhitelistFile) -> Result<()> {
    let dir = sentry_dir();
    std::fs::create_dir_all(&dir).with_context(|| format!("创建 {dir:?} 失败"))?;
    let path = dir.join("user.json");
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(f)?)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

pub fn whitelist_add(image_name: String, reason: Option<String>) -> Result<WhitelistFile> {
    let mut wf = read_user_whitelist();
    let name_lc = image_name.to_ascii_lowercase();
    wf.entries.retain(|e| e.image_name.to_ascii_lowercase() != name_lc);
    wf.entries.push(WhitelistEntry { image_name, signer_cn: None, reason });
    write_user_whitelist(&wf)?;
    Ok(wf)
}

pub fn whitelist_remove(image_name: String) -> Result<WhitelistFile> {
    let mut wf = read_user_whitelist();
    let name_lc = image_name.to_ascii_lowercase();
    wf.entries.retain(|e| e.image_name.to_ascii_lowercase() != name_lc);
    write_user_whitelist(&wf)?;
    Ok(wf)
}

// ============ 控制 ============

pub fn pause_for(minutes: i64) -> Result<()> {
    let mut c = read_control();
    c.paused_until = Some(Utc::now() + chrono::Duration::minutes(minutes));
    write_control(&c)
}

pub fn resume() -> Result<()> {
    let mut c = read_control();
    c.paused_until = None;
    write_control(&c)
}

pub fn request_stop() -> Result<()> {
    let mut c = read_control();
    c.stop_requested = true;
    write_control(&c)
}

// ============ 启动/自启 ============

fn sentry_exe_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let parent = exe.parent()?;
    let candidates = [
        parent.join("mingchuang-sentry.exe"),
        parent.join("mingchuang-sentry"),
    ];
    candidates.into_iter().find(|p| p.is_file())
}

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "MingchuangSentry";

fn is_autostart_registered() -> bool {
    use windows_registry::CURRENT_USER;
    CURRENT_USER.open(RUN_KEY)
        .and_then(|k| k.get_string(VALUE_NAME))
        .is_ok()
}

pub fn enable_autostart() -> Result<()> {
    use windows_registry::CURRENT_USER;
    let exe = sentry_exe_path()
        .ok_or_else(|| anyhow::anyhow!("找不到 mingchuang-sentry.exe (跟 GUI 不在同目录?)"))?;
    let cmd = format!("\"{}\" --autostart", exe.display());
    let key = CURRENT_USER.create(RUN_KEY).map_err(|e| anyhow::anyhow!("{e}"))?;
    key.set_string(VALUE_NAME, &cmd).map_err(|e| anyhow::anyhow!("{e}"))?;
    // 已注册时, 顺手现在就启动它
    let _ = start_now();
    Ok(())
}

pub fn disable_autostart() -> Result<()> {
    use windows_registry::CURRENT_USER;
    let key = CURRENT_USER.create(RUN_KEY).map_err(|e| anyhow::anyhow!("{e}"))?;
    let _ = key.remove_value(VALUE_NAME);
    // 顺手请求停止运行中的 sentry
    let _ = request_stop();
    Ok(())
}

pub fn start_now() -> Result<()> {
    let exe = sentry_exe_path()
        .ok_or_else(|| anyhow::anyhow!("找不到 mingchuang-sentry.exe"))?;
    // 启动时清掉 stop_requested 标志, 否则 sentry 一启动就退
    let mut c = read_control();
    c.stop_requested = false;
    let _ = write_control(&c);
    crate::sys_cmd::cmd(exe.to_str().unwrap_or_default())
        .spawn()
        .map_err(|e| anyhow::anyhow!("启动失败: {e}"))?;
    Ok(())
}
