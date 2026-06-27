//! mingchuang-sentry - 明窗后台监控守护进程

#![cfg_attr(windows, windows_subsystem = "windows")]

mod autostart;
mod events;
mod network;
mod notify;
mod process_filter;
mod state_io;
mod sys_cmd_local;
mod toast_winrt;
mod tray;
mod visibility;
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
    sample_history: Vec<(Instant, u64)>,
    over_threshold_since: Option<Instant>,
    last_alert_at: Option<Instant>,
}

fn main() {
    eprintln!("[sentry] 启动中...");

    let started_at = chrono::Utc::now();
    let mut alerts_total: u64 = 0;
    let mut last_alert_at: Option<chrono::DateTime<chrono::Utc>> = None;

    let args: Vec<String> = std::env::args().collect();

    // 优先处理 protocol activation (toast 按钮点击)
    for arg in &args[1..] {
        if arg.starts_with("mingchuang://") {
            handle_protocol_activation(arg);
            return;
        }
    }

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
                std::thread::sleep(Duration::from_secs(30));
            }
            _ => {}
        }
    }

    // 一次性注册 AUMID + 开始菜单 .lnk + protocol scheme (idempotent)
    if let Err(e) = toast_winrt::register_app_metadata() {
        eprintln!("[sentry] 注册 toast 元数据失败 (toast 按钮可能不工作): {e:#}");
    }

    // 启动托盘图标 (独立线程跑 message pump)
    std::thread::spawn(|| {
        if let Err(e) = tray::run() {
            eprintln!("[sentry] 托盘失败: {e:#}");
        }
    });

    // 主循环
    let mut states: HashMap<u32, ProcessUploadState> = HashMap::new();
    eprintln!(
        "[sentry] 进入监控循环, 采样 {}s, 阈值 {} bps",
        SAMPLE_INTERVAL.as_secs(),
        ALERT_THRESHOLD_BPS
    );

    loop {
        let control = state_io::read_control();
        if control.stop_requested {
            eprintln!("[sentry] 收到停止指令, 退出");
            return;
        }

        // 处理 pending actions (toast 按钮)
        if !control.pending_actions.is_empty() {
            for action in &control.pending_actions {
                handle_pending_action(action);
            }
            // 清空 pending_actions
            let mut c = control.clone();
            c.pending_actions.clear();
            let _ = state_io::write_control(&c);
        }

        let now_utc = chrono::Utc::now();
        let paused = control.paused_until.map(|t| t > now_utc).unwrap_or(false);

        let _ = state_io::write_state(&state_io::SentryState {
            started_at,
            updated_at: now_utc,
            last_alert_at,
            paused_until: control.paused_until,
            alerts_total,
            monitored_pids: states.len(),
        });

        if paused {
            std::thread::sleep(SAMPLE_INTERVAL);
            continue;
        }

        let now = Instant::now();
        match network::sample_per_pid_bytes_out() {
            Ok(snapshot) => {
                for (pid, total_bytes) in snapshot {
                    let state = states.entry(pid).or_insert_with(|| ProcessUploadState {
                        pid,
                        image_name: process_filter::image_name_for(pid).unwrap_or_default(),
                        sample_history: Vec::new(),
                        over_threshold_since: None,
                        last_alert_at: None,
                    });
                    state.sample_history.push((now, total_bytes));
                    state
                        .sample_history
                        .retain(|(t, _)| now.duration_since(*t) <= Duration::from_secs(30));
                }

                let wl = whitelist::load_merged();
                let visible = visibility::visible_pids();
                let foreground = visibility::foreground_pid();

                for state in states.values_mut() {
                    let rate = compute_avg_bps(&state.sample_history);
                    if rate >= ALERT_THRESHOLD_BPS {
                        if state.over_threshold_since.is_none() {
                            state.over_threshold_since = Some(now);
                        }
                        if let Some(start) = state.over_threshold_since {
                            if now.duration_since(start) >= ALERT_WINDOW {
                                let cooled = state
                                    .last_alert_at
                                    .map(|t| now.duration_since(t) >= ALERT_COOLDOWN)
                                    .unwrap_or(true);

                                let filtered = process_filter::is_filtered(
                                    state.pid,
                                    &state.image_name,
                                    &wl,
                                );
                                let is_visible = visible.contains(&state.pid)
                                    || foreground == Some(state.pid);

                                if cooled && !filtered {
                                    if is_visible {
                                        // 可见进程只写日志, 不弹 toast
                                        let _ = events::append_alert(
                                            state.pid,
                                            &state.image_name,
                                            rate,
                                        );
                                        state.last_alert_at = Some(now);
                                    } else {
                                        trigger_alert(state, rate);
                                        state.last_alert_at = Some(now);
                                        alerts_total += 1;
                                        last_alert_at = Some(chrono::Utc::now());
                                    }
                                }
                            }
                        }
                    } else {
                        state.over_threshold_since = None;
                    }
                }

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
    let now = history.last().unwrap().0;
    let window_start = now - ALERT_WINDOW.min(Duration::from_secs(30));
    let recent: Vec<_> = history.iter().filter(|(t, _)| *t >= window_start).collect();
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
    let display = if state.image_name.is_empty() {
        format!("PID {}", state.pid)
    } else {
        state.image_name.clone()
    };
    let body = format!(
        "{display} 正在用 {mbps:.1} Mbps 上传数据。可能是正常的云同步, 也可能是它在帮别人当中转站 (PCDN)。"
    );
    eprintln!("[sentry] {title}: {body}");
    // 先试 actionable toast(带按钮), 失败降级 notify-rust
    if let Err(e) = toast_winrt::show_actionable_toast(title, &body, state.pid, &state.image_name) {
        eprintln!("[sentry] winrt toast 失败: {e:#}, 降级 notify-rust");
        let _ = notify::show_toast(title, &body);
    }
    if let Err(e) = events::append_alert(state.pid, &state.image_name, rate_bps) {
        eprintln!("[sentry] 写日志失败: {e:#}");
    }
}

// ============ Protocol activation (toast 按钮) ============

fn handle_protocol_activation(url: &str) {
    // mingchuang://action=kill&pid=1234&name=xxx.exe
    let s = url.trim_start_matches("mingchuang://");
    let mut action = String::new();
    let mut pid: Option<u32> = None;
    let mut image_name: Option<String> = None;
    for pair in s.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            let v_dec = urldecode(v);
            match k {
                "action" => action = v_dec,
                "pid" => pid = v_dec.parse().ok(),
                "name" => image_name = Some(v_dec),
                _ => {}
            }
        }
    }
    if action.is_empty() {
        return;
    }
    let pa = state_io::PendingAction { action, pid, image_name };
    let _ = state_io::enqueue_pending_action(pa);
}

