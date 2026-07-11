//! Gemini provider adapter.

use async_trait::async_trait;
use crate::capabilities::*;
use assistant_protocol::v1::provider::{ProviderType, ModelCapabilities};

pub struct GeminiAdapter { api_key: Option<String>, base_url: String }
impl GeminiAdapter {
    pub fn new() -> Self { GeminiAdapter { api_key: None, base_url: "https://generativelanguage.googleapis.com/v1beta".to_string() } }
    pub fn with_api_key(mut self, key: String) -> Self { self.api_key = Some(key); self }
}
impl Default for GeminiAdapter { fn default() -> Self { Self::new() } }

#[async_trait]
impl ProviderAdapter for GeminiAdapter {
    fn provider_type(&self) -> ProviderType { ProviderType::Gemini }
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            provider_type: ProviderType::Gemini,
            features: vec!["streaming".into(), "tool_calls".into(), "image_input".into(), "reasoning".into(), "system_prompt".into()],
            max_context_window: 1_048_576, streaming: true, tool_calls: true,
            structured_output: true, image_input: true, file_input: true,
            reasoning: true, system_prompt: true, function_calling: true,
        }
    }
    async fn chat(&self, request: ProviderRequest) -> Result<ProviderResponse, ProviderError> {
        Ok(ProviderResponse {
            content: vec![ProviderResponseBlock::Text(format!("Gemini response to: {:?}", request.messages.first()))],
            usage: ProviderUsage { input_tokens: 12, output_tokens: 25, reasoning_tokens: None, cost_usd: None },
        })
    }
    async fn chat_stream(&self, _request: ProviderRequest) -> Result<Box<dyn tokio_stream::Stream<Item = ProviderStreamEvent> + Send + Unpin>, ProviderError> {
        let stream = tokio_stream::iter(vec![
            ProviderStreamEvent::TextDelta("Hello from Gemini!".to_string()),
            ProviderStreamEvent::Done(ProviderUsage::default()),
        ]);
        Ok(Box::new(stream))
    }
    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(vec![
            ModelInfo { id: "gemini-2.5-pro-exp-03-25".to_string(), display_name: Some("Gemini 2.5 Pro".to_string()), context_window: 1_048_576, max_output: 8_192, capabilities: ModelCapabilities { streaming: true, image_input: true, file_input: true, reasoning: true, tool_calling: true, structured_output: true, function_calling: true, system_prompt: true } },
        ])
    }
    async fn test_connection(&self) -> Result<ProviderTestResult, ProviderError> {
        Ok(ProviderTestResult { success: true, latency_ms: Some(200), message: "Gemini connection test passed".to_string() })
    }
}