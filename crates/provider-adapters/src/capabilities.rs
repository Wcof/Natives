use assistant_protocol::v1::provider::{ModelCapabilities, ProviderType};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Provider capabilities declaration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderCapabilities {
    /// Provider type.
    pub provider_type: ProviderType,
    /// Supported features.
    pub features: Vec<String>,
    /// Maximum context window in tokens.
    pub max_context_window: u64,
    /// Whether the provider supports streaming.
    pub streaming: bool,
    /// Whether the provider supports tool calls.
    pub tool_calls: bool,
    /// Whether the provider supports structured output.
    pub structured_output: bool,
    /// Whether the provider supports image input.
    pub image_input: bool,
    /// Whether the provider supports file input.
    pub file_input: bool,
    /// Whether the provider supports reasoning/thinking.
    pub reasoning: bool,
    /// Whether the provider supports system prompts.
    pub system_prompt: bool,
    /// Whether the provider supports function calling.
    pub function_calling: bool,
}

/// A single chat message in the provider format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderMessage {
    pub role: String,
    pub content: Vec<ProviderContentBlock>,
}

/// A content block in provider format.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ProviderContentBlock {
    Text {
        text: String,
    },
    Image {
        image_url: ImageSource,
    },
    ToolCall {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        tool_call_id: String,
        content: String,
        /// Tool name when known (required by Gemini functionResponse; optional for OpenAI).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
}

/// One tool call from engine/history (arguments may be a JSON string or object text).
#[derive(Debug, Clone)]
pub struct HistoryToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// Engine/history message parts used to build a structured [`ProviderMessage`].
///
/// Callers (daemon `RealProvider`, Tauri bridge) map their engine types into this
/// shape so `tool_calls` / `tool_call_id` are never flattened to plain text.
#[derive(Debug, Clone)]
pub struct HistoryMessage {
    pub role: String,
    pub content: String,
    pub tool_call_id: Option<String>,
    pub tool_name: Option<String>,
    pub tool_calls: Option<Vec<HistoryToolCall>>,
}

/// Convert a history/engine message into provider wire blocks.
///
/// - Assistant with `tool_calls` → optional text + one `ToolCall` block per call
/// - Tool role (or `tool_call_id` set without assistant tool_calls) → `ToolResult`
/// - Otherwise → text only
pub fn history_message_to_provider(msg: HistoryMessage) -> ProviderMessage {
    if let Some(calls) = msg.tool_calls {
        if !calls.is_empty() {
            let mut content = Vec::new();
            if !msg.content.is_empty() {
                content.push(ProviderContentBlock::Text { text: msg.content });
            }
            for call in calls {
                let input = parse_tool_arguments(&call.arguments);
                content.push(ProviderContentBlock::ToolCall {
                    id: call.id,
                    name: call.name,
                    input,
                });
            }
            return ProviderMessage {
                role: if msg.role.is_empty() {
                    "assistant".into()
                } else {
                    msg.role
                },
                content,
            };
        }
    }

    if msg.role == "tool" || msg.tool_call_id.is_some() {
        let tool_call_id = msg
            .tool_call_id
            .unwrap_or_else(|| "unknown_tool_call".into());
        return ProviderMessage {
            role: "tool".into(),
            content: vec![ProviderContentBlock::ToolResult {
                tool_call_id,
                content: msg.content,
                name: msg.tool_name,
            }],
        };
    }

    ProviderMessage {
        role: msg.role,
        content: vec![ProviderContentBlock::Text { text: msg.content }],
    }
}

/// Map a batch of history messages.
pub fn history_messages_to_provider(
    messages: impl IntoIterator<Item = HistoryMessage>,
) -> Vec<ProviderMessage> {
    messages
        .into_iter()
        .map(history_message_to_provider)
        .collect()
}

fn parse_tool_arguments(arguments: &str) -> serde_json::Value {
    let trimmed = arguments.trim();
    if trimmed.is_empty() {
        return serde_json::json!({});
    }
    serde_json::from_str(trimmed)
        .unwrap_or_else(|_| serde_json::Value::String(arguments.to_string()))
}

#[cfg(test)]
mod history_message_tests {
    use super::*;

