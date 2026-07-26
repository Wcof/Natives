//! Gemini GenerateContent adapter — real HTTP streaming (SSE / JSON array).

use crate::capabilities::*;
use crate::model_profile::{self, ReasoningControl};
use crate::stream::{parse_gemini_chunk, split_sse_lines, sse_data_payload, ProviderEvent};
use assistant_protocol::v1::provider::{ModelCapabilities, ProviderType};
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
        } else if message.role == "tool" {
            "user"
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
            structured_output: true,
            image_input: true,
            file_input: true,
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
        let url = format!(
            "{}/models/{}:streamGenerateContent?alt=sse&key={}",
            base.trim_end_matches('/'),
            model,
            key
        );
        let body = build_generate_body(&request);

        let client = match proxy_url.as_deref() {
            Some(url) if !url.trim().is_empty() => crate::http_client::client(Some(url))?,
            _ => self.client.clone(),
        };
        let response = client
            .post(&url)
            .header("Content-Type", "application/json")
            .timeout(Duration::from_secs(300))
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError {
                code: "network".into(),
                message: e.to_string(),
                category: ProviderErrorCategory::Network,
                retryable: true,
                retry_after_ms: None,
            })?;

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
            let mut saw_completed = false;
            tokio::pin!(byte_stream);
            while let Some(chunk) = byte_stream.next().await {
                match chunk {
                    Ok(bytes) => {
                        buffer.push_str(&String::from_utf8_lossy(&bytes));
                        for line in split_sse_lines(&mut buffer) {
                            if let Some(data) = sse_data_payload(&line) {
                                if data == "[DONE]" {
                                    saw_completed = true;
                                    yield ProviderEvent::Completed;
                                    continue;
                                }
                                for event in parse_gemini_chunk(data) {
                                    yield event;
                                }
                            }
                        }
                    }
                    Err(err) => {
                        yield ProviderEvent::Error(ProviderError {
                            code: "stream_error".into(),
                            message: err.to_string(),
                            category: ProviderErrorCategory::Network,
                            retryable: true,
                            retry_after_ms: None,
                        });
                        return;
                    }
                }
            }
            if !saw_completed {
                yield ProviderEvent::Completed;
            }
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
                    file_input: true,
                    reasoning: true,
                    tool_calling: true,
                    structured_output: true,
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
                    file_input: true,
                    reasoning: true,
                    tool_calling: true,
                    structured_output: true,
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

#[cfg(test)]
mod tool_message_tests {
    use super::*;

    fn plain(model: &str) -> ProviderRequest {
        ProviderRequest {
            model: model.into(),
            messages: vec![ProviderMessage {
                role: "user".into(),
                content: vec![ProviderContentBlock::Text { text: "hi".into() }],
            }],
            system_prompt: None,
            tools: None,
            max_tokens: None,
            temperature: None,
            stream: true,
            structured_output: None,
            controls: Default::default(),
        }
    }

    #[test]
    fn max_output_tokens_comes_from_the_model_profile() {
        assert_eq!(
            build_generate_body(&plain("gemini-2.5-pro"))["generationConfig"]["maxOutputTokens"],
            65_536
        );
        // Unknown model: no generationConfig at all rather than an invented cap.
        assert!(build_generate_body(&plain("gemma-local"))
            .get("generationConfig")
            .is_none());
    }

    #[test]
    fn thinking_budget_only_for_models_that_expose_it() {
        let controls = RequestControls {
            reasoning: Some(ReasoningRequest::new(ReasoningEffort::Medium)),
            ..Default::default()
        };
        let pro = build_generate_body_with_controls(&plain("gemini-2.5-pro"), &controls);
        assert_eq!(
            pro["generationConfig"]["thinkingConfig"]["thinkingBudget"],
            16_384
        );
        assert_eq!(
            pro["generationConfig"]["thinkingConfig"]["includeThoughts"],
            true
        );

        // Gemini 2.0 has no thinkingConfig; sending one is rejected.
        let flash = build_generate_body_with_controls(&plain("gemini-2.0-flash"), &controls);
        assert!(flash["generationConfig"].get("thinkingConfig").is_none());
    }

    #[test]
    fn tool_choice_encodes_to_function_calling_config() {
        let mut request = plain("gemini-2.5-pro");
        request.tools = Some(vec![ProviderTool {
            name: "get_weather".into(),
            description: Some("weather".into()),
            input_schema: serde_json::json!({"type": "object"}),
        }]);

        let forced = build_generate_body_with_controls(
            &request,
            &RequestControls {
                tool_choice: Some(ToolChoice::Tool {
                    name: "get_weather".into(),
                }),
                ..Default::default()
            },
        );
        let config = &forced["toolConfig"]["functionCallingConfig"];
        assert_eq!(config["mode"], "ANY");
        assert_eq!(config["allowedFunctionNames"][0], "get_weather");

        // Nothing requested: no toolConfig, provider default applies.
        assert!(build_generate_body(&request).get("toolConfig").is_none());
    }

