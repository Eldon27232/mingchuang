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

// 偷改告警轮询间隔 — 60s 是工程上的折中, 实时性够, 不至于撑 CPU
const TAMPER_CHECK_INTERVAL: Duration = Duration::from_secs(60);

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

    // 巡检/偷改告警状态
    let mut last_inspection_at: Option<chrono::DateTime<chrono::Utc>> = None;
    let mut last_tamper_check_at: Option<chrono::DateTime<chrono::Utc>> = None;
    let mut last_inspection_findings: u32 = 0;
    let mut last_tamper_at_instant: Option<Instant> = None;

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
            last_inspection_at,
            last_tamper_check_at,
            last_inspection_findings,
        });

        if paused {
            std::thread::sleep(SAMPLE_INTERVAL);
            continue;
        }

        // ============ 偷改告警 (高频快查) ============
        let tamper_due = control.tamper_alert_enabled
            && last_tamper_at_instant
                .map(|t| Instant::now().duration_since(t) >= TAMPER_CHECK_INTERVAL)
                .unwrap_or(true);
        if tamper_due {
            let findings = run_tamper_check();
            last_tamper_at_instant = Some(Instant::now());
            last_tamper_check_at = Some(chrono::Utc::now());
            for ev in &findings {
                fire_inspection_toast("🚨 偷改告警", ev);
                let _ = events::append_inspection(ev);
            }
            if !findings.is_empty() {
                alerts_total += findings.len() as u64;
                last_alert_at = Some(chrono::Utc::now());
            }
        }

        // ============ 定时巡检 (低频全面) ============
        let interval_minutes = control.inspection_interval_minutes.max(5) as i64;
        let inspection_due = control.inspection_enabled
            && (control.run_inspection_now
                || last_inspection_at
                    .map(|t| {
                        chrono::Utc::now()
                            .signed_duration_since(t)
                            .num_minutes()
                            >= interval_minutes
                    })
                    .unwrap_or(true));
        if inspection_due {
            let findings = run_full_inspection();
            last_inspection_at = Some(chrono::Utc::now());
            last_inspection_findings = findings.len() as u32;
            for ev in &findings {
                fire_inspection_toast("🔍 定时巡检发现变化", ev);
                let _ = events::append_inspection(ev);
            }
            if !findings.is_empty() {
                alerts_total += findings.len() as u64;
                last_alert_at = Some(chrono::Utc::now());
            }
            // 清掉一次性请求位
            if control.run_inspection_now {
                let mut c = control.clone();
                c.run_inspection_now = false;
                let _ = state_io::write_control(&c);
            }
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

// ============ 巡检 / 偷改告警 ============

fn run_full_inspection() -> Vec<mingchuang_lib::inspection::ChangeEvent> {
    let curr = match mingchuang_lib::inspection::scan_full() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[sentry] 定时巡检扫描失败: {e:#}");
            return Vec::new();
        }
    };
    let result = match mingchuang_lib::inspection::load_baseline() {
        None => {
            // 首次巡检: 把当前状态当作基准, 不算 diff, 不告警。
            // 否则用户开启巡检的瞬间会被自己电脑现有状态炸一脸 toast。
            let summary = format!(
                "已建立巡检基准: {} 项「此电脑」 / {} 项自启 / {} 项默认打开方式",
                curr.pc_namespace.len(),
                curr.autostart.len(),
                curr.userchoice.len()
            );
            eprintln!("[sentry] {summary}");
            let baseline_event = mingchuang_lib::inspection::ChangeEvent {
                ts: chrono::Utc::now(),
                kind: mingchuang_lib::inspection::ChangeKind::BaselineEstablished,
                category: "baseline".into(),
                label: summary,
                detail: "首次跑定时巡检, 把当前电脑状态当作基准。下次比对只在新增项时告警, 而不是把已有项也当新增。".into(),
            };
            let _ = events::append_inspection(&baseline_event);
            Vec::new()
        }
        Some(b) => mingchuang_lib::inspection::diff(&b, &curr),
    };
    if let Err(e) = mingchuang_lib::inspection::save_baseline(&curr) {
        eprintln!("[sentry] 保存 inspection baseline 失败: {e:#}");
    }
    result
}

/// 偷改告警: 跟 baseline 比, 但不更新 baseline (只有定时巡检会更新)
/// — 这样用户没点"立即巡检"或"接受变化"前, 同一个偷改会持续告 (按 60s 节流)
fn run_tamper_check() -> Vec<mingchuang_lib::inspection::ChangeEvent> {
    let curr = match mingchuang_lib::inspection::scan_quick() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[sentry] 偷改快查失败: {e:#}");
            return Vec::new();
        }
    };
    let baseline = match mingchuang_lib::inspection::load_baseline() {
        Some(b) => b,
        None => {
            // 没有 baseline = 还没第一次定时巡检过, 无对比基准, 静默
            return Vec::new();
        }
    };
    mingchuang_lib::inspection::diff(&baseline, &curr)
}

fn fire_inspection_toast(title: &str, ev: &mingchuang_lib::inspection::ChangeEvent) {
    eprintln!("[sentry] {title}: {}", ev.label);
    let _ = notify::show_toast(title, &ev.label);
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
