//! AI 暴露给 Executor 的 tool 集合
//!
//! 4 个只读 tool + 5 个破坏性 tool。
//! 只读 tool 自动执行 (Reviewer 无条件 safe)。
//! 破坏性 tool 走 Reviewer + 用户审批。

use crate::ai::client::AnthropicTool;
use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use serde_json::{json, Value};

pub const SAFE_TOOLS: &[&str] = &[
    "query_pc_namespace",
    "query_processes",
    "query_services",
    "query_registry_value",
];

pub const DESTRUCTIVE_TOOLS: &[&str] = &[
    "reg_delete",
    "service_stop",
    "service_disable",
    "task_disable",
    "process_kill",
];

pub fn definitions() -> Vec<AnthropicTool> {
    vec![
        // ---- 只读 ----
        AnthropicTool {
            name: "query_pc_namespace".into(),
            description: "扫描『此电脑』命名空间项 (HKCU NameSpace), 返回所有 CLSID 及其名称/图标/InProcServer。用于识别国产网盘塞的伪文件夹。".into(),
            input_schema: json!({"type": "object", "properties": {}, "required": []}),
        },
        AnthropicTool {
            name: "query_processes".into(),
            description: "列出当前所有进程及其 PID/exe 路径/父 PID。可选用 name_substr 过滤。".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name_substr": {"type": "string", "description": "子串过滤(大小写不敏感),空则返回所有"}
                },
                "required": []
            }),
        },
        AnthropicTool {
            name: "query_services".into(),
            description: "列出 Windows 服务及其状态/启动类型/PathName。可选 name_substr 过滤。".into(),
            input_schema: json!({
                "type": "object",
                "properties": {"name_substr": {"type": "string"}},
                "required": []
            }),
        },
        AnthropicTool {
            name: "query_registry_value".into(),
            description: "读取注册表某个键下的所有值。target 形如 HKCU\\Software\\Foo。".into(),
            input_schema: json!({
                "type": "object",
                "properties": {"target": {"type": "string"}},
                "required": ["target"]
            }),
        },

        // ---- 破坏性 ----
        AnthropicTool {
            name: "reg_delete".into(),
            description: "删除注册表键(含子树)。会先自动快照, 可还原。target 形如 HKCU\\Software\\Foo。受白名单护栏拦截关键系统键。".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "target": {"type": "string"},
                    "reason": {"type": "string", "description": "给用户看的中文原因, 必填"}
                },
                "required": ["target", "reason"]
            }),
        },
        AnthropicTool {
            name: "service_stop".into(),
            description: "停止 Windows 服务, 5 秒内验证已 Stopped。可还原 (重新启动)。".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string"},
                    "reason": {"type": "string"}
                },
                "required": ["name", "reason"]
            }),
        },
        AnthropicTool {
            name: "service_disable".into(),
            description: "设服务启动类型为 Disabled, 阻止下次开机自启。可还原 (改回原 start_type)。".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string"},
                    "reason": {"type": "string"}
                },
                "required": ["name", "reason"]
            }),
        },
        AnthropicTool {
            name: "task_disable".into(),
            description: "禁用计划任务。task_path 形如 \\Microsoft\\Windows\\Foo。可还原 (重新启用)。".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "task_path": {"type": "string"},
                    "reason": {"type": "string"}
                },
                "required": ["task_path", "reason"]
            }),
        },
        AnthropicTool {
            name: "process_kill".into(),
            description: "按 exe 名杀掉所有匹配进程。不可逆! Reviewer 会重点审。".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string"},
                    "reason": {"type": "string"}
                },
                "required": ["name", "reason"]
            }),
        },
    ]
}

pub fn is_destructive(name: &str) -> bool {
    DESTRUCTIVE_TOOLS.contains(&name)
}

// ---------- Tool 执行 ----------

#[derive(Debug, Clone, Serialize)]
pub struct ToolOutput {
    pub ok: bool,
    pub summary: String,
    pub data: Value,
}

pub fn run_tool(name: &str, args: &Value) -> Result<ToolOutput> {
    match name {
        "query_pc_namespace" => run_query_namespace(),
        "query_processes" => run_query_processes(args),
        "query_services" => run_query_services(args),
        "query_registry_value" => run_query_reg_value(args),
        "reg_delete" => run_reg_delete(args),
        "service_stop" => run_service_stop(args),
        "service_disable" => run_service_disable(args),
        "task_disable" => run_task_disable(args),
        "process_kill" => run_process_kill(args),
        other => Err(anyhow!("未知 tool: {other}")),
    }
}

fn arg_str(args: &Value, key: &str) -> Result<String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or_else(|| anyhow!("缺少参数: {key}"))
}

fn arg_str_opt(args: &Value, key: &str) -> Option<String> {
    args.get(key).and_then(|v| v.as_str()).map(String::from)
}

fn run_query_namespace() -> Result<ToolOutput> {
    let items = crate::inventory::namespace::scan_pc_namespace_items()?;
    let data = serde_json::to_value(&items)?;
    let total = items.len();
    let rogue = items.iter().filter(|i| !i.is_system).count();
    Ok(ToolOutput {
        ok: true,
        summary: format!("命名空间项 {total} 个 (其中 {rogue} 个第三方)"),
        data,
    })
}

