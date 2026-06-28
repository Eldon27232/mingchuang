//! 定时巡检 + 偷改告警的核心扫描逻辑
//!
//! sentry daemon 周期性调这里, 把当前状态和 baseline.json 里的"上一次干净状态"对比,
//! 发现新增/被改的项就 emit ChangeEvent (sentry 再决定 toast / 写日志)。
//!
//! 范围 (按用户口径):
//! - "此电脑" 命名空间 → 流氓软件装回来会塞自己的伪文件夹
//! - 自启动: HKCU\...\Run + HKLM\...\Run + 计划任务
//! - 默认打开方式漂移: 对比 manifest 里我设过的 ProgId
//!
//! **桌面快捷方式不扫** (用户明确不需要)
//! **保活 v1 不扫** (服务/进程枚举依赖较重, 留到下一版)

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use windows_registry::{CURRENT_USER, LOCAL_MACHINE};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct InspectionSnapshot {
    pub pc_namespace: Vec<PcNamespaceEntry>,
    pub autostart: Vec<AutostartEntry>,
    pub userchoice: Vec<UserChoiceEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct PcNamespaceEntry {
    pub clsid: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct AutostartEntry {
    /// 来源: "HKCU\\Run" / "HKLM\\Run" / "Task:\\<path>"
    pub source: String,
    pub name: String,
    pub command: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct UserChoiceEntry {
    pub ext: String,
    pub expected_progid: String,
    pub actual_progid: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChangeKind {
    /// 新增了一个之前不在 baseline 的项目, 偷改的典型迹象
    Added,
    /// UserChoice 期望和实际不一致 (默认打开方式被改回)
    UserChoiceLost,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeEvent {
    pub ts: chrono::DateTime<chrono::Utc>,
    pub kind: ChangeKind,
    pub category: String, // "pc_namespace" / "autostart" / "userchoice"
    pub label: String,    // 给用户看的描述
    pub detail: String,   // 技术细节
}

/// 全面扫描 (定时巡检用)
pub fn scan_full() -> Result<InspectionSnapshot> {
    Ok(InspectionSnapshot {
        pc_namespace: scan_pc_namespace()?,
        autostart: scan_autostart(),
        userchoice: scan_userchoice(),
    })
}

/// 轻量快查 (偷改告警用, 跳过相对慢的计划任务枚举)
pub fn scan_quick() -> Result<InspectionSnapshot> {
    Ok(InspectionSnapshot {
        pc_namespace: scan_pc_namespace().unwrap_or_default(),
        autostart: scan_autostart_registry_only(),
        userchoice: scan_userchoice(),
    })
}

fn scan_pc_namespace() -> Result<Vec<PcNamespaceEntry>> {
    let items = crate::inventory::namespace::scan_pc_namespace_items()?;
    Ok(items
        .into_iter()
        .filter(|i| !i.is_system)
        .map(|i| PcNamespaceEntry {
            clsid: i.clsid,
            display_name: i.display_name,
        })
        .collect())
}

fn scan_autostart() -> Vec<AutostartEntry> {
    let mut out = scan_autostart_registry_only();
    out.extend(scan_scheduled_tasks());
    out
}

fn scan_autostart_registry_only() -> Vec<AutostartEntry> {
    let mut out = Vec::new();
    for (root, root_label) in [(CURRENT_USER, "HKCU"), (LOCAL_MACHINE, "HKLM")] {
        for path in [
            "Software\\Microsoft\\Windows\\CurrentVersion\\Run",
            "Software\\Microsoft\\Windows\\CurrentVersion\\RunOnce",
        ] {
            let Ok(key) = root.open(path) else { continue };
            let Ok(values) = key.values() else { continue };
            for (name, val) in values {
                let cmd: String = match val.try_into() {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                out.push(AutostartEntry {
                    source: format!("{root_label}\\{}", path.rsplit('\\').next().unwrap_or(path)),
                    name,
                    command: cmd,
                });
            }
        }
    }
    out
}

/// 用 schtasks /query 列计划任务 (Win32 ITaskService COM 比较重, 子进程更简单)
fn scan_scheduled_tasks() -> Vec<AutostartEntry> {
    let out = crate::sys_cmd::cmd("schtasks")
        .args(["/query", "/fo", "csv", "/nh"])
        .output();
    let Ok(out) = out else { return Vec::new() };
    if !out.status.success() {
        return Vec::new();
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut tasks = Vec::new();
    for line in stdout.lines() {
        // CSV: "TaskName","Next Run Time","Status"
        let cols: Vec<&str> = line.split("\",\"").collect();
        if cols.len() < 3 {
            continue;
        }
        let path = cols[0].trim_start_matches('"').trim_end_matches('"');
        if path.is_empty() || path.starts_with("\\Microsoft\\") {
            continue; // 微软系统任务忽略, 不进偷改告警
        }
        let status = cols[2].trim_start_matches('"').trim_end_matches('"');
        tasks.push(AutostartEntry {
            source: "Task".into(),
            name: path.to_string(),
            command: status.to_string(),
        });
    }
    tasks
}

fn scan_userchoice() -> Vec<UserChoiceEntry> {
    let manifest = crate::fileassoc::manifest::load();
    let mut out = Vec::new();
    for app in &manifest.apps {
        // ProgId 形如 Mingchuang.<stem>, 重新算一遍 (跟 progid::register 一致)
        let exe_stem = std::path::Path::new(&app.exe_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        let cleaned: String = exe_stem
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
            .collect();
        if cleaned.is_empty() {
            continue;
        }
        let expected = format!("Mingchuang.{cleaned}");
        for ext in &app.extensions {
            let path = format!(
                "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\FileExts\\{ext}\\UserChoice"
            );
            let actual = CURRENT_USER
                .open(&path)
                .and_then(|k| k.get_string("ProgId"))
                .unwrap_or_default();
            out.push(UserChoiceEntry {
                ext: ext.clone(),
                expected_progid: expected.clone(),
                actual_progid: actual,
            });
        }
    }
    out
}

/// 把 curr 和 baseline 对比, 出"被偷改"的项。
/// - pc_namespace + autostart: 任何 curr 里出现而 baseline 里没有的都是"新增" → 告警
/// - userchoice: 任何 actual_progid != expected_progid 的都是"丢了" → 告警
pub fn diff(baseline: &InspectionSnapshot, curr: &InspectionSnapshot) -> Vec<ChangeEvent> {
    let now = chrono::Utc::now();
    let mut events = Vec::new();

    // PC namespace 新增
    let baseline_clsids: std::collections::HashSet<&String> =
        baseline.pc_namespace.iter().map(|e| &e.clsid).collect();
    for entry in &curr.pc_namespace {
        if !baseline_clsids.contains(&entry.clsid) {
            events.push(ChangeEvent {
                ts: now,
                kind: ChangeKind::Added,
                category: "pc_namespace".into(),
                label: format!(
                    "「我的电脑」被塞了一个新图标: {}",
                    if entry.display_name.is_empty() {
                        entry.clsid.as_str()
                    } else {
                        entry.display_name.as_str()
                    }
                ),
                detail: format!("CLSID {} 在 HKCU NameSpace 下新增", entry.clsid),
            });
        }
    }

    // Autostart 新增
    let baseline_keys: std::collections::HashSet<(String, String)> = baseline
        .autostart
        .iter()
        .map(|e| (e.source.clone(), e.name.clone()))
        .collect();
    for entry in &curr.autostart {
        let k = (entry.source.clone(), entry.name.clone());
        if !baseline_keys.contains(&k) {
            events.push(ChangeEvent {
                ts: now,
                kind: ChangeKind::Added,
                category: "autostart".into(),
                label: format!("有应用悄悄加了开机启动: {} ({})", entry.name, entry.source),
                detail: format!("命令: {}", entry.command),
            });
        }
    }

    // UserChoice 漂移 — 不参考 baseline, curr 里只要 actual != expected 就告
    for uc in &curr.userchoice {
        if uc.actual_progid != uc.expected_progid {
            let was_explicit_change = baseline
                .userchoice
                .iter()
                .find(|b| b.ext == uc.ext)
                .map(|b| b.actual_progid == b.expected_progid)
                .unwrap_or(false);
            // 只在 baseline 里"是我们的" 但 curr 里"不是了"才算偷改
            // (避免对从未成功设过的扩展名反复告警)
            if was_explicit_change {
                events.push(ChangeEvent {
                    ts: now,
                    kind: ChangeKind::UserChoiceLost,
                    category: "userchoice".into(),
                    label: format!(
                        "{} 的默认打开方式被改回了 ({} → {})",
                        uc.ext, uc.expected_progid, uc.actual_progid
                    ),
                    detail: format!(
                        "UserChoice 期望 {} 实际 {}",
                        uc.expected_progid, uc.actual_progid
                    ),
                });
            }
        }
    }

    events
}

// ============ baseline 持久化 ============

pub fn baseline_path() -> PathBuf {
    let base = std::env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    base.join("mingchuang")
        .join("sentry")
        .join("inspection-baseline.json")
}

pub fn load_baseline() -> Option<InspectionSnapshot> {
    let bytes = std::fs::read(baseline_path()).ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub fn save_baseline(snap: &InspectionSnapshot) -> Result<()> {
    let path = baseline_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context("创建 baseline 目录失败")?;
    }
    let bytes = serde_json::to_vec_pretty(snap)?;
    std::fs::write(&path, bytes).context("写 baseline 失败")?;
    Ok(())
}
