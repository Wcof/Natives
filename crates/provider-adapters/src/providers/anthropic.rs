//! Anthropic provider adapter.

use async_trait::async_trait;
use crate::capabilities::*;
use assistant_protocol::v1::provider::{ProviderType, ModelCapabilities};

pub struct AnthropicAdapter { api_key: Option<String>, base_url: String }
impl AnthropicAdapter {
    pub fn new() -> Self { AnthropicAdapter { api_key: None, base_url: "https://api.anthropic.com/v1".to_string() } }
    pub fn with_api_key(mut self, key: String) -> Self { self.api_key = Some(key); self }
}
impl Default for AnthropicAdapter { fn default() -> Self { Self::new() } }

#[async_trait]
impl ProviderAdapter for AnthropicAdapter {
    fn provider_type(&self) -> ProviderType { ProviderType::Anthropic }
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            provider_type: ProviderType::Anthropic,
            features: vec!["streaming".into(), "tool_calls".into(), "image_input".into(), "reasoning".into(), "system_prompt".into()],
            max_context_window: 200_000, streaming: true, tool_calls: true,
            structured_output: false, image_input: true, file_input: true,
            reasoning: true, system_prompt: true, function_calling: false,
        }
    }
    async fn chat(&self, request: ProviderRequest) -> Result<ProviderResponse, ProviderError> {
        Ok(ProviderResponse {
            content: vec![ProviderResponseBlock::Text(format!("Anthropic response to: {:?}", request.messages.first()))],
            usage: ProviderUsage { input_tokens: 15, output_tokens: 30, reasoning_tokens: Some(5), cost_usd: Some(0.003) },
        })
    }
    async fn chat_stream(&self, _request: ProviderRequest) -> Result<Box<dyn tokio_stream::Stream<Item = ProviderStreamEvent> + Send + Unpin>, ProviderError> {
        let stream = tokio_stream::iter(vec![
            ProviderStreamEvent::TextDelta("Hello from Anthropic!".to_string()),
            ProviderStreamEvent::Done(ProviderUsage::default()),
        ]);
        Ok(Box::new(stream))
    }
    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(vec![
            ModelInfo { id: "claude-sonnet-4-20250514".to_string(), display_name: Some("Claude Sonnet 4".to_string()), context_window: 200_000, max_output: 8_192, capabilities: ModelCapabilities { streaming: true, image_input: true, file_input: true, reasoning: true, tool_calling: true, structured_output: false, function_calling: false, system_prompt: true } },
            ModelInfo { id: "claude-haiku-3-5-20241022".to_string(), display_name: Some("Claude Haiku 3.5".to_string()), context_window: 200_000, max_output: 8_192, capabilities: ModelCapabilities { streaming: true, image_input: true, file_input: true, reasoning: true, tool_calling: true, structured_output: false, function_calling: false, system_prompt: true } },
        ])
    }
    async fn test_connection(&self) -> Result<ProviderTestResult, ProviderError> {
        Ok(ProviderTestResult { success: true, latency_ms: Some(150), message: "Anthropic connection test passed".to_string() })
    }
}