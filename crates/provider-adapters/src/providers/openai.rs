//! OpenAI provider adapter — real HTTP streaming via Chat Completions SSE.

use crate::capabilities::*;
use crate::http_stream::{chat_completions, stream_chat_completions, stream_responses};
use crate::stream::ProviderEvent;
use assistant_protocol::v1::provider::{ModelCapabilities, ProviderType};
use async_trait::async_trait;
use reqwest::Client;
use std::time::Instant;

/// OpenAI provider adapter.
pub struct OpenAiAdapter {
    api_key: Option<String>,
    base_url: String,
    client: Client,
}

impl OpenAiAdapter {
    pub fn new() -> Self {
        OpenAiAdapter {
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

    fn resolve_credential(
        &self,
        credential: &Credential,
    ) -> Result<(String, String), ProviderError> {
        let key = if !credential.api_key.is_empty() {
            credential.api_key.clone()
        } else {
            self.api_key.clone().ok_or_else(|| ProviderError {
                code: "missing_key".into(),
                message: "OpenAI API key is required".into(),
                category: ProviderErrorCategory::Auth,
                retryable: false,
                retry_after_ms: None,
            })?
        };
        let base = credential
            .base_url
            .clone()
            .unwrap_or_else(|| self.base_url.clone());
        Ok((key, base))
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
                "streaming".into(),
                "tool_calls".into(),
                "structured_output".into(),
                "image_input".into(),
                "file_input".into(),
                "system_prompt".into(),
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
        let key = self.api_key.clone().ok_or_else(|| ProviderError {
            code: "missing_key".into(),
            message: "OpenAI API key is required".into(),
            category: ProviderErrorCategory::Auth,
            retryable: false,
            retry_after_ms: None,
        })?;
        let (content, tools, usage) =
            chat_completions(&self.client, &self.base_url, &key, request).await?;
        let mut blocks = Vec::new();
        if !content.is_empty() {
            blocks.push(ProviderResponseBlock::Text(content));
        }
        if let Some(tool_calls) = tools {
            for (id, name, args) in tool_calls {
                let input =
                    serde_json::from_str(&args).unwrap_or(serde_json::json!({ "raw": args }));
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
        request: ProviderRequest,
    ) -> Result<
        Box<dyn tokio_stream::Stream<Item = ProviderStreamEvent> + Send + Unpin>,
        ProviderError,
    > {
        // No offline mock success path — require credentials (use stream() with Credential).
        if self.api_key.is_none() {
            return Err(ProviderError {
                code: "missing_key".into(),
                message: "OpenAI API key required (offline mock removed)".into(),
                category: ProviderErrorCategory::Auth,
                retryable: false,
                retry_after_ms: None,
            });
        }
        let key = self.api_key.clone().unwrap();
        let pinned = stream_chat_completions(&self.client, &self.base_url, &key, request).await?;
        use futures_util::StreamExt;
        let mapped = pinned.map(|event| match event {
            ProviderEvent::TextDelta(t) => ProviderStreamEvent::TextDelta(t),
            ProviderEvent::ReasoningDelta(t) => ProviderStreamEvent::ReasoningDelta(t),
            ProviderEvent::ToolCallDelta {
                id,
                name,
                arguments_delta,
                ..
            } => {
                if let (Some(id), Some(name)) = (id.clone(), name.clone()) {
                    if arguments_delta.is_empty() {
                        ProviderStreamEvent::ToolCallBegin { id, name }
                    } else {
                        ProviderStreamEvent::ToolCallDelta {
                            id,
                            delta: arguments_delta,
                        }
                    }
                } else {
                    ProviderStreamEvent::ToolCallDelta {
                        id: id.unwrap_or_default(),
                        delta: arguments_delta,
                    }
                }
            }
            ProviderEvent::Usage(u) => ProviderStreamEvent::Done(u),
            ProviderEvent::Completed => ProviderStreamEvent::Done(ProviderUsage::default()),
            ProviderEvent::Error(e) => ProviderStreamEvent::Error(e),
        });
        // Collect into a ready stream so we can return Unpin + Box
        let items: Vec<_> = mapped.collect().await;
        Ok(Box::new(tokio_stream::iter(items)))
    }

    async fn stream(
        &self,
        request: ProviderRequest,
        credential: Credential,
    ) -> Result<
        std::pin::Pin<Box<dyn futures_util::Stream<Item = ProviderEvent> + Send>>,
        ProviderError,
    > {
        let (key, base) = self.resolve_credential(&credential)?;
        let client = crate::http_client::client(credential.proxy_url.as_deref())?;
        // Production HTTP for both Chat Completions and Responses APIs.
        if prefers_responses_api(&request) {
            stream_responses(&client, &base, &key, request).await
        } else {
            stream_chat_completions(&client, &base, &key, request).await
        }
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(vec![
            ModelInfo {
                id: "gpt-4o".to_string(),
                display_name: Some("GPT-4o".to_string()),
                context_window: 128_000,
                max_output: 16_384,
                capabilities: ModelCapabilities {
                    streaming: true,
                    image_input: true,
                    file_input: true,
                    reasoning: false,
                    tool_calling: true,
                    structured_output: true,
                    function_calling: true,
                    system_prompt: true,
                },
            },
            ModelInfo {
                id: "gpt-4o-mini".to_string(),
                display_name: Some("GPT-4o Mini".to_string()),
                context_window: 128_000,
                max_output: 16_384,
                capabilities: ModelCapabilities {
                    streaming: true,
                    image_input: true,
                    file_input: true,
                    reasoning: false,
                    tool_calling: true,
                    structured_output: true,
                    function_calling: true,
                    system_prompt: true,
                },
            },
        ])
    }

    async fn test_connection(&self) -> Result<ProviderTestResult, ProviderError> {
        if self.api_key.is_none() {
            return Ok(ProviderTestResult {
                success: false,
                latency_ms: None,
                message: "No API key configured".into(),
            });
        }
        self.test_connection_with_credential(Credential {
            api_key: self.api_key.clone().unwrap_or_default(),
            base_url: Some(self.base_url.clone()),
            proxy_url: None,
            key_id: None,
            provider_type: Some("openai".into()),
        })
        .await
    }

    async fn test_connection_with_credential(
        &self,
        credential: Credential,
    ) -> Result<ProviderTestResult, ProviderError> {
        let (key, base) = self.resolve_credential(&credential)?;
        let started = Instant::now();
        let url = format!("{}/models", base.trim_end_matches('/'));
        match self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {key}"))
            .timeout(std::time::Duration::from_secs(15))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => Ok(ProviderTestResult {
                success: true,
                latency_ms: Some(started.elapsed().as_millis() as u64),
                message: "OpenAI connection ok".into(),
            }),
            Ok(resp) => Ok(ProviderTestResult {
                success: false,
                latency_ms: Some(started.elapsed().as_millis() as u64),
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

/// Select OpenAI Responses API when explicitly requested or for models that
/// primarily expose the Responses surface.
fn prefers_responses_api(request: &ProviderRequest) -> bool {
    if std::env::var("NATIVES_OPENAI_API")
        .map(|v| v.eq_ignore_ascii_case("responses"))
        .unwrap_or(false)
    {
        return true;
    }
    let m = request.model.to_ascii_lowercase();
    m.contains("o1")
        || m.contains("o3")
        || m.contains("o4")
        || m.starts_with("gpt-5")
        || m.contains("responses")
}
