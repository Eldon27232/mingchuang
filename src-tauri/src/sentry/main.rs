//! mingchuang-sentry - 明窗后台监控守护进程
//!
//! 极轻量, 开机自启, 监控所有非系统/非白名单进程的上行带宽,
//! 持续 15 秒 > 8 Mbps 弹 Windows toast 警告。
//!
//! Phase 1 (本 commit):
//!  - HKCU Run 注册自启动
//!  - 2s 轮询 GetExtendedTcpTable + PerTcpConnectionEStats
//!  - 系统/路径白名单过滤
//!  - 简单 toast (无 actionable callback)
//!  - 持久化 JSON 白名单 + JSONL 事件日志
//!
//! Phase 2 (下一 commit):
//!  - 签名校验 + 前台/可见性判定
//!  - Toast actionable callback (COM INotificationActivationCallback + AUMID)
//!  - 命名管道 IPC + GUI 守护面板
//!  - 托盘图标

#![cfg_attr(windows, windows_subsystem = "windows")]

mod autostart;
mod events;
mod network;
mod notify;
mod process_filter;
mod whitelist;

use std::collections::HashMap;
use std::time::{Duration, Instant};

const SAMPLE_INTERVAL: Duration = Duration::from_secs(2);
const ALERT_THRESHOLD_BPS: u64 = 8 * 1024 * 1024; // 8 Mbps
const ALERT_WINDOW: Duration = Duration::from_secs(15);
const ALERT_COOLDOWN: Duration = Duration::from_secs(15 * 60); // 同进程 15 分钟最多一条

#[derive(Debug, Clone)]
struct ProcessUploadState {
    pid: u32,
    image_name: String,
    sample_history: Vec<(Instant, u64)>, // (time, bytes_sent total)
    over_threshold_since: Option<Instant>,
    last_alert_at: Option<Instant>,
}

fn main() {
    eprintln!("[sentry] 启动中...");

    // 命令行参数: --register-autostart / --unregister-autostart
    let args: Vec<String> = std::env::args().collect();
    for arg in &args[1..] {
        match arg.as_str() {
            "--register-autostart" => {
                match autostart::register() {
                    Ok(_) => println!("已注册开机自启"),
                    Err(e) => eprintln!("注册自启失败: {e:#}"),
                }
                return;
            }
            "--unregister-autostart" => {
                match autostart::unregister() {
                    Ok(_) => println!("已取消开机自启"),
                    Err(e) => eprintln!("取消自启失败: {e:#}"),
                }
                return;
            }
            "--autostart" => {
                // 开机自启拉起时带的标记, 等几秒避免争抢系统启动 CPU
                std::thread::sleep(Duration::from_secs(30));
            }
            _ => {}
        }
    }

    // 主循环
    let mut states: HashMap<u32, ProcessUploadState> = HashMap::new();
    eprintln!("[sentry] 进入监控循环, 采样间隔 {}s, 阈值 {} bps", SAMPLE_INTERVAL.as_secs(), ALERT_THRESHOLD_BPS);

    loop {
        let now = Instant::now();
        match network::sample_per_pid_bytes_out() {
            Ok(snapshot) => {
                // 1. 更新每个 PID 的累计样本
                for (pid, total_bytes) in snapshot {
                    let state = states.entry(pid).or_insert_with(|| ProcessUploadState {
                        pid,
                        image_name: process_filter::image_name_for(pid).unwrap_or_default(),
                        sample_history: Vec::new(),
                        over_threshold_since: None,
                        last_alert_at: None,
                    });
                    state.sample_history.push((now, total_bytes));
                    // 只保留最近 30 秒的样本
                    state
                        .sample_history
                        .retain(|(t, _)| now.duration_since(*t) <= Duration::from_secs(30));
                }

                // 2. 检测阈值 + 触发告警
                let wl = whitelist::load_merged();
                for state in states.values_mut() {
                    let rate = compute_avg_bps(&state.sample_history);
                    if rate >= ALERT_THRESHOLD_BPS {
                        if state.over_threshold_since.is_none() {
                            state.over_threshold_since = Some(now);
                        }
                        if let Some(start) = state.over_threshold_since {
                            if now.duration_since(start) >= ALERT_WINDOW {
                                // 冷却期内不重复告警
                                let cooled = state
                                    .last_alert_at
                                    .map(|t| now.duration_since(t) >= ALERT_COOLDOWN)
                                    .unwrap_or(true);
                                if cooled
                                    && !process_filter::is_filtered(
                                        state.pid,
                                        &state.image_name,
                                        &wl,
                                    )
                                {
                                    trigger_alert(state, rate);
                                    state.last_alert_at = Some(now);
                                }
                            }
                        }
                    } else {
                        state.over_threshold_since = None;
                    }
                }

                // 3. 清理已退出进程
                states.retain(|pid, _| process_filter::pid_alive(*pid));
            }
            Err(e) => {
                eprintln!("[sentry] 采样失败: {e:#}");
            }
        }
        std::thread::sleep(SAMPLE_INTERVAL);
    }
}

fn compute_avg_bps(history: &[(Instant, u64)]) -> u64 {
    if history.len() < 2 {
        return 0;
    }
    // 取最近 ALERT_WINDOW 内的样本计算平均
    let now = history.last().unwrap().0;
    let window_start = now - ALERT_WINDOW.min(Duration::from_secs(30));
    let recent: Vec<_> = history
        .iter()
        .filter(|(t, _)| *t >= window_start)
        .collect();
    if recent.len() < 2 {
        return 0;
    }
    let (t0, b0) = recent.first().unwrap();
    let (t1, b1) = recent.last().unwrap();
    let dt = t1.duration_since(*t0).as_secs_f64();
    if dt < 0.1 {
        return 0;
    }
    let bytes = b1.saturating_sub(*b0);
    ((bytes as f64 * 8.0) / dt) as u64
}

fn trigger_alert(state: &ProcessUploadState, rate_bps: u64) {
    let mbps = rate_bps as f64 / 1_000_000.0;
    let title = "明窗发现一个在偷偷上传的程序";
    let body = format!(
        "{} 正在用 {:.1} Mbps 上传数据。\n这可能是正常的云同步,也可能是它在帮别人当中转站 (PCDN)。\n在明窗里可以加入白名单或停止它。",
        if state.image_name.is_empty() {
            format!("PID {}", state.pid)
        } else {
            state.image_name.clone()
        },
        mbps
    );
    eprintln!("[sentry] {title}: {body}");
    if let Err(e) = notify::show_toast(title, &body) {
        eprintln!("[sentry] toast 失败: {e:#}");
    }
    if let Err(e) = events::append_alert(state.pid, &state.image_name, rate_bps) {
        eprintln!("[sentry] 写日志失败: {e:#}");
    }
}
