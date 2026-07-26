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
///
/// # Prompt-cache token contract
///
/// Providers disagree about whether cached prompt tokens are counted inside
/// their prompt-token field: Anthropic **excludes** them from `input_tokens`,
/// while OpenAI, DeepSeek and Gemini **include** them. Rather than leak that to
/// every caller, the stream parsers normalise on the Anthropic convention:
///
/// - `input_tokens` — prompt tokens that were **not** served from cache.
/// - `cache_read_tokens` — prompt tokens served from cache (billed at a
///   discount by every provider that reports them).
/// - `cache_creation_tokens` — prompt tokens **written** to the cache this
///   request (billed at a premium). Only Anthropic reports this separately;
///   automatic-prefix providers fold cache writes into `input_tokens`.
///
/// Total prompt size is therefore always
/// `input_tokens + cache_read_tokens + cache_creation_tokens`
/// (see [`ProviderUsage::total_prompt_tokens`]).
///
/// `None` means "the provider did not report this", which is distinct from
/// `Some(0)` ("reported, and it was zero"). Callers that persist cache metrics
/// must preserve that distinction.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: Option<u64>,
    /// Prompt tokens written to the provider's prompt cache this request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_creation_tokens: Option<u64>,
    /// Prompt tokens served from the provider's prompt cache this request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<u64>,
    pub cost_usd: Option<f64>,
}

impl ProviderUsage {
    /// Full prompt size including cached and cache-written tokens.
    pub fn total_prompt_tokens(&self) -> u64 {
        self.input_tokens
            .saturating_add(self.cache_creation_tokens.unwrap_or(0))
            .saturating_add(self.cache_read_tokens.unwrap_or(0))
    }

    /// Whether the provider reported any prompt-cache activity at all.
    pub fn reported_cache(&self) -> bool {
        self.cache_creation_tokens.is_some() || self.cache_read_tokens.is_some()
    }

    /// Fold a later usage report into an earlier one.
    ///
    /// Anthropic sends the full prompt breakdown once on `message_start` and
    /// then a terminal `message_delta` that carries only the fields it knows;
    /// naively replacing the accumulator there loses the input and cache
    /// counts. Non-zero / `Some` values from `next` win, everything else is
    /// carried forward.
    pub fn merge_from(&mut self, next: &ProviderUsage) {
        if next.input_tokens > 0 {
            self.input_tokens = next.input_tokens;
        }
        if next.output_tokens > 0 {
            self.output_tokens = next.output_tokens;
        }
        if next.reasoning_tokens.is_some() {
            self.reasoning_tokens = next.reasoning_tokens;
        }
        if next.cache_creation_tokens.is_some() {
            self.cache_creation_tokens = next.cache_creation_tokens;
        }
        if next.cache_read_tokens.is_some() {
            self.cache_read_tokens = next.cache_read_tokens;
        }
        if next.cost_usd.is_some() {
            self.cost_usd = next.cost_usd;
        }
    }
}

/// Which tool (if any) the model is forced to call.
///
/// Wire encoding is provider-specific; see
/// [`ToolChoice::to_anthropic`] / [`ToolChoice::to_openai`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum ToolChoice {
    /// Model decides freely (provider default).
    Auto,
    /// Model must not call a tool.
    None,
    /// Model must call at least one tool, its choice which.
    Required,
    /// Model must call this exact tool.
    Tool { name: String },
}

impl ToolChoice {
    /// Anthropic Messages API `tool_choice` object.
    ///
    /// `disable_parallel_tool_use` rides on the same object for Anthropic, so
    /// it is passed in here rather than emitted as a sibling field.
    pub fn to_anthropic(&self, parallel_tool_calls: Option<bool>) -> serde_json::Value {
        let mut value = match self {
            ToolChoice::Auto => serde_json::json!({ "type": "auto" }),
            ToolChoice::None => serde_json::json!({ "type": "none" }),
            ToolChoice::Required => serde_json::json!({ "type": "any" }),
            ToolChoice::Tool { name } => serde_json::json!({ "type": "tool", "name": name }),
        };
        if let Some(false) = parallel_tool_calls {
            value["disable_parallel_tool_use"] = serde_json::json!(true);
        }
        value
    }

