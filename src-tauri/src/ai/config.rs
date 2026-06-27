//! AI 配置 (API key 等), 存 %APPDATA%\kuake-fuckyou\ai-config.json

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiConfig {
    pub provider: String,            // 当前 MVP 只支持 "anthropic"
    pub api_key: String,
    pub base_url: String,
    pub model_executor: String,
    pub model_reviewer: String,
    /// "auto_safe" 不需要的破坏性动作是否每次都要用户审批
    /// false = 危险动作总是需要审批(默认,推荐)
    /// true  = 用户授权一次后会话内自动放行(省心但风险更高)
    #[serde(default)]
    pub auto_approve_all: bool,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            // anthropic | openai (后者也含所有 OpenAI 兼容服务: DeepSeek/Kimi/智谱/本地 vllm 等)
            provider: "anthropic".into(),
            api_key: String::new(),
            base_url: "https://api.anthropic.com".into(),
            // 默认用最新 Claude 模型
            model_executor: "claude-sonnet-4-6".into(),
            model_reviewer: "claude-haiku-4-5-20251001".into(),
            auto_approve_all: false,
        }
    }
}

pub fn config_path() -> PathBuf {
    let dir = std::env::var("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    dir.join("kuake-fuckyou").join("ai-config.json")
}

pub fn load() -> AiConfig {
    let p = config_path();
    if !p.is_file() {
        return AiConfig::default();
    }
    match std::fs::read_to_string(&p) {
        Ok(txt) => serde_json::from_str(&txt).unwrap_or_else(|e| {
            eprintln!("AI 配置解析失败 {p:?}: {e}, 用默认");
            AiConfig::default()
        }),
        Err(_) => AiConfig::default(),
    }
}

pub fn save(cfg: &AiConfig) -> Result<()> {
    let p = config_path();
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("创建目录失败: {parent:?}"))?;
    }
    let json = serde_json::to_string_pretty(cfg)?;
    std::fs::write(&p, json).with_context(|| format!("写配置失败: {p:?}"))?;
    Ok(())
}

/// 取一个安全用于前端展示的版本 (API key 脱敏)
/// **必须按 chars 切, 否则非 ASCII key (含中文/emoji) 会 panic on char boundary**
pub fn redact(cfg: &AiConfig) -> AiConfig {
    let mut c = cfg.clone();
    let chars: Vec<char> = c.api_key.chars().collect();
    if chars.len() > 10 {
        let head: String = chars.iter().take(6).collect();
        let tail: String = chars.iter().skip(chars.len() - 4).collect();
        c.api_key = format!("{head}...{tail}");
    } else if !chars.is_empty() {
        c.api_key = "****".into();
    }
    c
}

pub fn is_configured(cfg: &AiConfig) -> bool {
    !cfg.api_key.is_empty()
}
