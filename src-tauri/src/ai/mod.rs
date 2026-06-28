//! AI 双 agent: Executor + Reviewer 协作治理流氓软件
//!
//! 用户场景:
//!   用户: "123云盘装了卸不掉,帮我清"
//!   Executor agent (sonnet) → 调 query_* 看现状 → 调 reg_delete/service_disable
//!   每个破坏性 tool call → 拦截 → Reviewer agent (haiku) 评估风险
//!     - safe + 用户已"全程许可" → 自动执行
//!     - dangerous / 用户未许可 → 弹审批面板, 用户点 OK 才执行
//!
//! MVP 范围 (2026-06-27):
//! - 只支持 Anthropic provider (OpenAI 兼容下一轮)
//! - 双 LLM (Executor sonnet + Reviewer haiku)
//! - 5 个破坏性 tool (reg_delete/service_stop/service_disable/task_disable/process_kill)
//!   + 4 个只读 tool (query_namespace/processes/services/registry_value)
//! - 会话状态全局 Mutex<HashMap<id, Session>>

pub mod agent;
pub mod client;
pub mod config;
pub mod tools;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,      // "user" | "assistant" | "tool"
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCallView>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallView {
    pub id: String,
    pub name: String,
    pub args: serde_json::Value,
    /// Reviewer 评估
    pub review: Option<ReviewResult>,
    /// 执行状态
    pub status: ToolCallStatus,
    /// 执行结果 (snapshot_id / 错误 / 输出 摘要)
    pub result: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallStatus {
    /// 提交给 Reviewer 审查中
    Reviewing,
    /// 等用户审批
    WaitingApproval,
    /// 已批准, 待执行
    Approved,
    /// 用户拒绝
    Denied,
    /// 执行中
    Executing,
    /// 已完成
    Done,
    /// 执行失败
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewResult {
    pub verdict: String,        // "safe" | "needs_approval" | "deny"
    pub reason: String,         // 给用户看的中文说明
}