    #[test]
    fn gemini_body_uses_function_call_and_response() {
        let body = build_generate_body(&ProviderRequest {
            model: "gemini-2.0-flash".into(),
            messages: vec![
                ProviderMessage {
                    role: "user".into(),
                    content: vec![ProviderContentBlock::Text {
                        text: "weather?".into(),
                    }],
                },
                ProviderMessage {
                    role: "assistant".into(),
                    content: vec![ProviderContentBlock::ToolCall {
                        id: "c1".into(),
                        name: "get_weather".into(),
                        input: serde_json::json!({"city": "SF"}),
                    }],
                },
                ProviderMessage {
                    role: "tool".into(),
                    content: vec![ProviderContentBlock::ToolResult {
                        tool_call_id: "c1".into(),
                        content: r#"{"temp":72}"#.into(),
                        name: Some("get_weather".into()),
                    }],
                },
            ],
            system_prompt: None,
            tools: None,
            max_tokens: None,
            temperature: None,
            stream: true,
            structured_output: None,
            controls: Default::default(),
        });

        let contents = body["contents"].as_array().unwrap();
        assert_eq!(contents.len(), 3);
        assert_eq!(contents[1]["role"], "model");
        assert_eq!(
            contents[1]["parts"][0]["functionCall"]["name"],
            "get_weather"
        );
        assert_eq!(contents[2]["role"], "user");
        assert_eq!(
            contents[2]["parts"][0]["functionResponse"]["name"],
            "get_weather"
        );
        assert_eq!(
            contents[2]["parts"][0]["functionResponse"]["response"]["temp"],
            72
        );
    }

    fn image_request(image: ImageSource) -> ProviderRequest {
        ProviderRequest {
            model: "gemini-2.5-flash".into(),
            messages: vec![ProviderMessage {
                role: "user".into(),
                content: vec![
                    ProviderContentBlock::Text {
                        text: "what is this".into(),
                    },
                    ProviderContentBlock::Image { image_url: image },
                ],
            }],
            system_prompt: None,
            tools: None,
            max_tokens: None,
            temperature: None,
            stream: true,
            structured_output: None,
            controls: Default::default(),
        }
    }

    #[test]
    fn data_uri_becomes_an_inline_data_part() {
        let body = build_generate_body(&image_request(ImageSource::new(
            "data:image/png;base64,AAAB",
        )));
        let part = &body["contents"][0]["parts"][1];
        assert_eq!(part["inlineData"]["mimeType"], "image/png");
        assert_eq!(part["inlineData"]["data"], "AAAB");
    }

    #[test]
    fn google_file_uri_becomes_a_file_data_part() {
        let body = build_generate_body(&image_request(
            ImageSource::new("gs://bucket/cat.png").with_media_type("image/png"),
        ));
        let part = &body["contents"][0]["parts"][1];
        assert_eq!(part["fileData"]["mimeType"], "image/png");
        assert_eq!(part["fileData"]["fileUri"], "gs://bucket/cat.png");
    }

    #[test]
    fn plain_web_url_is_announced_not_dropped() {
        // Gemini does not fetch arbitrary URLs. Historically this block was an
        // empty match arm and the image simply vanished.
        let body = build_generate_body(&image_request(ImageSource::new(
            "https://example.test/cat.png",
        )));
        let parts = body["contents"][0]["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 2);
        let note = parts[1]["text"].as_str().unwrap();
        assert!(note.contains("image not sent to the model"), "{note}");
        assert!(note.contains("https://example.test/cat.png"), "{note}");
    }

    #[test]
    fn inline_data_without_a_mime_type_is_announced_not_dropped() {
        let body = build_generate_body(&image_request(ImageSource::new("data:;base64,AAAB")));
        let note = body["contents"][0]["parts"][1]["text"].as_str().unwrap();
        assert!(note.contains("mimeType"), "{note}");
    }

    #[test]
    fn capability_flag_matches_the_encoder() {
        let caps = GeminiAdapter::new().capabilities();
        assert!(caps.image_input);
        let body = build_generate_body(&image_request(ImageSource::new(
            "data:image/webp;base64,AAAB",
        )));
        assert!(body["contents"][0]["parts"][1]
            .get("inlineData")
            .is_some());
    }
}
