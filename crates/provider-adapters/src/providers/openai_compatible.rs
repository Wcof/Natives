//! OpenAI-compatible provider adapter — real HTTP streaming.

use crate::capabilities::*;
use crate::http_stream::{chat_completions, stream_chat_completions};
use crate::stream::ProviderEvent;
use assistant_protocol::v1::provider::{ModelCapabilities, ProviderType};
use async_trait::async_trait;
use reqwest::Client;

pub struct OpenAiCompatibleAdapter {
    api_key: Option<String>,
    base_url: String,
    client: Client,
}

impl OpenAiCompatibleAdapter {
    pub fn new() -> Self {
        OpenAiCompatibleAdapter {
            api_key: None,
            base_url: "https://api.openai.com/v1".to_string(),
            client: Client::new(),
        }
    }
    pub fn with_api_key(mut self, key: String) -> Self {
        self.api_key = Some(key);
        self
    }
    pub fn with_base_url(mut self, url: String) -> Self {
        self.base_url = url;
        self
    }
}
impl Default for OpenAiCompatibleAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ProviderAdapter for OpenAiCompatibleAdapter {
    fn provider_type(&self) -> ProviderType {
        ProviderType::OpenaiCompatible
    }
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            provider_type: ProviderType::OpenaiCompatible,
            features: vec![
                "streaming".into(),
                "tool_calls".into(),
                "function_calling".into(),
                "system_prompt".into(),
            ],
            max_context_window: 128_000,
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
        let key = self.api_key.clone().ok_or_else(|| ProviderError {
            code: "missing_key".into(),
            message: "API key required".into(),
            category: ProviderErrorCategory::Auth,
            retryable: false,
            retry_after_ms: None,
        })?;
        let (content, tools, usage) =
            chat_completions(&self.client, &self.base_url, &key, request).await?;
        let mut blocks = vec![ProviderResponseBlock::Text(content)];
        if let Some(tcs) = tools {
            for (id, name, args) in tcs {
                let input = serde_json::from_str(&args).map_err(|error| ProviderError {
                    code: "invalid_tool_arguments".into(),
                    message: format!("tool call {id} arguments are not valid JSON: {error}"),
                    category: ProviderErrorCategory::BadRequest,
                    retryable: false,
                    retry_after_ms: None,
                })?;
                blocks.push(ProviderResponseBlock::ToolCall { id, name, input });
            }
        }
        Ok(ProviderResponse {
            content: blocks,
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
            message: "Use stream(request, credential) for OpenAI-compatible; offline mock removed"
                .into(),
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
                message: "API key required".into(),
                category: ProviderErrorCategory::Auth,
                retryable: false,
                retry_after_ms: None,
            })?
        };
        let base = credential.base_url.unwrap_or_else(|| self.base_url.clone());
        let client = crate::http_client::client(credential.proxy_url.as_deref())?;
        stream_chat_completions(&client, &base, &key, request).await
    }
    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(vec![ModelInfo {
            id: "compatible-model".into(),
            display_name: Some("Compatible Model".into()),
            context_window: 32_000,
            max_output: 4_096,
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
        Ok(ProviderTestResult {
            success: self.api_key.is_some(),
            latency_ms: Some(0),
            message: if self.api_key.is_some() {
                "Key present".into()
            } else {
                "No key".into()
            },
        })
    }
}
