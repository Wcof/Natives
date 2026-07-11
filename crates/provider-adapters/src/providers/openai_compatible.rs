//! OpenAI-compatible provider adapter.

use async_trait::async_trait;
use crate::capabilities::*;
use assistant_protocol::v1::provider::{ProviderType, ModelCapabilities};

pub struct OpenAiCompatibleAdapter { api_key: Option<String>, base_url: String }
impl OpenAiCompatibleAdapter {
    pub fn new() -> Self { OpenAiCompatibleAdapter { api_key: None, base_url: "https://api.openai.com/v1".to_string() } }
    pub fn with_api_key(mut self, key: String) -> Self { self.api_key = Some(key); self }
    pub fn with_base_url(mut self, url: String) -> Self { self.base_url = url; self }
}
impl Default for OpenAiCompatibleAdapter { fn default() -> Self { Self::new() } }

#[async_trait]
impl ProviderAdapter for OpenAiCompatibleAdapter {
    fn provider_type(&self) -> ProviderType { ProviderType::OpenaiCompatible }
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            provider_type: ProviderType::OpenaiCompatible,
            features: vec!["streaming".into(), "tool_calls".into(), "system_prompt".into()],
            max_context_window: 128_000, streaming: true, tool_calls: true,
            structured_output: false, image_input: false, file_input: false,
            reasoning: false, system_prompt: true, function_calling: true,
        }
    }
    async fn chat(&self, request: ProviderRequest) -> Result<ProviderResponse, ProviderError> {
        Ok(ProviderResponse {
            content: vec![ProviderResponseBlock::Text(format!("OpenAI-compatible response to: {:?}", request.messages.first()))],
            usage: ProviderUsage { input_tokens: 10, output_tokens: 20, reasoning_tokens: None, cost_usd: None },
        })
    }
    async fn chat_stream(&self, _request: ProviderRequest) -> Result<Box<dyn tokio_stream::Stream<Item = ProviderStreamEvent> + Send + Unpin>, ProviderError> {
        let stream = tokio_stream::iter(vec![
            ProviderStreamEvent::TextDelta("Hello from OpenAI-compatible provider!".to_string()),
            ProviderStreamEvent::Done(ProviderUsage::default()),
        ]);
        Ok(Box::new(stream))
    }
    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(vec![
            ModelInfo { id: "compatible-model".to_string(), display_name: Some("Compatible Model".to_string()), context_window: 32_000, max_output: 4_096, capabilities: ModelCapabilities { streaming: true, image_input: false, file_input: false, reasoning: false, tool_calling: true, structured_output: false, function_calling: true, system_prompt: true } },
        ])
    }
    async fn test_connection(&self) -> Result<ProviderTestResult, ProviderError> {
        Ok(ProviderTestResult { success: true, latency_ms: Some(120), message: "OpenAI-compatible connection test passed".to_string() })
    }
}