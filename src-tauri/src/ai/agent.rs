//! Executor + Reviewer 编排
//!
//! Session 全局状态 (Mutex<HashMap<id, Session>>):
//!   - 用户发消息 → push to messages → 启动 Executor
//!   - Executor 调 LLM → 拿到 text/tool_use blocks
//!     - text: append to messages, 显示
//!     - tool_use:
//!         - 只读 tool: 直接 run_tool, 结果回喂 Executor, 继续 loop
//!         - 破坏性 tool: 调 Reviewer 评估 → safe 自动执行 / needs_approval 等用户
//!   - stop_reason=end_turn: 会话结束

use crate::ai::client::{self, AnthropicBlock, AnthropicMessage, AnthropicRequest};
use crate::ai::config::AiConfig;
use crate::ai::tools;
use crate::ai::{ChatMessage, ReviewResult, ToolCallStatus, ToolCallView};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

const MAX_TOKENS: u32 = 4096;
const SYSTEM_PROMPT: &str = r#"你是明窗 (Mingchuang) 内置的 Windows 治理 AI, 帮用户对付国产流氓软件 (夸克网盘 / 酷狗 / 123 云盘 / 百度网盘 / 搜狗输入法之类)。运行环境是 Windows 11, 工具调用直接动用户机器。

# 你应该是什么样

**直接、行动导向**。用户来问"X 装了卸不掉", 你不要先问 5 个澄清问题。直接调 query_* 看现状, 凭看到的事实下判断, 给出结论 + 计划 + 执行。

**先证据再动作**。在调任何破坏性 tool (reg_delete / service_* / task_disable / process_kill) 之前, 必须先用 query_* 看到具体目标存在。不要凭"这家厂商一般会装这个"的猜测就动手。

**不偷懒, 不甩锅**。
- 你说"工具不支持枚举子键名称" → **错的**, 用 list_registry_subkeys
- 你说"我需要用户确认才能执行" → 用户已经在跟你说话了, 你直接调; reviewer 会拦下需要二次确认的, 你不要自己提前甩锅
- 你说"建议用户手动操作" → 不行, 你能调的就调; 真做不到 (比如要 UEFI 改 BIOS 这种) 才让用户做

**最小动作**。能停服务就别杀进程, 能删一个键就别删整棵子树。每一步都说清楚 reason。

**结果验证**。动完手再调一次相关的 query_* 看是不是真生效, 不能拍脑袋说"已完成"。

# 工作流模式

典型任务: 用户说"夸克网盘卸不掉, 后台还在跑"

1. **诊断** (并行用只读工具):
   - query_processes name_substr="quark" → 看进程
   - query_services name_substr="quark" → 看服务
   - query_pc_namespace → 看「此电脑」有没有它的伪文件夹
   - list_registry_subkeys target="HKCU\\Software" 看注册痕迹
2. **报告** + 计划: 列出找到的 N 个东西, 简要说明每一项是什么 + 准备怎么处理
3. **动作** (破坏性 tool, 每条都填 reason):
   - reg_delete "此电脑" 命名空间项 (HKCU\\...\\NameSpace\\{CLSID} 和 HKCU\\Software\\Classes\\CLSID\\{CLSID} 两条, 见知识库)
   - service_stop + service_disable 它的保活服务
   - process_kill 残余进程
4. **验证**: 再 query 一遍, 确认目标消失

# 工具速查 (有 ✓ 是只读不需要审批; 有 ⚠ 是破坏性, 走 Reviewer + 用户审批)

只读 (诊断用):
- `query_pc_namespace` ✓ — 列「此电脑」HKCU NameSpace 下所有 CLSID 项 (国产网盘伪文件夹的家)
- `query_processes` ✓ — 列进程, 可 name_substr 过滤
- `query_services` ✓ — 列 Windows 服务, 可 name_substr 过滤
- `query_registry_value` ✓ — 读 target 键下【完整子树】(values + subkeys 递归)。看不熟的分支用它
- `list_registry_subkeys` ✓ — 仅枚举 target 下【直接子键名称】, 不读 value, 不递归。比 query_registry_value 快; 想先看结构再深入用它

