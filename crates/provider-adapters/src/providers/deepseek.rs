//! DeepSeek provider adapter — OpenAI-compatible Chat Completions.

use crate::capabilities::*;
use crate::http_stream::stream_chat_completions;
use crate::stream::ProviderEvent;
use assistant_protocol::v1::provider::{ModelCapabilities, ProviderType};
use async_trait::async_trait;
use reqwest::Client;

pub struct DeepSeekAdapter {
    api_key: Option<String>,
    base_url: String,
    client: Client,
}

impl DeepSeekAdapter {
    pub fn new() -> Self {
        DeepSeekAdapter {
            api_key: None,
            base_url: "https://api.deepseek.com".to_string(),
            client: Client::new(),
        }
    }
    pub fn with_api_key(mut self, key: String) -> Self {
        self.api_key = Some(key);
        self
    }
}
impl Default for DeepSeekAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ProviderAdapter for DeepSeekAdapter {
    fn provider_type(&self) -> ProviderType {
        ProviderType::Deepseek
    }
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            provider_type: ProviderType::Deepseek,
            features: vec![
                "streaming".into(),
                "tool_calls".into(),
                "reasoning".into(),
                "system_prompt".into(),
                // Automatic context caching; usage read back from
                // `prompt_cache_hit_tokens` / `prompt_cache_miss_tokens`.
                "prompt_cache_automatic".into(),
                "tool_choice".into(),
                // NOTE: `parallel_tool_calls` is deliberately absent — DeepSeek
                // does not document a per-request toggle.
            ],
            max_context_window: 64_000,
            streaming: true,
            tool_calls: true,
            structured_output: false,
            image_input: false,
            file_input: false,
            reasoning: true,
            system_prompt: true,
            function_calling: true,
        }
    }
    async fn chat(&self, request: ProviderRequest) -> Result<ProviderResponse, ProviderError> {
        if self.api_key.is_none() {
            return Err(ProviderError {
                code: "missing_key".into(),
                message: "DeepSeek API key required (offline mock removed)".into(),
                category: ProviderErrorCategory::Auth,
                retryable: false,
                retry_after_ms: None,
            });
        }
        let mut text = String::new();
        let mut usage = ProviderUsage::default();
        let stream = self
            .stream(
                request,
                Credential {
                    api_key: self.api_key.clone().unwrap_or_default(),
                    base_url: Some(self.base_url.clone()),
                    proxy_url: None,
                    key_id: None,
                    provider_type: Some("deepseek".into()),
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
    ) -> Result<
        Box<dyn tokio_stream::Stream<Item = ProviderStreamEvent> + Send + Unpin>,
        ProviderError,
    > {
        Err(ProviderError {
            code: "use_stream".into(),
            message: "Use stream(request, credential) for DeepSeek; offline mock removed".into(),
            category: ProviderErrorCategory::Auth,
            retryable: false,
            retry_after_ms: None,
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
        let key = if !credential.api_key.is_empty() {
            credential.api_key
        } else {
            self.api_key.clone().ok_or_else(|| ProviderError {
                code: "missing_key".into(),
                message: "DeepSeek API key required".into(),
                category: ProviderErrorCategory::Auth,
                retryable: false,
                retry_after_ms: None,
            })?
        };
        let base = credential.base_url.unwrap_or_else(|| self.base_url.clone());
        let client = match credential.proxy_url.as_deref() {
            Some(url) if !url.trim().is_empty() => crate::http_client::client(Some(url))?,
            _ => self.client.clone(),
        };
        stream_chat_completions(&client, &base, &key, request).await
    }
    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(vec![
            ModelInfo {
                id: "deepseek-chat".into(),
                display_name: Some("DeepSeek Chat".into()),
                context_window: 64_000,
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
            },
            ModelInfo {
                id: "deepseek-reasoner".into(),
                display_name: Some("DeepSeek Reasoner".into()),
                context_window: 64_000,
                max_output: 8_192,
                capabilities: ModelCapabilities {
                    streaming: true,
                    image_input: false,
                    file_input: false,
                    reasoning: true,
                    tool_calling: false,
                    structured_output: false,
                    function_calling: false,
                    system_prompt: true,
                },
            },
        ])
    }
    async fn test_connection(&self) -> Result<ProviderTestResult, ProviderError> {
        Ok(ProviderTestResult {
            success: self.api_key.is_some(),
            latency_ms: None,
            message: "DeepSeek adapter ready".into(),
        })
    }
}
