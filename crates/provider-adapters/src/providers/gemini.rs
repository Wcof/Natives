//! Gemini GenerateContent adapter — real HTTP streaming (SSE / JSON array).

use crate::adapter::{ProviderAdapter, ProviderStreamEvent};
use crate::capabilities::*;
use crate::controls::RequestControls;
use crate::model_profile::{self, ReasoningControl};
use crate::provider_identity::{ModelCapabilities, ProviderType};
use crate::stream::{parse_gemini_chunk, split_sse_lines, sse_data_payload, ProviderEvent};
use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::Client;
use std::time::Duration;

pub struct GeminiAdapter {
    api_key: Option<String>,
    base_url: String,
    client: Client,
}

impl GeminiAdapter {
    pub fn new() -> Self {
        GeminiAdapter {
            api_key: None,
            base_url: "https://generativelanguage.googleapis.com/v1beta".to_string(),
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

impl Default for GeminiAdapter {
    fn default() -> Self {
        Self::new()
    }
}

/// Build the `generateContent` body using the request's own
/// [`ProviderRequest::controls`].
pub fn build_generate_body(request: &ProviderRequest) -> serde_json::Value {
    build_generate_body_with_controls(request, &request.controls)
}

/// Build the `generateContent` body with caller-supplied controls, which
/// override [`ProviderRequest::controls`] entirely.
///
/// # Prompt caching
///
/// Gemini has no per-request cache parameter to send. Implicit caching is
/// applied automatically by the provider on 2.5-series models, and explicit
/// caching requires creating a separate stateful `CachedContent` resource
/// (`POST /cachedContents`) with its own TTL and lifecycle — out of scope for a
/// stateless streaming adapter. Cache usage is therefore **read back only**,
/// from `usageMetadata.cachedContentTokenCount`.
pub fn build_generate_body_with_controls(
    request: &ProviderRequest,
    controls: &RequestControls,
) -> serde_json::Value {
    let profile = model_profile::resolve(&request.model);
    let mut contents = Vec::new();
    for message in &request.messages {
        let role = if message.role == "assistant" {
            "model"
        } else {
            "user"
        };
        let mut parts = Vec::new();
        for block in &message.content {
            match block {
                ProviderContentBlock::Text { text } => {
                    if !text.is_empty() {
                        parts.push(serde_json::json!({ "text": text }));
                    }
                }
                ProviderContentBlock::ToolCall { name, input, .. } => {
                    parts.push(serde_json::json!({
                        "functionCall": {
                            "name": name,
                            "args": input,
                        }
                    }));
                }
                ProviderContentBlock::ToolResult {
                    content,
                    name,
                    tool_call_id,
                } => {
                    let fn_name = name
                        .clone()
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| tool_call_id.clone());
                    let response = serde_json::from_str::<serde_json::Value>(content)
                        .unwrap_or_else(|_| serde_json::json!({ "result": content }));
                    parts.push(serde_json::json!({
                        "functionResponse": {
                            "name": fn_name,
                            "response": response,
                        }
                    }));
                }
                ProviderContentBlock::Image { image_url } => {
                    parts.push(image_part(image_url));
                }
            }
        }
        if parts.is_empty() {
            continue;
        }
        contents.push(serde_json::json!({ "role": role, "parts": parts }));
    }

    let mut body = serde_json::json!({ "contents": contents });
    if let Some(system) = &request.system_prompt {
        if !system.is_empty() {
            body["systemInstruction"] = serde_json::json!({
                "parts": [{ "text": system }]
            });
        }
    }
    if let Some(tools) = &request.tools {
        if !tools.is_empty() {
            let decls: Vec<_> = tools
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.input_schema,
                    })
                })
                .collect();
            body["tools"] = serde_json::json!([{ "functionDeclarations": decls }]);
            if let Some(choice) = &controls.tool_choice {
                body["toolConfig"] = serde_json::json!({
                    "functionCallingConfig": choice.to_gemini(),
                });
            }
        }
    }

    let mut generation_config = serde_json::Map::new();
    if let Some(max) = model_profile::resolve_max_output(request.max_tokens, &profile) {
        generation_config.insert("maxOutputTokens".into(), serde_json::json!(max));
    }
    if let Some(temperature) = request.temperature {
        if profile.sampling_params {
            generation_config.insert("temperature".into(), serde_json::json!(temperature));
        }
    }
    if let Some(reasoning) = &controls.reasoning {
        if profile.reasoning == ReasoningControl::GeminiThinkingBudget {
            generation_config.insert(
                "thinkingConfig".into(),
                serde_json::json!({
                    "thinkingBudget": reasoning.budget(),
                    "includeThoughts": true,
                }),
            );
        }
    }
    if !generation_config.is_empty() {
        body["generationConfig"] = serde_json::Value::Object(generation_config);
    }
    body
}

