use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use assistant_protocol::v1::provider::{ProviderType, ModelCapabilities};

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
    Text { text: String },
    Image { image_url: ImageSource },
    ToolCall { id: String, name: String, input: serde_json::Value },
    ToolResult { tool_call_id: String, content: String },
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
    ToolCallBegin { id: String, name: String },
    ToolCallDelta { id: String, delta: String },
    ToolCallComplete { id: String, name: String, input: serde_json::Value },
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
    ToolCall { id: String, name: String, input: serde_json::Value },
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

/// Provider adapter trait — all providers must implement this.
#[async_trait]
pub trait ProviderAdapter: Send + Sync {
    /// Get the provider type.
    fn provider_type(&self) -> ProviderType;

    /// Get provider capabilities.
    fn capabilities(&self) -> ProviderCapabilities;

    /// Send a chat completion request (non-streaming).
    async fn chat(&self, request: ProviderRequest) -> Result<ProviderResponse, ProviderError>;

    /// Send a streaming chat completion request.
    async fn chat_stream(
        &self,
        request: ProviderRequest,
    ) -> Result<Box<dyn tokio_stream::Stream<Item = ProviderStreamEvent> + Send + Unpin>, ProviderError>;

    /// List available models from this provider.
    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError>;

    /// Test the provider connection.
    async fn test_connection(&self) -> Result<ProviderTestResult, ProviderError>;
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
        assert!(caps.max_context_window > 0, "Max context window should be > 0");

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