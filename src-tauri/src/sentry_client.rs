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
    #[serde(default)]
    pub last_inspection_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub last_tamper_check_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub last_inspection_findings: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ControlFile {
    #[serde(default)]
    pub paused_until: Option<DateTime<Utc>>,
    #[serde(default)]
    pub stop_requested: bool,
    #[serde(default)]
    pub inspection_enabled: bool,
    #[serde(default)]
    pub tamper_alert_enabled: bool,
    #[serde(default = "default_inspection_interval")]
    pub inspection_interval_minutes: u32,
    #[serde(default)]
    pub run_inspection_now: bool,
}

fn default_inspection_interval() -> u32 { 60 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectionConfig {
    pub inspection_enabled: bool,
    pub tamper_alert_enabled: bool,
    pub inspection_interval_minutes: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InspectionEvent {
    pub ts: String,
    pub kind: String,      // "added" / "userchoice_lost"
    pub category: String,  // "pc_namespace" / "autostart" / "userchoice"
    pub label: String,
    pub detail: String,
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
    // 顺手自愈 Run 键路径漂移 (升级/卸载后老路径失效的情况)
    let _ = repair_autostart_path();

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

// ============ 巡检/偷改告警 ============

pub fn get_inspection_config() -> InspectionConfig {
    let c = read_control();
    InspectionConfig {
        inspection_enabled: c.inspection_enabled,
        tamper_alert_enabled: c.tamper_alert_enabled,
        inspection_interval_minutes: c.inspection_interval_minutes,
    }
}

pub fn set_inspection_config(cfg: InspectionConfig) -> Result<()> {
    let mut c = read_control();
    c.inspection_enabled = cfg.inspection_enabled;
    c.tamper_alert_enabled = cfg.tamper_alert_enabled;
    c.inspection_interval_minutes = cfg.inspection_interval_minutes.max(5).min(1440 * 7);
    write_control(&c)
}

pub fn request_inspection_now() -> Result<()> {
    let mut c = read_control();
    c.run_inspection_now = true;
    write_control(&c)
}

pub fn reset_inspection_baseline() -> Result<()> {
    crate::inspection::reset_baseline()
}

pub fn list_recent_inspection_events(limit: usize) -> Vec<InspectionEvent> {
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
            // jsonl 里有两种 schema, 用字段在不在区分
            if let Ok(ev) = serde_json::from_str::<InspectionEvent>(line) {
                // 防止把 AlertEvent 误解为 InspectionEvent (PCDN alert 没 kind/category 字段)
                if !ev.kind.is_empty() && !ev.category.is_empty() {
                    out.push(ev);
                    if out.len() >= limit { return out; }
                }
            }
        }
    }
    out
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

/// 自愈: Run 键里存的 sentry 路径若指向不存在的 exe (升级/卸载后挪位置了),
/// 用当前 sentry exe 路径覆盖。
/// 用户反馈: 开机自启列表里有明窗但实际没启动 — 多半就是这个。
/// 返回是否做了修复 (写新值)。
fn repair_autostart_path() -> bool {
    use windows_registry::CURRENT_USER;
    let Ok(key) = CURRENT_USER.open(RUN_KEY) else { return false; };
    let Ok(existing) = key.get_string(VALUE_NAME) else { return false; };
    let Some(current_exe) = sentry_exe_path() else { return false; };
    let expected = format!("\"{}\" --autostart", current_exe.display());
    if existing == expected {
        return false;
    }
    // 检查 existing 里嵌的 exe 路径是不是还存在
    let still_valid = parse_quoted_exe(&existing)
        .map(|p| std::path::Path::new(&p).is_file())
        .unwrap_or(false);
    if still_valid {
        // 老路径还有效 (比如用户有多个安装), 不动 — 用户自己点 enable_autostart 才重写
        return false;
    }
    // 老路径失效 → 静默重写到当前 sentry exe
    let Ok(key_w) = CURRENT_USER.create(RUN_KEY) else { return false; };
    let _ = key_w.set_string(VALUE_NAME, &expected);
    eprintln!("[autostart-repair] Run 键路径已修正: {existing} → {expected}");
    true
}

/// 从 `"C:\path\app.exe" --autostart` 这种 Run 键值里抠出引号内的 exe 路径
fn parse_quoted_exe(s: &str) -> Option<String> {
    let s = s.trim();
    if !s.starts_with('"') { return None; }
    let end = s[1..].find('"')?;
    Some(s[1..1 + end].to_string())
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
