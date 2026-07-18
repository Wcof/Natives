//! Ollama provider adapter — local OpenAI-compatible API.

use async_trait::async_trait;
use crate::capabilities::*;
use crate::http_stream::stream_chat_completions;
use crate::stream::ProviderEvent;
use assistant_protocol::v1::provider::{ProviderType, ModelCapabilities};
use reqwest::Client;

pub struct OllamaAdapter {
    base_url: String,
    client: Client,
}

impl OllamaAdapter {
    pub fn new() -> Self {
        OllamaAdapter {
            base_url: "http://127.0.0.1:11434/v1".to_string(),
            client: Client::new(),
        }
    }
    pub fn with_base_url(mut self, url: String) -> Self {
        self.base_url = url;
        self
    }
}
impl Default for OllamaAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ProviderAdapter for OllamaAdapter {
    fn provider_type(&self) -> ProviderType {
        ProviderType::Ollama
    }
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            provider_type: ProviderType::Ollama,
            features: vec!["streaming".into(), "tool_calls".into(), "system_prompt".into()],
            max_context_window: 32_000,
            streaming: true,
            tool_calls: true,
            structured_output: false,
            image_input: false,
            file_input: false,
            reasoning: false,
            system_prompt: true,
            function_calling: true,
        }
    }
    async fn chat(&self, request: ProviderRequest) -> Result<ProviderResponse, ProviderError> {
        // Collect stream() against local daemon — never return offline mock success text.
        let mut text = String::new();
        let mut usage = ProviderUsage::default();
        let stream = self
            .stream(
                request,
                Credential {
                    api_key: "ollama".into(),
                    base_url: Some(self.base_url.clone()),
                    key_id: None,
                    provider_type: Some("ollama".into()),
                },
            )
            .await?;
        use futures_util::StreamExt;
        tokio::pin!(stream);
        while let Some(ev) = stream.next().await {
            match ev {
                ProviderEvent::TextDelta(t) => text.push_str(&t),
                ProviderEvent::Usage(u) => usage = u,
                ProviderEvent::Error(e) => return Err(e),
                _ => {}
            }
        }
        Ok(ProviderResponse {
            content: vec![ProviderResponseBlock::Text(text)],
            usage,
        })
    }
    async fn chat_stream(
        &self,
        _request: ProviderRequest,
    ) -> Result<Box<dyn tokio_stream::Stream<Item = ProviderStreamEvent> + Send + Unpin>, ProviderError>
    {
        Err(ProviderError {
            code: "use_stream".into(),
            message: "Use stream(request, credential) for Ollama; offline mock removed".into(),
            category: ProviderErrorCategory::Auth,
            retryable: false,
        })
    }
    async fn stream(
        &self,
        request: ProviderRequest,
        credential: Credential,
    ) -> Result<
        std::pin::Pin<Box<dyn futures_util::Stream<Item = ProviderEvent> + Send>>,
        ProviderError,
    > {
        // Ollama often ignores the key; use a placeholder.
        let key = if credential.api_key.is_empty() {
            "ollama".into()
        } else {
            credential.api_key
        };
        let base = credential
            .base_url
            .unwrap_or_else(|| self.base_url.clone());
        stream_chat_completions(&self.client, &base, &key, request).await
    }
    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(vec![ModelInfo {
            id: "llama3.2".into(),
            display_name: Some("Llama 3.2".into()),
            context_window: 32_000,
            max_output: 8_192,
            capabilities: ModelCapabilities {
                streaming: true,
                image_input: false,
                file_input: false,
                reasoning: false,
                tool_calling: true,
                structured_output: false,
                function_calling: true,
                system_prompt: true,
            },
        }])
    }
    async fn test_connection(&self) -> Result<ProviderTestResult, ProviderError> {
        let url = format!("{}/models", self.base_url.trim_end_matches('/'));
        match self.client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => Ok(ProviderTestResult {
                success: true,
                latency_ms: Some(0),
                message: "Ollama reachable".into(),
            }),
            Ok(resp) => Ok(ProviderTestResult {
                success: false,
                latency_ms: None,
                message: format!("HTTP {}", resp.status()),
            }),
            Err(err) => Ok(ProviderTestResult {
                success: false,
                latency_ms: None,
                message: err.to_string(),
            }),
        }
    }
}