破坏性 (动手用, 全部支持快照还原, process_kill 除外):
- `reg_delete target reason` ⚠ — 删注册表键 (含子树, 自动快照)
- `service_stop name reason` ⚠ — 停服务, 5s 内验证
- `service_disable name reason` ⚠ — 改服务启动类型为 Disabled
- `task_disable task_path reason` ⚠ — 禁用计划任务
- `process_kill name reason` ⚠ — 按 exe 名杀, **不可逆**

# 安全护栏

- 系统关键键 (HKLM\\SYSTEM, Defender, Update, explorer 自身) **已经在代码层白名单挡死**, 你调 reg_delete 命中会自动 deny, 不用担心误删
- 不要碰 svchost.exe / csrss.exe / lsass.exe / wininit.exe / services.exe — Reviewer 会立即 deny
- Windows Defender 的服务名 (WinDefend / WdNisSvc / SecurityHealthService) 不要停 — 同上

# 知识库 (国产流氓软件常见痕迹)

**"此电脑" 伪文件夹** = `HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\MyComputer\NameSpace\{CLSID}` + `HKCU\Software\Classes\CLSID\{CLSID}` 两条都得删, 单删一条 explorer 自愈机制会写回。

**自启常见点**:
- `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` + `RunOnce`
- `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Run`
- 计划任务: 用 query_services 找不到自启项时, 怀疑是计划任务

**保活服务命名套路**: 厂商英文名 + Service/Guard/Helper/Daemon 后缀 (例: KGMusicService / QuarkService / CloudMusicServ)。

**进程命名套路**: 主程序 + 守护伴生 + 推送/升级伴生 (例: QQ.exe / QQDaemon.exe / QQUpdater.exe), 全杀干净。

# 对话风格