/// Whether a URI is one Gemini's `fileData` part can reference.
///
/// `fileData.fileUri` only resolves Google-hosted objects (File API uploads and
/// Cloud Storage). An arbitrary web URL is not fetched by the model.
fn is_google_file_uri(url: &str) -> bool {
    url.starts_with("gs://") || url.contains("generativelanguage.googleapis.com/")
}

/// Encode one image as a Gemini content part.
///
/// Gemini takes inline bytes (`inlineData`, needs a `mimeType`) or a
/// Google-hosted reference (`fileData`, also needs a `mimeType`). A plain web
/// URL is not fetchable, so it becomes a visible text part instead of being
/// dropped — the model is told an image was meant to be here and was not sent.
fn image_part(image: &ImageSource) -> serde_json::Value {
    match image.payload() {
        ImagePayload::Base64 {
            media_type: Some(mime_type),
            data,
        } => serde_json::json!({
            "inlineData": { "mimeType": mime_type, "data": data },
        }),
        ImagePayload::Base64 {
            media_type: None, ..
        } => serde_json::json!({
            "text": image.degraded_note("Gemini needs an explicit mimeType for inline image data"),
        }),
        ImagePayload::Remote {
            url,
            media_type: Some(mime_type),
        } if is_google_file_uri(url) => serde_json::json!({
            "fileData": { "mimeType": mime_type, "fileUri": url },
        }),
        ImagePayload::Remote { url, .. } if is_google_file_uri(url) => serde_json::json!({
            "text": image.degraded_note("Gemini needs an explicit mimeType for a fileData part"),
        }),
        ImagePayload::Remote { .. } => serde_json::json!({
            "text": image.degraded_note(
                "Gemini accepts inline base64 or a Google File API / gs:// URI, not a plain web URL",
            ),
        }),
    }
}