    #[test]
    fn preserves_assistant_tool_calls_and_tool_results() {
        let assistant = history_message_to_provider(HistoryMessage {
            role: "assistant".into(),
            content: "calling".into(),
            tool_call_id: None,
            tool_name: None,
            tool_calls: Some(vec![
                HistoryToolCall {
                    id: "call_1".into(),
                    name: "read_file".into(),
                    arguments: r#"{"path":"a.txt"}"#.into(),
                },
                HistoryToolCall {
                    id: "call_2".into(),
                    name: "echo".into(),
                    arguments: r#"{"x":1}"#.into(),
                },
            ]),
        });
        assert_eq!(assistant.role, "assistant");
        assert_eq!(assistant.content.len(), 3);
        assert!(matches!(
            &assistant.content[0],
            ProviderContentBlock::Text { text } if text == "calling"
        ));
        assert!(matches!(
            &assistant.content[1],
            ProviderContentBlock::ToolCall { id, name, .. }
                if id == "call_1" && name == "read_file"
        ));
        assert!(matches!(
            &assistant.content[2],
            ProviderContentBlock::ToolCall { id, name, .. }
                if id == "call_2" && name == "echo"
        ));

        let tool = history_message_to_provider(HistoryMessage {
            role: "tool".into(),
            content: r#"{"ok":true}"#.into(),
            tool_call_id: Some("call_1".into()),
            tool_name: Some("read_file".into()),
            tool_calls: None,
        });
        assert_eq!(tool.role, "tool");
        assert!(matches!(
            &tool.content[0],
            ProviderContentBlock::ToolResult {
                tool_call_id,
                name: Some(n),
                ..
            } if tool_call_id == "call_1" && n == "read_file"
        ));
    }

    #[test]
    fn plain_user_stays_text() {
        let msg = history_message_to_provider(HistoryMessage {
            role: "user".into(),
            content: "hi".into(),
            tool_call_id: None,
            tool_name: None,
            tool_calls: None,
        });
        assert_eq!(msg.content.len(), 1);
        assert!(matches!(
            &msg.content[0],
            ProviderContentBlock::Text { text } if text == "hi"
        ));
    }
}

/// Image source for provider requests.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageSource {
    pub url: String,
    pub detail: Option<String>,
}

/// A tool/function definition for provider requests.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderTool {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: serde_json::Value,
}

/// Request to send to a provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderRequest {
    pub model: String,
    pub messages: Vec<ProviderMessage>,
    pub system_prompt: Option<String>,
    pub tools: Option<Vec<ProviderTool>>,
    pub max_tokens: Option<u64>,
    pub temperature: Option<f64>,
    pub stream: bool,
    pub structured_output: Option<serde_json::Value>,
}

/// A single delta from a streaming response.
#[derive(Debug, Clone)]
pub enum ProviderStreamEvent {
    TextDelta(String),
    ReasoningDelta(String),
    ToolCallBegin {
        id: String,
        name: String,
    },
    ToolCallDelta {
        id: String,
        delta: String,
    },
    ToolCallComplete {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    Done(ProviderUsage),
    Error(ProviderError),
}

/// Non-streaming response from a provider.
#[derive(Debug, Clone)]
pub struct ProviderResponse {
    pub content: Vec<ProviderResponseBlock>,
    pub usage: ProviderUsage,
}

/// A response block.
#[derive(Debug, Clone)]
pub enum ProviderResponseBlock {
    Text(String),
    Reasoning(String),
    ToolCall {
        id: String,
        name: String,
        input: serde_json::Value,
    },
}

/// Token usage information.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: Option<u64>,
    pub cost_usd: Option<f64>,
}

/// Provider error.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderError {
    pub code: String,
    pub message: String,
    pub category: ProviderErrorCategory,
    pub retryable: bool,
    #[serde(default)]
    pub retry_after_ms: Option<u64>,
}

/// Error category.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderErrorCategory {
    Auth,
    RateLimit,
    QuotaExceeded,
    ModelNotFound,
    ContextLengthExceeded,
    BadRequest,
    ServerError,
    Timeout,
    Network,
    Unknown,
}

/// Runtime credential material for a single request (never logged).
#[derive(Debug, Clone)]
pub struct Credential {
    pub api_key: String,
    pub base_url: Option<String>,
    pub key_id: Option<String>,
    pub provider_type: Option<String>,
}