    /// OpenAI chat-completions / Responses `tool_choice` value.
    pub fn to_openai(&self) -> serde_json::Value {
        match self {
            ToolChoice::Auto => serde_json::json!("auto"),
            ToolChoice::None => serde_json::json!("none"),
            ToolChoice::Required => serde_json::json!("required"),
            ToolChoice::Tool { name } => serde_json::json!({
                "type": "function",
                "function": { "name": name },
            }),
        }
    }

    /// Gemini `toolConfig.functionCallingConfig` object.
    pub fn to_gemini(&self) -> serde_json::Value {
        match self {
            ToolChoice::Auto => serde_json::json!({ "mode": "AUTO" }),
            ToolChoice::None => serde_json::json!({ "mode": "NONE" }),
            ToolChoice::Required => serde_json::json!({ "mode": "ANY" }),
            ToolChoice::Tool { name } => serde_json::json!({
                "mode": "ANY",
                "allowedFunctionNames": [name],
            }),
        }
    }
}

/// Coarse reasoning depth, mapped onto whatever knob the model exposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningEffort {
    Low,
    Medium,
    High,
}

impl ReasoningEffort {
    /// OpenAI `reasoning_effort` string.
    pub fn as_openai_str(self) -> &'static str {
        match self {
            ReasoningEffort::Low => "low",
            ReasoningEffort::Medium => "medium",
            ReasoningEffort::High => "high",
        }
    }

    /// Default thinking budget in tokens for providers that take a number
    /// instead of a level. Callers may override via
    /// [`ReasoningRequest::budget_tokens`].
    pub fn default_budget_tokens(self) -> u64 {
        match self {
            ReasoningEffort::Low => 4_096,
            ReasoningEffort::Medium => 16_384,
            ReasoningEffort::High => 32_768,
        }
    }
}

/// Caller-requested reasoning configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReasoningRequest {
    pub effort: ReasoningEffort,
    /// Explicit thinking budget. Ignored by models whose only knob is a level
    /// (`ReasoningControl::OpenAiEffort`, `ReasoningControl::AnthropicAdaptive`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_tokens: Option<u64>,
}

impl ReasoningRequest {
    pub fn new(effort: ReasoningEffort) -> Self {
        ReasoningRequest {
            effort,
            budget_tokens: None,
        }
    }

    /// Budget to send, honouring an explicit value over the effort default.
    pub fn budget(&self) -> u64 {
        self.budget_tokens
            .unwrap_or_else(|| self.effort.default_budget_tokens())
    }
}

/// Request-side controls that are orthogonal to the message payload.
///
/// [`Default`] is exactly today's behaviour: no forced tool, provider-default
/// parallelism, no reasoning parameter, and prompt caching left to the
/// per-model default (enabled wherever the model supports explicit
/// breakpoints).
///
/// This rides alongside [`ProviderRequest`] rather than inside it because
/// `ProviderRequest` is built with struct literals in crates outside this one;
/// see `build_messages_body_with_controls` and
/// `build_chat_completions_body_with_controls` for the seam.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestControls {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    /// `Some(false)` forces one tool call per assistant turn. `None` leaves the
    /// provider default (parallel calls allowed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parallel_tool_calls: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ReasoningRequest>,
    /// `None` = per-model default. `Some(false)` disables prompt-cache
    /// breakpoints for this request (incident escape hatch).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_cache: Option<bool>,
}

impl RequestControls {
    /// Whether explicit prompt-cache breakpoints should be emitted for a model.
    pub fn prompt_cache_enabled(&self, profile: &crate::model_profile::ModelProfile) -> bool {
        profile.wants_explicit_cache_breakpoints() && self.prompt_cache.unwrap_or(true)
    }
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
    /// Per-request outbound proxy. Kept memory-only alongside the credential.
    pub proxy_url: Option<String>,
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
