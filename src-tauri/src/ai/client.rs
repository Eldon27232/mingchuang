//! LLM 客户端 — 抽象 Anthropic 和 OpenAI 兼容两种 schema

use crate::ai::config::AiConfig;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicMessage {
    pub role: String,
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
    #[allow(dead_code)]
    pub id: String,
    #[allow(dead_code)]
    pub stop_reason: Option<String>,
    pub content: Vec<AnthropicBlock>,
}

/// 内部统一的 block 模型 — 不管 provider 是 Anthropic 还是 OpenAI, 最终都转成这个
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
    #[allow(dead_code)]
    ToolResult {
        tool_use_id: String,
        content: serde_json::Value,
    },
}

pub async fn call(cfg: &AiConfig, req: &AnthropicRequest) -> Result<AnthropicResponse> {
    if cfg.api_key.is_empty() {
        return Err(anyhow!("未配置 API key, 请在设置里填入"));
    }
    match cfg.provider.as_str() {
        "anthropic" => call_anthropic(cfg, req).await,
        "openai" | "custom" => call_openai(cfg, req).await,
        other => Err(anyhow!("未知 provider: {other}")),
    }
}

async fn http_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(180))
        .build()?)
}

async fn call_anthropic(cfg: &AiConfig, req: &AnthropicRequest) -> Result<AnthropicResponse> {
    let url = format!("{}/v1/messages", cfg.base_url.trim_end_matches('/'));
    let resp = http_client()
        .await?
        .post(&url)
        .header("x-api-key", &cfg.api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(req)
        .send()
        .await
        .context("HTTP 请求失败 (网络/代理问题)")?;
    let status = resp.status();
    let body = resp.text().await.context("读响应体失败")?;
    if !status.is_success() {
        return Err(anyhow!("Anthropic API {status}: {body}"));
    }
    serde_json::from_str::<AnthropicResponse>(&body)
        .with_context(|| format!("解析 Anthropic 响应失败: {body}"))
}

/// OpenAI 兼容路径 (DeepSeek/Kimi/智谱/Ollama/vllm/Together 都按此 schema)
async fn call_openai(cfg: &AiConfig, req: &AnthropicRequest) -> Result<AnthropicResponse> {
    let url = format!(
        "{}/v1/chat/completions",
        cfg.base_url.trim_end_matches('/')
    );

    let openai_req = anth_to_openai(req)?;
    let resp = http_client()
        .await?
        .post(&url)
        .bearer_auth(&cfg.api_key)
        .header("content-type", "application/json")
        .json(&openai_req)
        .send()
        .await
        .context("HTTP 请求失败 (网络/代理问题)")?;
    let status = resp.status();
    let body = resp.text().await.context("读响应体失败")?;
    if !status.is_success() {
        return Err(anyhow!("OpenAI API {status}: {body}"));
    }
    let parsed: serde_json::Value = serde_json::from_str(&body)
        .with_context(|| format!("解析 OpenAI 响应失败: {body}"))?;
    openai_to_anth(&parsed).with_context(|| format!("OpenAI 响应转换失败: {body}"))
}

/// 把内部 Anthropic-style request 转成 OpenAI Chat Completions request
fn anth_to_openai(req: &AnthropicRequest) -> Result<serde_json::Value> {
    let mut messages: Vec<serde_json::Value> = Vec::new();
    if let Some(sys) = &req.system {
        messages.push(serde_json::json!({"role": "system", "content": sys}));
    }
    for m in &req.messages {
        match m.role.as_str() {
            "user" => {
                // content 可能是 String 或 Array of blocks (含 tool_result)
                if let Some(s) = m.content.as_str() {
                    messages.push(serde_json::json!({"role": "user", "content": s}));
                } else if let Some(arr) = m.content.as_array() {
                    // 检查是否含 tool_result block
                    let mut text_parts: Vec<String> = Vec::new();
                    let mut tool_results: Vec<serde_json::Value> = Vec::new();
                    for b in arr {
                        let ty = b.get("type").and_then(|v| v.as_str()).unwrap_or("");
                        match ty {
                            "tool_result" => {
                                let id = b.get("tool_use_id").and_then(|v| v.as_str()).unwrap_or("");
                                let content = b.get("content").cloned().unwrap_or(serde_json::Value::Null);
                                let content_str = content.as_str().map(String::from)
                                    .unwrap_or_else(|| content.to_string());
                                tool_results.push(serde_json::json!({
                                    "role": "tool",
                                    "tool_call_id": id,
                                    "content": content_str,
                                }));
                            }
                            "text" => {
                                if let Some(t) = b.get("text").and_then(|v| v.as_str()) {
                                    text_parts.push(t.to_string());
                                }
                            }
                            _ => {}
                        }
                    }
                    if !text_parts.is_empty() {
                        messages.push(serde_json::json!({"role": "user", "content": text_parts.join("\n")}));
                    }
                    messages.extend(tool_results);
                }
            }
            "assistant" => {
                // content 通常是 Array (text + tool_use)
                let mut text_parts: Vec<String> = Vec::new();
                let mut tool_calls: Vec<serde_json::Value> = Vec::new();
                if let Some(arr) = m.content.as_array() {
                    for b in arr {
                        let ty = b.get("type").and_then(|v| v.as_str()).unwrap_or("");
                        match ty {
                            "text" => {
                                if let Some(t) = b.get("text").and_then(|v| v.as_str()) {
                                    text_parts.push(t.to_string());
                                }
                            }
                            "tool_use" => {
                                let id = b.get("id").and_then(|v| v.as_str()).unwrap_or("");
                                let name = b.get("name").and_then(|v| v.as_str()).unwrap_or("");
                                let input = b.get("input").cloned().unwrap_or(serde_json::Value::Null);
                                tool_calls.push(serde_json::json!({
                                    "id": id,
                                    "type": "function",
                                    "function": {
                                        "name": name,
                                        "arguments": input.to_string(),
                                    }
                                }));
                            }
                            _ => {}
                        }
                    }
                } else if let Some(s) = m.content.as_str() {
                    text_parts.push(s.to_string());
                }
                let mut msg = serde_json::json!({
                    "role": "assistant",
                    "content": if text_parts.is_empty() { serde_json::Value::Null } else { serde_json::Value::String(text_parts.join("\n")) }
                });
                if !tool_calls.is_empty() {
                    msg.as_object_mut().unwrap().insert("tool_calls".into(), serde_json::Value::Array(tool_calls));
                }
                messages.push(msg);
            }
            _ => {}
        }
    }

    let tools: Vec<serde_json::Value> = req
        .tools
        .iter()
        .map(|t| {
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.input_schema,
                }
            })
        })
        .collect();

    let mut body = serde_json::json!({
        "model": req.model,
        "max_tokens": req.max_tokens,
        "messages": messages,
    });
    if !tools.is_empty() {
        body.as_object_mut().unwrap().insert("tools".into(), serde_json::Value::Array(tools));
    }
    Ok(body)
}