简体中文, 直接简洁。报告调研结果用 markdown 列表/表格 (前端有 markdown 渲染)。代码块用 ``` 括起来。

不要写"我将先扫描进程..." 这种废话铺垫, 直接给结果 + 结论。"#;

const REVIEWER_PROMPT: &str = r#"你是 mingchuang 的安全审查员。Executor 想调用一个破坏性工具,
你的任务是判断:
- safe: 完全安全, 可以直接执行 (例: 删除已知国产流氓的注册表项 / 停掉其服务)
- needs_approval: 风险存在,需要让用户确认 (例: 杀进程, 操作不熟悉的服务名)
- deny: 明显危险,应直接拒绝 (例: 操作 Defender / 系统服务)

输出严格 JSON: {"verdict": "safe|needs_approval|deny", "reason": "中文简短理由"}

只输出 JSON, 不要其他文字。"#;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub messages: Vec<ChatMessage>,
    /// 待用户审批的 tool call (一次只有一个)
    pub pending_call: Option<ToolCallView>,
    pub status: SessionStatus,
    /// 累计 tool 调用次数(仅供展示, 不再设硬上限)
    pub tool_call_count: usize,
    pub last_error: Option<String>,
    /// 用户请求中止此 session 的执行
    #[serde(default)]
    pub aborted: bool,
    /// 创建时间 + 最后修改时间(给前端列表用)
    #[serde(default)]
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Idle,
    Thinking,
    WaitingApproval,
    Done,
    Failed,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    Approve,
    Deny,
}

// ============ 持久化 ============
fn sessions_dir() -> PathBuf {
    let local = std::env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    local.join("mingchuang").join("ai-sessions")
}

fn session_file(id: &str) -> PathBuf {
    sessions_dir().join(format!("{id}.json"))
}

fn save_session_to_disk(s: &Session) {
    let dir = sessions_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let Ok(json) = serde_json::to_string_pretty(s) else { return; };
    // 原子写: 先写 .tmp 再 rename, 避免 relaunch/崩溃留下半残 JSON
    let final_path = session_file(&s.id);
    let tmp_path = final_path.with_extension("json.tmp");
    if std::fs::write(&tmp_path, json).is_ok() {
        let _ = std::fs::rename(&tmp_path, &final_path);
    }
}

fn load_all_sessions_from_disk() -> HashMap<String, Session> {
    let mut out = HashMap::new();
    let dir = sessions_dir();
    if !dir.is_dir() { return out; }
    let Ok(entries) = std::fs::read_dir(&dir) else { return out; };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") { continue; }
        let Ok(txt) = std::fs::read_to_string(&path) else { continue; };
        match serde_json::from_str::<Session>(&txt) {
            Ok(s) => { out.insert(s.id.clone(), s); }
            Err(e) => {
                // 损坏文件转 .broken, 避免下次加载又卡住
                eprintln!("session 损坏 {path:?}: {e}");
                let _ = std::fs::rename(&path, path.with_extension("json.broken"));
            }
        }
    }
    out
}

/// 列出所有持久化 session(给前端历史会话列表用)。按 updated_at 倒序。
pub fn list_persisted_sessions() -> Vec<SessionSummary> {
    let mut sessions: Vec<Session> = with_sessions(|m| m.values().cloned().collect());
    sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    sessions
        .into_iter()
        .map(|s| {
            let first_user = s
                .messages
                .iter()
                .find(|m| m.role == "user")
                .map(|m| m.content.chars().take(60).collect::<String>())
                .unwrap_or_default();
            SessionSummary {
                id: s.id,
                title: if first_user.is_empty() { "新会话".into() } else { first_user },
                status: s.status,
                updated_at: s.updated_at,
                message_count: s.messages.len(),
            }
        })
        .collect()
}

pub fn delete_session(id: &str) -> Result<()> {
    with_sessions(|m| m.remove(id));
    let path = session_file(id);
    if path.exists() {
        std::fs::remove_file(&path)
            .with_context(|| format!("删除 session 文件失败: {path:?}"))?;
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionSummary {
    pub id: String,
    pub title: String,
    pub status: SessionStatus,
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
    pub message_count: usize,
}

static SESSIONS: Mutex<Option<HashMap<String, Session>>> = Mutex::new(None);

fn with_sessions<F, R>(f: F) -> R
where
    F: FnOnce(&mut HashMap<String, Session>) -> R,
{
    let mut g = SESSIONS.lock().unwrap();
    if g.is_none() {
        // 首次访问: 从磁盘加载所有 session
        *g = Some(load_all_sessions_from_disk());
    }
    f(g.as_mut().unwrap())
}

/// 在每次 session 修改后调用, 自动持久化
fn touch_and_persist(session_id: &str) {
    let snapshot = with_sessions(|m| {
        if let Some(s) = m.get_mut(session_id) {
            s.updated_at = Some(chrono::Utc::now());
            Some(s.clone())
        } else { None }
    });
    if let Some(s) = snapshot {
        save_session_to_disk(&s);
    }
}

pub fn create_session() -> String {
    let id = crate::snapshot::new_snapshot_id();
    let now = chrono::Utc::now();
    let s = Session {
        id: id.clone(),
        messages: Vec::new(),
        pending_call: None,
        status: SessionStatus::Idle,
        tool_call_count: 0,
        last_error: None,
        aborted: false,
        created_at: Some(now),
        updated_at: Some(now),
    };
    with_sessions(|m| m.insert(id.clone(), s.clone()));
    save_session_to_disk(&s);
    id
}

/// 请求中止: 下一次 LLM 调用返回前会被检查, 设为 Done 并清 pending
pub fn abort_session(session_id: &str) {
    with_sessions(|m| {
        if let Some(s) = m.get_mut(session_id) {
            s.aborted = true;
            s.pending_call = None;
            if matches!(s.status, SessionStatus::Thinking | SessionStatus::WaitingApproval) {
                s.status = SessionStatus::Done;
                s.last_error = Some("已被用户中止".into());
            }
        }
    });
}

/// 重试最后一条用户消息: 截掉之后的所有 messages, 然后再发一次
pub async fn retry_last(session_id: &str) -> Result<()> {
    let cfg = crate::ai::config::load();
    let last_user = with_sessions(|m| {
        m.get_mut(session_id).and_then(|s| {
            let idx = s.messages.iter().rposition(|m| m.role == "user")?;
            let text = s.messages[idx].content.clone();
            s.messages.truncate(idx);
            s.aborted = false;
            s.last_error = None;
            s.status = SessionStatus::Thinking;
            Some(text)
        })
    });
    match last_user {
        Some(t) => send_user_message(session_id, t).await,
        None => Err(anyhow!("没有可重试的用户消息")),
    }
}

/// 编辑某条用户消息: 把该 index 之后(含)的全删掉, 用 new_content 重新发
pub async fn edit_user_message(
    session_id: &str,
    msg_index: usize,
    new_content: String,
) -> Result<()> {
    let cfg = crate::ai::config::load();
    let ok = with_sessions(|m| {
        m.get_mut(session_id).map(|s| {
            if msg_index < s.messages.len() && s.messages[msg_index].role == "user" {
                s.messages.truncate(msg_index);
                s.aborted = false;
                s.last_error = None;
                s.status = SessionStatus::Thinking;
                true
            } else {
                false
            }
        })
    }).unwrap_or(false);
    if !ok {
        return Err(anyhow!("msg_index 不指向有效的 user 消息"));
    }
    send_user_message(session_id, new_content).await
}

fn check_aborted(session_id: &str) -> bool {
    with_sessions(|m| m.get(session_id).map(|s| s.aborted).unwrap_or(false))
}

pub fn get_session(id: &str) -> Option<Session> {
    with_sessions(|m| m.get(id).cloned())
}

/// 用户发送一条消息, 启动 agent loop
pub async fn send_user_message(session_id: &str, user_msg: String) -> Result<()> {
    let cfg = crate::ai::config::load();
    if !crate::ai::config::is_configured(&cfg) {
        return Err(anyhow!("未配置 AI API key, 请在设置里填入"));
    }

    // 互斥: 当前 thinking/waiting_approval 不接受新消息
    let busy = with_sessions(|m| {
        m.get(session_id).map(|s| {
            matches!(s.status, SessionStatus::Thinking | SessionStatus::WaitingApproval)
        }).unwrap_or(false)
    });
    if busy {
        return Err(anyhow!("AI 正在处理上一条消息, 先停止或等它完成"));
    }

    with_sessions(|m| {
        if let Some(s) = m.get_mut(session_id) {
            s.messages.push(ChatMessage {
                role: "user".into(),
                content: user_msg.clone(),
                tool_calls: None,
                tool_call_id: None,
            });
            s.status = SessionStatus::Thinking;
            s.last_error = None;
        }
    });
    touch_and_persist(session_id);

    let r = run_executor_loop(session_id, &cfg).await;
    touch_and_persist(session_id);
    r
}

/// 用户审批结果
pub async fn approve_pending(session_id: &str, decision: ApprovalDecision) -> Result<()> {
    let cfg = crate::ai::config::load();
    // 一次性把 pending_call 取走并切到 Thinking, 防止用户重复点导致并发执行
    let pending = with_sessions(|m| {
        m.get_mut(session_id).and_then(|s| {
            let p = s.pending_call.take()?;
            s.status = SessionStatus::Thinking;
            Some(p)
        })
    });
    let mut call = match pending {
        Some(c) => c,
        // 已经被处理过(并发点击),静默 OK
        None => return Ok(()),
    };

    match decision {
        ApprovalDecision::Approve => {
            call.status = ToolCallStatus::Approved;
        }
        ApprovalDecision::Deny => {
            call.status = ToolCallStatus::Denied;
            // 把"拒绝"作为 tool_result 喂回 Executor
            with_sessions(|m| {
                if let Some(s) = m.get_mut(session_id) {
                    s.pending_call = None;
                    s.status = SessionStatus::Thinking;
                    // 把 denied 状态记到最后一条 assistant 消息
                    if let Some(last) = s.messages.last_mut() {
                        if let Some(calls) = last.tool_calls.as_mut() {
                            if let Some(c) = calls.iter_mut().find(|c| c.id == call.id) {
                                c.status = ToolCallStatus::Denied;
                            }
                        }
                    }
                    // 加一个 tool 角色消息说被拒
                    s.messages.push(ChatMessage {
                        role: "tool".into(),
                        content: "用户拒绝执行此动作".into(),
                        tool_calls: None,
                        tool_call_id: Some(call.id.clone()),
                    });
                }
            });
            return run_executor_loop(session_id, &cfg).await;
        }
    }

    // approve -> 真正执行 → 继续 LLM 循环
    do_execute_call(session_id, call).await?;
    run_executor_loop(session_id, &cfg).await
}

/// 同步执行一个 tool call: 跑 tool + 写 messages + 更新状态。**不**调 run_executor_loop, 避免 async 递归。
async fn do_execute_call(session_id: &str, mut call: ToolCallView) -> Result<()> {
    call.status = ToolCallStatus::Executing;
    update_call(session_id, &call);
    let result = tools::run_tool(&call.name, &call.args);
    let (status, summary, _data) = match result {
        Ok(o) => (
            if o.ok { ToolCallStatus::Done } else { ToolCallStatus::Failed },
            o.summary,
            o.data,
        ),
        Err(e) => (ToolCallStatus::Failed, format!("执行失败: {e:#}"), json!(null)),
    };
    call.status = status.clone();
    call.result = Some(summary.clone());
    update_call(session_id, &call);

    with_sessions(|m| {
        if let Some(s) = m.get_mut(session_id) {
            s.pending_call = None;
            s.status = SessionStatus::Thinking;
            s.messages.push(ChatMessage {
                role: "tool".into(),
                content: format!("[{}] {}", call.name, summary),
                tool_calls: None,
                tool_call_id: Some(call.id.clone()),
            });
            for msg in s.messages.iter_mut().rev() {
                if let Some(calls) = msg.tool_calls.as_mut() {
                    if let Some(c) = calls.iter_mut().find(|c| c.id == call.id) {
                        c.status = status.clone();
                        c.result = Some(summary.clone());
                        break;
                    }
                }
            }
        }
    });
    Ok(())
}

fn update_call(session_id: &str, call: &ToolCallView) {
    with_sessions(|m| {
        if let Some(s) = m.get_mut(session_id) {
            for msg in s.messages.iter_mut().rev() {
                if let Some(calls) = msg.tool_calls.as_mut() {
                    if let Some(c) = calls.iter_mut().find(|c| c.id == call.id) {
                        *c = call.clone();
                        return;
                    }
                }
            }
        }
    });
}

/// Executor 主循环: 调 LLM → 处理 blocks → 继续直到 end_turn 或 等用户 / 被中止
async fn run_executor_loop(session_id: &str, cfg: &AiConfig) -> Result<()> {
    loop {
        if check_aborted(session_id) {
            with_sessions(|m| {
                if let Some(s) = m.get_mut(session_id) {
                    s.status = SessionStatus::Done;
                }
            });
            return Ok(());
        }
        // 取当前 session 状态构造 LLM 请求(不再有 tool 调用上限)
        let messages = with_sessions(|m| {
            m.get(session_id)
                .map(|s| s.messages.clone())
                .unwrap_or_default()
        });

        let anthropic_msgs = to_anthropic_messages(&messages);
        let req = AnthropicRequest {
            model: cfg.model_executor.clone(),
            max_tokens: MAX_TOKENS,
            system: Some(SYSTEM_PROMPT.into()),
            messages: anthropic_msgs,
            tools: tools::definitions(),
        };
        let resp = match client::call(cfg, &req).await {
            Ok(r) => r,
            Err(e) => {
                with_sessions(|m| {
                    if let Some(s) = m.get_mut(session_id) {
                        s.status = SessionStatus::Failed;
                        s.last_error = Some(format!("LLM 调用失败: {e:#}"));
                    }
                });
                return Err(e);
            }
        };

        // 分离 text 和 tool_use blocks
        let mut text_parts = Vec::new();
        let mut tool_calls = Vec::new();
        for b in &resp.content {
            match b {
                AnthropicBlock::Text { text } => text_parts.push(text.clone()),
                AnthropicBlock::ToolUse { id, name, input } => {
                    tool_calls.push((id.clone(), name.clone(), input.clone()));
                }
                _ => {}
            }
        }

        // 把 assistant 这一轮加进 history
        let mut tc_views = Vec::new();
        for (id, name, input) in &tool_calls {
            tc_views.push(ToolCallView {
                id: id.clone(),
                name: name.clone(),
                args: input.clone(),
                review: None,
                status: ToolCallStatus::Reviewing,
                result: None,
            });
        }
        with_sessions(|m| {
            if let Some(s) = m.get_mut(session_id) {
                s.messages.push(ChatMessage {
                    role: "assistant".into(),
                    content: text_parts.join("\n"),
                    tool_calls: if tc_views.is_empty() { None } else { Some(tc_views.clone()) },
                    tool_call_id: None,
                });
            }
        });

        if tool_calls.is_empty() {
            // end_turn
            with_sessions(|m| {
                if let Some(s) = m.get_mut(session_id) {
                    s.status = SessionStatus::Done;
                }
            });
            return Ok(());
        }

        // 处理 tool calls
        for (id, name, input) in tool_calls {
            with_sessions(|m| {
                if let Some(s) = m.get_mut(session_id) {
                    s.tool_call_count += 1;
                }
            });
            let mut call = ToolCallView {
                id: id.clone(),
                name: name.clone(),
                args: input.clone(),
                review: None,
                status: ToolCallStatus::Reviewing,
                result: None,
            };
            if tools::is_destructive(&name) {
                // 调 Reviewer
                let review = review_tool_call(cfg, &name, &input).await.unwrap_or_else(|e| ReviewResult {
                    verdict: "needs_approval".into(),
                    reason: format!("(Reviewer 调用失败, 保守降为待审批) {e:#}"),
                });
                call.review = Some(review.clone());
                match (review.verdict.as_str(), cfg.auto_approve_all) {
                    ("safe", true) => {
                        // 用户配置了全程许可 + Reviewer 判 safe → 自动执行后继续 for
                        call.status = ToolCallStatus::Approved;
                        update_call(session_id, &call);
                        do_execute_call(session_id, call).await?;
                        continue;
                    }
                    ("deny", _) => {
                        call.status = ToolCallStatus::Denied;
                        update_call(session_id, &call);
                        with_sessions(|m| {
                            if let Some(s) = m.get_mut(session_id) {
                                // 写更明确的指令避免 LLM 死循环换理由重试同一 tool
                                s.messages.push(ChatMessage {
                                    role: "tool".into(),
                                    content: format!(
                                        "审查员判定此工具调用有破坏性、已拒绝。原因: {}\n\
                                         **请不要再尝试调用 {}, 换用别的安全工具或直接给用户文字回答。**",
                                        review.reason, name
                                    ),
                                    tool_calls: None,
                                    tool_call_id: Some(call.id.clone()),
                                });
                            }
                        });
                    }
                    _ => {
                        // 等用户审批
                        call.status = ToolCallStatus::WaitingApproval;
                        update_call(session_id, &call);
                        with_sessions(|m| {
                            if let Some(s) = m.get_mut(session_id) {
                                s.pending_call = Some(call.clone());
                                s.status = SessionStatus::WaitingApproval;
                            }
                        });
                        return Ok(());
                    }
                }
            } else {
                // 安全 tool, 直接执行
                call.status = ToolCallStatus::Executing;
                update_call(session_id, &call);
                let r = tools::run_tool(&name, &input);
                let (status, summary) = match r {
                    Ok(o) => (
                        if o.ok { ToolCallStatus::Done } else { ToolCallStatus::Failed },
                        o.summary,
                    ),
                    Err(e) => (ToolCallStatus::Failed, format!("失败: {e:#}")),
                };
                call.status = status;
                call.result = Some(summary.clone());
                update_call(session_id, &call);
                with_sessions(|m| {
                    if let Some(s) = m.get_mut(session_id) {
                        s.messages.push(ChatMessage {
                            role: "tool".into(),
                            content: format!("[{name}] {summary}"),
                            tool_calls: None,
                            tool_call_id: Some(id.clone()),
                        });
                    }
                });
            }
        }
        // 继续下一轮
    }
}

async fn review_tool_call(cfg: &AiConfig, name: &str, args: &serde_json::Value) -> Result<ReviewResult> {
    let user_msg = format!(
        "Executor 想调用工具:\n  tool: {name}\n  args: {}\n\n请输出 JSON 判定。",
        serde_json::to_string_pretty(args).unwrap_or_default()
    );
    let req = AnthropicRequest {
        model: cfg.model_reviewer.clone(),
        max_tokens: 512,
        system: Some(REVIEWER_PROMPT.into()),
        messages: vec![AnthropicMessage {
            role: "user".into(),
            content: serde_json::Value::String(user_msg),
        }],
        tools: vec![],
    };
    let resp = client::call(cfg, &req).await?;
    let text = resp
        .content
        .iter()
        .filter_map(|b| match b {
            AnthropicBlock::Text { text } => Some(text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("");
    // 容错解析 JSON: 尝试找第一个 { 和最后一个 }
    // **必须用 char_indices 而不是 find — 找到 byte 索引若落在多字节中文 brace 上会 panic**
    let bytes = text.as_bytes();
    let json_str = match (text.find('{'), text.rfind('}')) {
        (Some(i), Some(j)) if i <= j && j < bytes.len() && text.is_char_boundary(i) && text.is_char_boundary(j + 1) => {
            &text[i..=j]
        }
        _ => &text[..],
    };
    serde_json::from_str::<ReviewResult>(json_str)
        .with_context(|| format!("Reviewer 输出无法解析: {text}"))
}

fn to_anthropic_messages(msgs: &[ChatMessage]) -> Vec<AnthropicMessage> {
    // 把内部 ChatMessage 转 Anthropic 的格式
    // 注意: tool_result 必须是 user role, content 数组形式
    //
    // 防 400 invalid_request: edit_user_message 截掉 messages 后, 可能留下
    // assistant.tool_calls 但没对应 tool_result。先扫一遍把没有匹配 tool_result 的
    // tool_use id 收集起来, 转换时跳过这些 tool_use block。
    let mut answered_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    for m in msgs {
        if m.role == "tool" {
            if let Some(id) = &m.tool_call_id {
                answered_ids.insert(id.clone());
            }
        }
    }

    let mut out: Vec<AnthropicMessage> = Vec::new();
    for m in msgs {
        match m.role.as_str() {
            "user" => out.push(AnthropicMessage {
                role: "user".into(),
                content: serde_json::Value::String(m.content.clone()),
            }),
            "assistant" => {
                let mut blocks: Vec<serde_json::Value> = Vec::new();
                if !m.content.is_empty() {
                    blocks.push(json!({"type": "text", "text": m.content}));
                }
                if let Some(calls) = &m.tool_calls {
                    for c in calls {
                        // 跳过没有对应 tool_result 的孤儿 tool_use (会让 API 返 400)
                        if !answered_ids.contains(&c.id) { continue; }
                        blocks.push(json!({
                            "type": "tool_use",
                            "id": c.id,
                            "name": c.name,
                            "input": c.args
                        }));
                    }
                }
                if blocks.is_empty() {
                    // 整条 assistant 消息全是孤儿 tool_use, 跳过
                    continue;
                }
                out.push(AnthropicMessage {
                    role: "assistant".into(),
                    content: serde_json::Value::Array(blocks),
                });
            }
            "tool" => {
                // 转成 user-role 的 tool_result block
                if let Some(id) = &m.tool_call_id {
                    out.push(AnthropicMessage {
                        role: "user".into(),
                        content: json!([{
                            "type": "tool_result",
                            "tool_use_id": id,
                            "content": m.content,
                        }]),
                    });
                }
            }
            _ => {}
        }
    }
    out
}
