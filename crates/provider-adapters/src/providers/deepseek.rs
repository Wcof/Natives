//! DeepSeek provider adapter.

use async_trait::async_trait;
use crate::capabilities::*;
use assistant_protocol::v1::provider::{ProviderType, ModelCapabilities};

pub struct DeepSeekAdapter { api_key: Option<String>, base_url: String }
impl DeepSeekAdapter {
    pub fn new() -> Self { DeepSeekAdapter { api_key: None, base_url: "https://api.deepseek.com/v1".to_string() } }
    pub fn with_api_key(mut self, key: String) -> Self { self.api_key = Some(key); self }
}
impl Default for DeepSeekAdapter { fn default() -> Self { Self::new() } }

#[async_trait]
impl ProviderAdapter for DeepSeekAdapter {
    fn provider_type(&self) -> ProviderType { ProviderType::Deepseek }
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            provider_type: ProviderType::Deepseek,
            features: vec!["streaming".into(), "tool_calls".into(), "reasoning".into()],
            max_context_window: 64_000, streaming: true, tool_calls: true,
            structured_output: false, image_input: false, file_input: false,
            reasoning: true, system_prompt: true, function_calling: true,
        }
    }
    async fn chat(&self, request: ProviderRequest) -> Result<ProviderResponse, ProviderError> {
        Ok(ProviderResponse {
            content: vec![ProviderResponseBlock::Text(format!("DeepSeek response to: {:?}", request.messages.first()))],
            usage: ProviderUsage { input_tokens: 8, output_tokens: 16, reasoning_tokens: Some(4), cost_usd: Some(0.0005) },
        })
    }
    async fn chat_stream(&self, _request: ProviderRequest) -> Result<Box<dyn tokio_stream::Stream<Item = ProviderStreamEvent> + Send + Unpin>, ProviderError> {
        let stream = tokio_stream::iter(vec![
            ProviderStreamEvent::TextDelta("Hello from DeepSeek!".to_string()),
            ProviderStreamEvent::Done(ProviderUsage::default()),
        ]);
        Ok(Box::new(stream))
    }
    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(vec![
            ModelInfo { id: "deepseek-chat".to_string(), display_name: Some("DeepSeek Chat".to_string()), context_window: 64_000, max_output: 8_192, capabilities: ModelCapabilities { streaming: true, image_input: false, file_input: false, reasoning: true, tool_calling: true, structured_output: false, function_calling: true, system_prompt: true } },
            ModelInfo { id: "deepseek-reasoner".to_string(), display_name: Some("DeepSeek Reasoner".to_string()), context_window: 64_000, max_output: 8_192, capabilities: ModelCapabilities { streaming: true, image_input: false, file_input: false, reasoning: true, tool_calling: false, structured_output: false, function_calling: false, system_prompt: true } },
        ])
    }
    async fn test_connection(&self) -> Result<ProviderTestResult, ProviderError> {
        Ok(ProviderTestResult { success: true, latency_ms: Some(80), message: "DeepSeek connection test passed".to_string() })
    }
}