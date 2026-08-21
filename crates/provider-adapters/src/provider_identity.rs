//! Provider 身份与模型能力类型（P0-B Extract）。
//!
//! 从 `assistant-protocol::v1::provider` 提取到 provider-adapters 本地，
//! 解除对 legacy `assistant-protocol` 的依赖（P0-B 矩阵：Extract）。
//! 字段与 serde 表示保持与旧类型字节级兼容，避免契约回归。

use serde::{Deserialize, Serialize};

/// Provider type identifier（原 `assistant_protocol::v1::provider::ProviderType`）。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderType {
    Openai,
    Anthropic,
    Gemini,
    Deepseek,
    OpenaiCompatible,
    Ollama,
}

impl ProviderType {
    /// 稳定字符串标识（DB 持久化 / 前端透传使用）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Openai => "openai",
            Self::Anthropic => "anthropic",
            Self::Gemini => "gemini",
            Self::Deepseek => "deepseek",
            Self::OpenaiCompatible => "openai_compatible",
            Self::Ollama => "ollama",
        }
    }
}

/// Model capability flags（原 `assistant_protocol::v1::provider::ModelCapabilities`）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelCapabilities {
    pub streaming: bool,
    pub image_input: bool,
    pub file_input: bool,
    pub reasoning: bool,
    pub tool_calling: bool,
    pub structured_output: bool,
    pub function_calling: bool,
    pub system_prompt: bool,
}
