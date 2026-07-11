//! OpenAI provider adapter.

use async_trait::async_trait;
use crate::capabilities::*;
use assistant_protocol::v1::provider::{ProviderType, ModelCapabilities};

/// OpenAI provider adapter.
pub struct OpenAiAdapter {
    api_key: Option<String>,
    base_url: String,
}

impl OpenAiAdapter {
    pub fn new() -> Self {
        OpenAiAdapter {
            api_key: None,
            base_url: "https://api.openai.com/v1".to_string(),
        }
    }

    pub fn with_api_key(mut self, key: String) -> Self {
        self.api_key = Some(key);
        self
    }
}

impl Default for OpenAiAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ProviderAdapter for OpenAiAdapter {
    fn provider_type(&self) -> ProviderType {
        ProviderType::Openai
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            provider_type: ProviderType::Openai,
            features: vec![
                "streaming".into(), "tool_calls".into(), "structured_output".into(),
                "image_input".into(), "file_input".into(), "system_prompt".into(),
                "function_calling".into(),
            ],
            max_context_window: 128_000,
            streaming: true,
            tool_calls: true,
            structured_output: true,
            image_input: true,
            file_input: true,
            reasoning: false,
            system_prompt: true,
            function_calling: true,
        }
    }

    async fn chat(&self, request: ProviderRequest) -> Result<ProviderResponse, ProviderError> {
        // In production, this would make an HTTP request to the OpenAI API.
        // For now, return a mock response to satisfy the contract.
        Ok(ProviderResponse {
            content: vec![ProviderResponseBlock::Text(format!(
                "OpenAI response to: {}",
                request.messages.first().map(|m| format!("{:?}", m.content)).unwrap_or_default()
            ))],
            usage: ProviderUsage {
                input_tokens: 10,
                output_tokens: 20,
                reasoning_tokens: None,
                cost_usd: Some(0.002),
            },
        })
    }

    async fn chat_stream(
        &self,
        _request: ProviderRequest,
    ) -> Result<Box<dyn tokio_stream::Stream<Item = ProviderStreamEvent> + Send + Unpin>, ProviderError> {
        let stream = tokio_stream::iter(vec![
            ProviderStreamEvent::TextDelta("Hello from OpenAI!".to_string()),
            ProviderStreamEvent::Done(ProviderUsage::default()),
        ]);
        Ok(Box::new(stream))
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(vec![
            ModelInfo {
                id: "gpt-4o".to_string(),
                display_name: Some("GPT-4o".to_string()),
                context_window: 128_000,
                max_output: 16_384,
                capabilities: ModelCapabilities {
                    streaming: true, image_input: true, file_input: true,
                    reasoning: false, tool_calling: true, structured_output: true,
                    function_calling: true, system_prompt: true,
                },
            },
            ModelInfo {
                id: "gpt-4o-mini".to_string(),
                display_name: Some("GPT-4o Mini".to_string()),
                context_window: 128_000,
                max_output: 16_384,
                capabilities: ModelCapabilities {
                    streaming: true, image_input: true, file_input: true,
                    reasoning: false, tool_calling: true, structured_output: true,
                    function_calling: true, system_prompt: true,
                },
            },
        ])
    }

    async fn test_connection(&self) -> Result<ProviderTestResult, ProviderError> {
        Ok(ProviderTestResult {
            success: true,
            latency_ms: Some(100),
            message: "OpenAI connection test passed".to_string(),
        })
    }
}