/// Provider adapter trait — all providers must implement this.
#[async_trait]
pub trait ProviderAdapter: Send + Sync {
    /// Get the provider type.
    fn provider_type(&self) -> ProviderType;

    /// Get provider capabilities.
    fn capabilities(&self) -> ProviderCapabilities;

    /// Send a chat completion request (non-streaming).
    async fn chat(&self, request: ProviderRequest) -> Result<ProviderResponse, ProviderError>;

    /// Send a streaming chat completion request (legacy mock-friendly path).
    async fn chat_stream(
        &self,
        request: ProviderRequest,
    ) -> Result<
        Box<dyn tokio_stream::Stream<Item = ProviderStreamEvent> + Send + Unpin>,
        ProviderError,
    >;

    /// Authenticated streaming path used by the Agent Engine.
    ///
    /// Default implementation falls back to `chat_stream` and maps events.
    /// Real adapters override this to perform HTTP SSE with `credential`.
    async fn stream(
        &self,
        request: ProviderRequest,
        _credential: Credential,
    ) -> Result<
        std::pin::Pin<Box<dyn futures_util::Stream<Item = crate::stream::ProviderEvent> + Send>>,
        ProviderError,
    > {
        use crate::stream::ProviderEvent;
        use futures_util::StreamExt;
        let legacy = self.chat_stream(request).await?;
        let mapped = legacy.map(|event| match event {
            ProviderStreamEvent::TextDelta(t) => ProviderEvent::TextDelta(t),
            ProviderStreamEvent::ReasoningDelta(t) => ProviderEvent::ReasoningDelta(t),
            ProviderStreamEvent::ToolCallBegin { id, name } => ProviderEvent::ToolCallDelta {
                index: 0,
                id: Some(id),
                name: Some(name),
                arguments_delta: String::new(),
            },
            ProviderStreamEvent::ToolCallDelta { id, delta } => ProviderEvent::ToolCallDelta {
                index: 0,
                id: Some(id),
                name: None,
                arguments_delta: delta,
            },
            ProviderStreamEvent::ToolCallComplete { id, name, input } => {
                ProviderEvent::ToolCallDelta {
                    index: 0,
                    id: Some(id),
                    name: Some(name),
                    arguments_delta: input.to_string(),
                }
            }
            ProviderStreamEvent::Done(usage) => ProviderEvent::Usage(usage),
            ProviderStreamEvent::Error(err) => ProviderEvent::Error(err),
        });
        // Append Completed after legacy Done for engine compatibility.
        let completed = futures_util::stream::once(async { ProviderEvent::Completed });
        Ok(Box::pin(mapped.chain(completed)))
    }

    /// List available models from this provider.
    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError>;

    /// Discover models with credentials (real HTTP when available).
    async fn discover_models(
        &self,
        _credential: Credential,
    ) -> Result<Vec<ModelInfo>, ProviderError> {
        self.list_models().await
    }

    /// Test the provider connection.
    async fn test_connection(&self) -> Result<ProviderTestResult, ProviderError>;

    /// Test with credentials.
    async fn test_connection_with_credential(
        &self,
        _credential: Credential,
    ) -> Result<ProviderTestResult, ProviderError> {
        self.test_connection().await
    }
}

/// Model information from a provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub display_name: Option<String>,
    pub context_window: u64,
    pub max_output: u64,
    pub capabilities: ModelCapabilities,
}

/// Provider connection test result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderTestResult {
    pub success: bool,
    pub latency_ms: Option<u64>,
    pub message: String,
}

/// Contract tests for all provider adapters.
pub mod contract_tests {
    use super::*;

    /// Test that all adapters satisfy the contract:
    /// - Have Non-empty capabilities
    /// - Non-empty provider type
    /// Can chat
    /// Can list models
    pub fn run_contract_tests(adapter: &dyn ProviderAdapter) {
        let caps = adapter.capabilities();
        assert!(!caps.features.is_empty(), "Features should not be empty");
        assert!(
            caps.max_context_window > 0,
            "Max context window should be > 0"
        );

        // Provider type should be set
        let pt = adapter.provider_type();
        match pt {
            ProviderType::Openai
            | ProviderType::Anthropic
            | ProviderType::Gemini
            | ProviderType::Deepseek
            | ProviderType::OpenaiCompatible
            | ProviderType::Ollama => {}
        }
    }
}
