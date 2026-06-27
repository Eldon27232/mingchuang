//! 治理场景 batch API — 后端封装"一键清此电脑 / 一键停保活"等场景
//!
//! 设计原则: 前端只看"场景按钮"和"统计数字",**绝不暴露具体进程/服务/CLSID 名字**。
//! 后端按内置规则 + 画像档案 自动判定流氓项, 批量执行, 返回汇总结果。

use crate::action;
use crate::inventory::{namespace, shortcuts};
use crate::profile::{load_profiles, Action, Profile};
use anyhow::Result;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ScenarioStats {
    /// 等待处理的项数(扫描结果)
    pub pending: usize,
    /// 摘要(给 UI 显示)
    pub summary: String,
    /// 命中的"用户认识的厂商/软件名"列表(仅小白看得懂的, 不含技术细节)
    /// 例: ["123云盘", "百度网盘", "酷狗音乐"]
    #[serde(default)]
    pub display_names: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScenarioRunResult {
    pub attempted: usize,
    pub succeeded: usize,
    pub failed: usize,
    /// 生成的快照 id 列表(可批量还原)
    pub snapshot_ids: Vec<String>,
    /// 给 UI 显示的人话摘要
    pub summary: String,
}

// =============================================================================
// 场景 1: 一键清『此电脑』
// =============================================================================

pub fn scan_pc_namespace() -> Result<ScenarioStats> {
    let items = namespace::scan_pc_namespace_items()?;
    let rogue: Vec<_> = items.into_iter().filter(|i| !i.is_system).collect();
    let display_names: Vec<String> = rogue
        .iter()
        .map(|i| {
            if i.display_name.is_empty() {
                "未知项".into()
            } else {
                i.display_name.clone()
            }
        })
        .collect();
    let summary = if rogue.is_empty() {
        "你的「我的电脑」干干净净,没人偷塞图标。".into()
    } else {
        format!("「我的电脑」里被塞了 {} 个图标,可以一键清掉。", rogue.len())
    };
    Ok(ScenarioStats {
        pending: rogue.len(),
        summary,
        display_names,
    })
}

pub fn clean_pc_namespace() -> Result<ScenarioRunResult> {
    let items = namespace::scan_pc_namespace_items()?;
    let rogue: Vec<_> = items.into_iter().filter(|i| !i.is_system).collect();

    let mut snapshot_ids = Vec::new();
    let mut attempted = 0;
    let mut succeeded = 0;
    let mut failed = 0;

    for item in &rogue {
        attempted += 1;
        // 一个临时画像, 双 reg-delete
        let actions = vec![
            Action {
                kind: "reg-delete".into(),
                target: format!(
                    "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\MyComputer\\NameSpace\\{}",
                    item.clsid
                ),
                reason: format!("清『此电脑』第三方项 {}", item.display_name),
                elevate: false,
                rollback: None,
            },
            Action {
                kind: "reg-delete".into(),
                target: format!("HKCU\\Software\\Classes\\CLSID\\{}", item.clsid),
                reason: format!("清 Shell CLSID 本体 {}", item.display_name),
                elevate: false,
                rollback: None,
            },
        ];
        let p = adhoc_profile(actions);
        for idx in 0..p.actions.len() {
            let r = action::execute_action(&p, idx);
            if r.success {
                succeeded += 1;
                if let Some(sid) = r.snapshot_id {
                    snapshot_ids.push(sid);
                }
            } else {
                failed += 1;
            }
        }
    }

    let summary = format!(
        "清『此电脑』完成: 共 {} 项第三方伪文件夹, 成功 {}, 失败 {}, 已生成 {} 个还原快照。",
        rogue.len(),
        succeeded,
        failed,
        snapshot_ids.len()
    );

    Ok(ScenarioRunResult {
        attempted,
        succeeded,
        failed,
        snapshot_ids,
        summary,
    })
}

// =============================================================================
// 场景 3: 一键清流氓快捷方式
// =============================================================================

pub fn scan_rogue_shortcuts() -> Result<ScenarioStats> {
    let items = shortcuts::scan_rogue()?;
    let display_names: Vec<String> = items
        .iter()
        .filter_map(|i| i.matched_profile_name.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    let summary = if items.is_empty() {
        "桌面和开始菜单很整洁,没有国产软件塞的多余快捷方式。".into()
    } else {
        format!("桌面和开始菜单有 {} 个国产软件的快捷方式可清。", items.len())
    };
    Ok(ScenarioStats {
        pending: items.len(),
        summary,
        display_names,
    })
}

pub fn clean_rogue_shortcuts() -> Result<ScenarioRunResult> {
    let items = shortcuts::scan_rogue()?;
    let mut snapshot_ids = Vec::new();
    let mut attempted = 0;
    let mut succeeded = 0;
    let mut failed = 0;

    for it in &items {
        attempted += 1;
        let actions = vec![Action {
            kind: "file-delete".into(),
            target: it.path.clone(),
            reason: format!("删除流氓快捷方式 {}", it.name),
            elevate: false,
            rollback: None,
        }];
        let p = adhoc_profile(actions);
        let r = action::execute_action(&p, 0);
        if r.success {
            succeeded += 1;
            if let Some(sid) = r.snapshot_id {
                snapshot_ids.push(sid);
            }
        } else {
            failed += 1;
        }
    }

    let summary = format!(
        "清快捷方式完成: 共 {} 个流氓项, 成功 {}, 失败 {}, 已生成 {} 个还原快照。",
        items.len(),
        succeeded,
        failed,
        snapshot_ids.len()
    );

    Ok(ScenarioRunResult {
        attempted,
        succeeded,
        failed,
        snapshot_ids,
        summary,
    })
}

// =============================================================================
// 场景 2: 一键停保活服务 + 禁自启
// =============================================================================

pub fn scan_keepalive_services() -> Result<ScenarioStats> {
    let (targets, display_names) = collect_keepalive_with_display()?;
    let summary = if targets.is_empty() {
        "没有国产软件在你电脑后台偷跑。".into()
    } else {
        format!("有 {} 个国产软件在后台偷跑,可一键关掉。", targets.len())
    };
    Ok(ScenarioStats {
        pending: targets.len(),
        summary,
        display_names,
    })
}

pub fn stop_keepalive_services() -> Result<ScenarioRunResult> {
    let targets = collect_keepalive_service_names()?;

    let mut snapshot_ids = Vec::new();
    let mut attempted = 0;
    let mut succeeded = 0;
    let mut failed = 0;

    for svc in &targets {
        let actions = vec![
            Action {
                kind: "service-stop".into(),
                target: svc.clone(),
                reason: format!("停止保活服务 {svc}"),
                elevate: true,
                rollback: None,
            },
            Action {
                kind: "service-disable".into(),
                target: svc.clone(),
                reason: format!("禁止 {svc} 开机自启"),
                elevate: true,
                rollback: None,
            },
        ];
        let p = adhoc_profile(actions);
        for idx in 0..p.actions.len() {
            attempted += 1;
            let r = action::execute_action(&p, idx);
            if r.success {
                succeeded += 1;
                if let Some(sid) = r.snapshot_id {
                    snapshot_ids.push(sid);
                }
            } else {
                failed += 1;
            }
        }
    }

    let summary = format!(
        "停保活完成: 处理 {} 个服务, 成功 {}, 失败 {}, 已生成 {} 个还原快照。",
        targets.len(),
        succeeded,
        failed,
        snapshot_ids.len()
    );

    Ok(ScenarioRunResult {
        attempted,
        succeeded,
        failed,
        snapshot_ids,
        summary,
    })
}

/// 收集本机当前在跑、命中画像 service_names 的服务名列表。
/// 后端内部使用,**不外泄前端**。
fn collect_keepalive_service_names() -> Result<Vec<String>> {
    Ok(collect_keepalive_with_display()?.0)
}

/// 同上, 但同时返回该服务对应画像的人话 display_name
fn collect_keepalive_with_display() -> Result<(Vec<String>, Vec<String>)> {
    let profiles = load_profiles().unwrap_or_default();
    // service_name(lc) -> profile_display_name
    let mut wanted: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for p in &profiles {
        for s in &p.fingerprints.service_names {
            wanted.insert(s.to_lowercase(), p.name.clone());
        }
    }

    let out = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "Get-CimInstance Win32_Service | Where-Object { $_.State -eq 'Running' } | Select-Object -ExpandProperty Name",
        ])
        .output()?;
    let running: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let mut hits = Vec::new();
    let mut display_set: std::collections::HashSet<String> = std::collections::HashSet::new();
    for name in running {
        if let Some(display) = wanted.get(&name.to_lowercase()) {
            hits.push(name);
            display_set.insert(display.clone());
        }
    }
    Ok((hits, display_set.into_iter().collect()))
}

fn adhoc_profile(actions: Vec<Action>) -> Profile {
    Profile {
        id: "govern-adhoc".into(),
        name: "治理 临时动作".into(),
        vendor: "kuake-fuckyou".into(),
        category: "other".into(),
        severity: "high".into(),
        tested_on: None,
        fingerprints: Default::default(),
        actions,
        verify: vec![],
        notes: None,
    }
}