fn run_query_processes(args: &Value) -> Result<ToolOutput> {
    use sysinfo::System;
    let filter = arg_str_opt(args, "name_substr")
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    let mut sys = System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let mut out = Vec::new();
    for (pid, p) in sys.processes() {
        let name = p.name().to_string_lossy().to_string();
        if !filter.is_empty() && !name.to_ascii_lowercase().contains(&filter) {
            continue;
        }
        out.push(json!({
            "pid": pid.as_u32(),
            "name": name,
            "exe": p.exe().map(|e| e.display().to_string()).unwrap_or_default(),
            "ppid": p.parent().map(|pp| pp.as_u32()),
        }));
    }
    let summary = format!("命中 {} 个进程 (filter={:?})", out.len(), filter);
    Ok(ToolOutput { ok: true, summary, data: Value::Array(out) })
}

fn run_query_services(args: &Value) -> Result<ToolOutput> {
    // 用 PowerShell Get-Service 简化 (本工具的 windows-service crate 不直接列所有)
    let filter = arg_str_opt(args, "name_substr").unwrap_or_default();
    let out = crate::sys_cmd::cmd("powershell")
        .args(["-NoProfile", "-Command",
               "Get-CimInstance Win32_Service | Select-Object Name,DisplayName,State,StartMode,PathName,ProcessId | ConvertTo-Json -Depth 3 -Compress"])
        .output()
        .context("调用 powershell Get-CimInstance Win32_Service 失败")?;
    if !out.status.success() {
        return Err(anyhow!("Get-Service 失败: {}", String::from_utf8_lossy(&out.stderr)));
    }
    let txt = String::from_utf8_lossy(&out.stdout);
    let all: Value = serde_json::from_str(&txt).unwrap_or(Value::Array(vec![]));
    let arr = if let Value::Array(a) = all { a } else { vec![all] };
    let filter_lc = filter.to_ascii_lowercase();
    let filtered: Vec<Value> = arr
        .into_iter()
        .filter(|s| {
            if filter.is_empty() {
                return true;
            }
            let name = s.get("Name").and_then(|v| v.as_str()).unwrap_or("");
            let dn = s.get("DisplayName").and_then(|v| v.as_str()).unwrap_or("");
            let p = s.get("PathName").and_then(|v| v.as_str()).unwrap_or("");
            name.to_ascii_lowercase().contains(&filter_lc)
                || dn.to_ascii_lowercase().contains(&filter_lc)
                || p.to_ascii_lowercase().contains(&filter_lc)
        })
        .collect();
    let summary = format!("命中 {} 个服务 (filter={:?})", filtered.len(), filter);
    Ok(ToolOutput { ok: true, summary, data: Value::Array(filtered) })
}

fn run_query_reg_value(args: &Value) -> Result<ToolOutput> {
    let target = arg_str(args, "target")?;
    let (hive, path) = crate::whitelist::split_hive(&target)
        .ok_or_else(|| anyhow!("无法识别 hive: {target}"))?;
    let tree = crate::snapshot::reg::read_subtree(hive, &path)?;
    let data = serde_json::to_value(&tree)?;
    Ok(ToolOutput {
        ok: true,
        summary: format!("读 {} 下 {} 个值, {} 个子键", target, tree.values.len(), tree.subkeys.len()),
        data,
    })
}

// ---- 破坏性 tool: 共用 ad-hoc action 路径 ----
// 这些是用户/Reviewer 已经批准后才进的 run_tool, 直接走 action 模块

fn run_reg_delete(args: &Value) -> Result<ToolOutput> {
    let target = arg_str(args, "target")?;
    let reason = arg_str(args, "reason")?;
    let action = crate::profile::Action {
        kind: "reg-delete".into(),
        target: target.clone(),
        reason,
        elevate: false,
        rollback: None,
    };
    let p = make_adhoc_profile(action.clone());
    let r = crate::action::execute_action(&p, 0);
    Ok(ToolOutput {
        ok: r.success,
        summary: if r.success {
            format!("已删 {target}, snapshot={}", r.snapshot_id.unwrap_or_default())
        } else {
            format!("失败: {}", r.error.unwrap_or_default())
        },
        data: serde_json::to_value(&r.plan)?,
    })
}

fn run_service_stop(args: &Value) -> Result<ToolOutput> {
    run_simple_action("service-stop", arg_str(args, "name")?, arg_str(args, "reason")?, true)
}
fn run_service_disable(args: &Value) -> Result<ToolOutput> {
    run_simple_action("service-disable", arg_str(args, "name")?, arg_str(args, "reason")?, true)
}
fn run_task_disable(args: &Value) -> Result<ToolOutput> {
    run_simple_action("task-disable", arg_str(args, "task_path")?, arg_str(args, "reason")?, true)
}
fn run_process_kill(args: &Value) -> Result<ToolOutput> {
    run_simple_action("process-kill", arg_str(args, "name")?, arg_str(args, "reason")?, false)
}

fn run_simple_action(kind: &str, target: String, reason: String, elevate: bool) -> Result<ToolOutput> {
    let action = crate::profile::Action {
        kind: kind.into(),
        target: target.clone(),
        reason,
        elevate,
        rollback: None,
    };
    let p = make_adhoc_profile(action);
    let r = crate::action::execute_action(&p, 0);
    Ok(ToolOutput {
        ok: r.success,
        summary: if r.success {
            format!("{kind} {target} 成功, snapshot={}", r.snapshot_id.unwrap_or_default())
        } else {
            format!("{kind} {target} 失败: {}", r.error.unwrap_or_default())
        },
        data: serde_json::to_value(&r.plan)?,
    })
}

fn make_adhoc_profile(action: crate::profile::Action) -> crate::profile::Profile {
    crate::profile::Profile {
        id: "ai-adhoc".into(),
        name: "AI 临时动作".into(),
        vendor: "ai".into(),
        category: "other".into(),
        severity: "medium".into(),
        tested_on: None,
        fingerprints: Default::default(),
        actions: vec![action],
        verify: vec![],
        notes: None,
    }
}
