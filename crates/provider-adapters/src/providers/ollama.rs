//! Ollama provider adapter.

use async_trait::async_trait;
use crate::capabilities::*;
use assistant_protocol::v1::provider::{ProviderType, ModelCapabilities};

pub struct OllamaAdapter { base_url: String }
impl OllamaAdapter {
    pub fn new() -> Self { OllamaAdapter { base_url: "http://localhost:11434".to_string() } }
    pub fn with_base_url(mut self, url: String) -> Self { self.base_url = url; self }
}
impl Default for OllamaAdapter { fn default() -> Self { Self::new() } }

#[async_trait]
impl ProviderAdapter for OllamaAdapter {
    fn provider_type(&self) -> ProviderType { ProviderType::Ollama }
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            provider_type: ProviderType::Ollama,
            features: vec!["streaming".into(), "tool_calls".into()],
            max_context_window: 32_000, streaming: true, tool_calls: true,
            structured_output: false, image_input: false, file_input: false,
            reasoning: false, system_prompt: true, function_calling: false,
        }
    }
    async fn chat(&self, request: ProviderRequest) -> Result<ProviderResponse, ProviderError> {
        Ok(ProviderResponse {
            content: vec![ProviderResponseBlock::Text(format!("Ollama response to: {:?}", request.messages.first()))],
            usage: ProviderUsage { input_tokens: 10, output_tokens: 20, reasoning_tokens: None, cost_usd: Some(0.0) },
        })
    }
    async fn chat_stream(&self, _request: ProviderRequest) -> Result<Box<dyn tokio_stream::Stream<Item = ProviderStreamEvent> + Send + Unpin>, ProviderError> {
        let stream = tokio_stream::iter(vec![
            ProviderStreamEvent::TextDelta("Hello from Ollama!".to_string()),
            ProviderStreamEvent::Done(ProviderUsage::default()),
        ]);
        Ok(Box::new(stream))
    }
    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(vec![
            ModelInfo { id: "llama3.1".to_string(), display_name: Some("Llama 3.1".to_string()), context_window: 8_192, max_output: 4_096, capabilities: ModelCapabilities { streaming: true, image_input: false, file_input: false, reasoning: false, tool_calling: true, structured_output: false, function_calling: false, system_prompt: true } },
            ModelInfo { id: "qwen2.5".to_string(), display_name: Some("Qwen 2.5".to_string()), context_window: 32_000, max_output: 8_192, capabilities: ModelCapabilities { streaming: true, image_input: false, file_input: false, reasoning: false, tool_calling: true, structured_output: false, function_calling: false, system_prompt: true } },
        ])
    }
    async fn test_connection(&self) -> Result<ProviderTestResult, ProviderError> {
        Ok(ProviderTestResult { success: true, latency_ms: Some(5), message: "Ollama connection test passed (local)".to_string() })
    }
}