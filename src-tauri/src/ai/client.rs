//! Anthropic Messages API 客户端 (MVP, OpenAI 兼容下一轮)
//!
//! POST {base_url}/v1/messages
//! Headers: x-api-key, anthropic-version: 2023-06-01, content-type: application/json
//! Body: { model, max_tokens, system, messages, tools }
//!
//! 返回:
//! { content: [ { type: "text" | "tool_use", text, id, name, input } ] }
//!   stop_reason: "tool_use" | "end_turn"

use crate::ai::config::AiConfig;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicMessage {
    pub role: String, // "user" | "assistant"
    pub content: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct AnthropicTool {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct AnthropicRequest {
    pub model: String,
    pub max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    pub messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<AnthropicTool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AnthropicResponse {
    pub id: String,
    pub stop_reason: Option<String>,
    pub content: Vec<AnthropicBlock>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AnthropicBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        tool_use_id: String,
        content: serde_json::Value,
    },
}

pub async fn call(cfg: &AiConfig, req: &AnthropicRequest) -> Result<AnthropicResponse> {
    if cfg.api_key.is_empty() {
        return Err(anyhow!("未配置 API key, 请在设置里填入"));
    }
    let url = format!("{}/v1/messages", cfg.base_url.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(180))
        .build()?;
    let resp = client
        .post(&url)
        .header("x-api-key", &cfg.api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(req)
        .send()
        .await
        .context("HTTP 请求失败 (可能是网络/代理问题)")?;
    let status = resp.status();
    let body_txt = resp.text().await.context("读取响应体失败")?;
    if !status.is_success() {
        return Err(anyhow!("Anthropic API 返回 {status}: {body_txt}"));
    }
    serde_json::from_str::<AnthropicResponse>(&body_txt)
        .with_context(|| format!("解析 Anthropic 响应失败, 原文: {body_txt}"))
}
