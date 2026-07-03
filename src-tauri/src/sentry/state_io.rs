//! 共享文件 IPC - state.json (sentry 写, GUI 读) + control.json (GUI 写, sentry 读)

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SentryState {
    pub started_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_alert_at: Option<DateTime<Utc>>,
    pub paused_until: Option<DateTime<Utc>>,
    pub alerts_total: u64,
    pub monitored_pids: usize,
    /// 上次巡检完成时间
    #[serde(default)]
    pub last_inspection_at: Option<DateTime<Utc>>,
    /// 上次偷改告警快查时间
    #[serde(default)]
    pub last_tamper_check_at: Option<DateTime<Utc>>,
    /// 最近一次巡检的发现数 (新增问题项, 0 表示一切正常)
    #[serde(default)]
    pub last_inspection_findings: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ControlFile {
    /// GUI 写入: 暂停到何时
    #[serde(default)]
    pub paused_until: Option<DateTime<Utc>>,
    /// GUI 写入: 请求停止
    #[serde(default)]
    pub stop_requested: bool,
    /// Toast 按钮点击后, sentry 帮手进程把动作写到这里, 主守护进程读后处理
    #[serde(default)]
    pub pending_actions: Vec<PendingAction>,

    // ============ 巡检/偷改告警 (默认全关) ============
    /// 定时巡检: 每 inspection_interval_minutes 跑一次全面扫描 (PC namespace + 保活 + 自启 + 默认打开方式漂移)
    #[serde(default)]
    pub inspection_enabled: bool,
    /// 偷改告警: 每 60s 跑轻量快查 (UserChoice + Run 键), 发现变化立刻告警
    #[serde(default)]
    pub tamper_alert_enabled: bool,
    /// 巡检间隔, 分钟。15/60/360/1440 四个推荐值, 默认 60
    #[serde(default = "default_inspection_interval")]
    pub inspection_interval_minutes: u32,
    /// GUI 写入: 请求立即跑一次巡检, 守护进程消化后会清回 false
    #[serde(default)]
    pub run_inspection_now: bool,

    // ============ 证书链监控 (默认全关) ============
    /// CA 证书监控: 周期扫受信任根证书库, 新增根证书告警
    #[serde(default)]
    pub ca_watch_enabled: bool,
    /// Claude 链路监控: 周期直连 Anthropic 端点抓证书链, 被中间人拦截告警
    #[serde(default)]
    pub claude_tls_watch_enabled: bool,
    /// GUI 写入: 请求立即跑一次证书检查 (CA + Claude 链路), 守护进程消化后清回 false
    #[serde(default)]
    pub run_cert_check_now: bool,
}

fn default_inspection_interval() -> u32 {
    60
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingAction {
    pub action: String, // "kill" / "whitelist" / "ignore"
    pub pid: Option<u32>,
    pub image_name: Option<String>,
}

pub fn state_path() -> PathBuf {
    crate::whitelist::sentry_dir().join("state.json")
}

pub fn control_path() -> PathBuf {
    crate::whitelist::sentry_dir().join("control.json")
}

pub fn write_state(s: &SentryState) -> Result<()> {
    let path = state_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("创建 {parent:?} 失败"))?;
    }
    let json = serde_json::to_string_pretty(s)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

pub fn read_control() -> ControlFile {
    let path = control_path();
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn write_control(c: &ControlFile) -> Result<()> {
    let path = control_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("创建 {parent:?} 失败"))?;
    }
    let json = serde_json::to_string_pretty(c)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// 把一个 pending action 追加到 control.json (sentry 帮手进程用)
pub fn enqueue_pending_action(action: PendingAction) -> Result<()> {
    let mut c = read_control();
    c.pending_actions.push(action);
    write_control(&c)
}