fn openai_to_anth(resp: &serde_json::Value) -> Result<AnthropicResponse> {
    let id = resp.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let choices = resp
        .get("choices")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("OpenAI 响应缺 choices"))?;
    let first = choices.first().ok_or_else(|| anyhow!("choices 为空"))?;
    let stop_reason = first.get("finish_reason").and_then(|v| v.as_str()).map(String::from);
    let msg = first.get("message").ok_or_else(|| anyhow!("缺 message"))?;

    let mut content: Vec<AnthropicBlock> = Vec::new();
    if let Some(text) = msg.get("content").and_then(|v| v.as_str()) {
        if !text.is_empty() {
            content.push(AnthropicBlock::Text { text: text.to_string() });
        }
    }
    if let Some(calls) = msg.get("tool_calls").and_then(|v| v.as_array()) {
        for c in calls {
            let call_id = c.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let func = c.get("function").ok_or_else(|| anyhow!("tool_call 缺 function"))?;
            let name = func.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let args_str = func.get("arguments").and_then(|v| v.as_str()).unwrap_or("{}");
            let input: serde_json::Value = serde_json::from_str(args_str)
                .unwrap_or(serde_json::Value::Object(Default::default()));
            content.push(AnthropicBlock::ToolUse { id: call_id, name, input });
        }
    }

    Ok(AnthropicResponse {
        id,
        stop_reason,
        content,
    })
}