#[async_trait]
impl ProviderAdapter for GeminiAdapter {
    fn provider_type(&self) -> ProviderType {
        ProviderType::Gemini
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            provider_type: ProviderType::Gemini,
            features: vec![
                "streaming".into(),
                "tool_calls".into(),
                "function_calling".into(),
                "image_input".into(),
                "reasoning".into(),
                "system_prompt".into(),
                // Implicit caching only (2.5 series). Explicit `CachedContent`
                // resources are not created by this adapter.
                "prompt_cache_automatic".into(),
                // Via `toolConfig.functionCallingConfig`.
                "tool_choice".into(),
                // NOTE: no `parallel_tool_calls` — Gemini has no per-request
                // toggle for it.
            ],
            max_context_window: 1_048_576,
            streaming: true,
            tool_calls: true,
            structured_output: false,
            image_input: true,
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
                message: "Gemini API key required (no mock tool path)".into(),
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
                    provider_type: Some("gemini".into()),
                    project_id: None,
                },
            )
            .await?;
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
        // No mock "Hello from Gemini" tool-call path — require credentials via stream().
        Err(ProviderError {
            code: "use_stream".into(),
            message: "Use stream(request, credential) for Gemini; offline mock removed".into(),
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
                message: "Gemini API key required".into(),
                category: ProviderErrorCategory::Auth,
                retryable: false,
                retry_after_ms: None,
            })?
        };
        let proxy_url = credential.proxy_url.clone();
        let base = credential.base_url.unwrap_or_else(|| self.base_url.clone());
        let model = if request.model.is_empty() {
            "gemini-2.0-flash".to_string()
        } else {
            request.model.clone()
        };
        let url = stream_generate_content_url(&base, &model);
        let body = build_generate_body(&request);

        let client = match proxy_url.as_deref() {
            Some(url) if !url.trim().is_empty() => crate::http_client::client(Some(url))?,
            _ => self.client.clone(),
        };
        let response = client
            .post(&url)
            .header("x-goog-api-key", &key)
            .header("Content-Type", "application/json")
            .timeout(Duration::from_secs(300))
            .json(&body)
            .send()
            .await
            .map_err(|error| crate::http_stream::transport_error("network", &error))?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let headers = response.headers().clone();
            let text = response.text().await.unwrap_or_default();
            return Err(crate::http_stream::map_http_status(
                status,
                &text,
                Some(&headers),
            ));
        }

        let byte_stream = response.bytes_stream();
        let stream = async_stream::stream! {
            let mut buffer = String::new();
            tokio::pin!(byte_stream);
            while let Some(chunk) = byte_stream.next().await {
                match chunk {
                    Ok(bytes) => {
                        if let Err(error) = crate::http_stream::append_sse_bytes(&mut buffer, &bytes) {
                            yield ProviderEvent::Error(error);
                            return;
                        }
                        for line in split_sse_lines(&mut buffer) {
                            if let Some(data) = sse_data_payload(&line) {
                                if data == "[DONE]" {
                                    yield ProviderEvent::Error(crate::http_stream::incomplete_stream_error());
                                    return;
                                }
                                for event in parse_gemini_chunk(data) {
                                    let terminal = matches!(event, ProviderEvent::Completed { .. } | ProviderEvent::Error(_));
                                    yield event;
                                    if terminal {
                                        return;
                                    }
                                }
                            }
                        }
                    }
                    Err(err) => {
                        yield ProviderEvent::Error(crate::http_stream::transport_error("stream_error", &err));
                        return;
                    }
                }
            }
            yield ProviderEvent::Error(crate::http_stream::incomplete_stream_error());
        };
        Ok(Box::pin(stream))
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(vec![
            ModelInfo {
                id: "gemini-2.0-flash".into(),
                display_name: Some("Gemini 2.0 Flash".into()),
                context_window: 1_048_576,
                max_output: 8_192,
                capabilities: ModelCapabilities {
                    streaming: true,
                    image_input: true,
                    file_input: false,
                    reasoning: false,
                    tool_calling: true,
                    structured_output: false,
                    function_calling: true,
                    system_prompt: true,
                },
            },
            ModelInfo {
                id: "gemini-2.5-pro".into(),
                display_name: Some("Gemini 2.5 Pro".into()),
                context_window: 1_048_576,
                max_output: 8_192,
                capabilities: ModelCapabilities {
                    streaming: true,
                    image_input: true,
                    file_input: false,
                    reasoning: true,
                    tool_calling: true,
                    structured_output: false,
                    function_calling: true,
                    system_prompt: true,
                },
            },
        ])
    }

    async fn test_connection(&self) -> Result<ProviderTestResult, ProviderError> {
        Ok(ProviderTestResult {
            success: self.api_key.is_some(),
            latency_ms: None,
            message: if self.api_key.is_some() {
                "Gemini key present".into()
            } else {
                "No Gemini key — use stream with Credential".into()
            },
        })
    }
}

fn stream_generate_content_url(base_url: &str, model: &str) -> String {
    format!(
        "{}/models/{}:streamGenerateContent?alt=sse",
        base_url.trim_end_matches('/'),
        model
    )
}
#[cfg(test)]
#[path = "gemini_tests.rs"]
mod tool_message_tests;
