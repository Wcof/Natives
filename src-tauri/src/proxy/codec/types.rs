//! Canonical Request & Event 核心类型定义。

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CanonicalImage {
    pub url: String,
    pub media_type: Option<String>,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CanonicalToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CanonicalMessage {
    pub role: String,
    pub content: String,
    pub tool_call_id: Option<String>,
    pub tool_name: Option<String>,
    pub tool_calls: Option<Vec<CanonicalToolCall>>,
    pub images: Vec<CanonicalImage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CanonicalTool {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CanonicalRequest {
    pub model: String,
    pub messages: Vec<CanonicalMessage>,
    pub system: Option<String>,
    pub tools: Vec<CanonicalTool>,
    pub tool_choice: Option<Value>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
    pub stream: bool,
    pub reasoning_effort: Option<String>,
    pub thinking_budget: Option<u64>,
    pub structured_output: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CanonicalUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub reasoning_tokens: Option<u64>,
    pub cached_tokens: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CanonicalEvent {
    TextDelta(String),
    ReasoningDelta(String),
    ToolCallDelta {
        index: usize,
        id: Option<String>,
        name: Option<String>,
        arguments_delta: String,
    },
    Usage(CanonicalUsage),
    Completed {
        finish_reason: String,
    },
    Error {
        message: String,
        code: String,
        category: String,
        retryable: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolKind {
    ChatCompletions,
    Responses,
    Messages,
}

impl ProtocolKind {
    pub fn from_path_or_name(s: &str) -> Option<Self> {
        match s {
            "/v1/chat/completions" | "openai_chat_completions" | "chat_completions" | "chat" => {
                Some(Self::ChatCompletions)
            }
            "/v1/responses" | "openai_responses" | "responses" => Some(Self::Responses),
            "/v1/messages" | "anthropic_messages" | "messages" => Some(Self::Messages),
            _ => None,
        }
    }
}