fn handle_pending_action(action: &state_io::PendingAction) {
    eprintln!("[sentry] 处理用户 action: {:?}", action);
    match action.action.as_str() {
        "kill" => {
            if let Some(pid) = action.pid {
                use windows::Win32::Foundation::CloseHandle;
                use windows::Win32::System::Threading::{
                    OpenProcess, TerminateProcess, PROCESS_TERMINATE,
                };
                unsafe {
                    if let Ok(h) = OpenProcess(PROCESS_TERMINATE, false, pid) {
                        let _ = TerminateProcess(h, 1);
                        let _ = CloseHandle(h);
                    }
                }
            }
        }
        "whitelist" => {
            if let Some(name) = &action.image_name {
                let mut wf = whitelist::load_user();
                let lc = name.to_ascii_lowercase();
                wf.entries.retain(|e| e.image_name.to_ascii_lowercase() != lc);
                wf.entries.push(whitelist::WhitelistEntry {
                    image_name: name.clone(),
                    signer_cn: None,
                    reason: Some("从 toast 加白".into()),
                });
                let _ = whitelist::save_user(&wf);
            }
        }
        "ignore" | _ => {
            // ignore: 什么都不做, 默认冷却 15 分钟里不会再弹
        }
    }
}

fn urldecode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'%' && i + 2 < bytes.len() {
            let h = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("00");
            let v = u8::from_str_radix(h, 16).unwrap_or(0);
            out.push(v);
            i += 3;
        } else if c == b'+' {
            out.push(b' ');
            i += 1;
        } else {
            out.push(c);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}
