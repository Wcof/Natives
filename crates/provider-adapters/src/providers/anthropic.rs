//! Anthropic Messages API adapter — real HTTP streaming.

use crate::adapter::{ProviderAdapter, ProviderStreamEvent};
use crate::capabilities::*;
use crate::controls::{RequestControls, ToolChoice};
use crate::model_profile::{self, ModelProfile, ReasoningControl};
use crate::provider_identity::{ModelCapabilities, ProviderType};
use crate::stream::{split_sse_lines, sse_data_payload, AnthropicSseParser, ProviderEvent};
use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::Client;
use std::time::Duration;

/// `max_tokens` is mandatory on the Messages API, so an unknown model still
/// needs a number. This is the historical value and is kept so that a model id
/// missing from the profile table behaves exactly as it did before.
const ANTHROPIC_FALLBACK_MAX_OUTPUT: u64 = 4096;

/// Anthropic accepts at most four `cache_control` breakpoints per request.
const MAX_CACHE_BREAKPOINTS: usize = 4;

/// Smallest thinking budget Anthropic accepts on `budget_tokens` models.
const MIN_THINKING_BUDGET: u64 = 1024;

pub struct AnthropicAdapter {
    api_key: Option<String>,
    base_url: String,
    client: Client,
}

impl AnthropicAdapter {
    pub fn new() -> Self {
        AnthropicAdapter {
            api_key: None,
            base_url: "https://api.anthropic.com".to_string(),
            client: Client::new(),
        }
    }
    pub fn with_api_key(mut self, key: String) -> Self {
        self.api_key = Some(key);
        self
    }
}
impl Default for AnthropicAdapter {
    fn default() -> Self {
        Self::new()
    }
}

/// Build the Messages API body using the request's own
/// [`ProviderRequest::controls`].
pub fn build_messages_body(request: &ProviderRequest) -> serde_json::Value {
    build_messages_body_with_controls(request, &request.controls)
}

/// Build the Messages API body with caller-supplied controls, which override
/// [`ProviderRequest::controls`] entirely.
///
/// # Prompt caching
///
/// The system prompt assembled by `agent-core` (AGENTS.md + CLAUDE.md +
/// `.claude/rules/*` + skill bodies) and the tool schemas are resent verbatim
/// on every turn of an agent loop. Without breakpoints every one of those turns
/// is billed at full input price. Anthropic renders a request as
/// `tools → system → messages`, so a breakpoint covers everything before it.
///
/// Three of the four available breakpoints are used, chosen so that a change in
/// one region does not invalidate the regions before it:
///
/// 1. **Last tool definition** — the most stable prefix. Survives a system
///    prompt edit.
/// 2. **Last system block** — covers tools + system. Survives any new turn.
/// 3. **Second-to-last wire message** — the newest *stable* history boundary.
///    A turn appends after it, so the next request reads this entry and writes
///    a new one one turn further along (the standard rolling pattern). The
///    final message is deliberately left unmarked: marking it would write a
///    cache entry for a prefix that the next request immediately extends.
///
/// The fourth slot is intentionally left free as headroom.
///
/// Marking a prefix shorter than the model's minimum is a silent no-op rather
/// than an error, but it still consumes a slot — so the system breakpoint is
/// skipped when the prompt is provably too short (character count is an upper
/// bound on token count).
pub fn build_messages_body_with_controls(
    request: &ProviderRequest,
    controls: &RequestControls,
) -> serde_json::Value {
    let profile = model_profile::resolve(&request.model);
    let mut messages = Vec::new();
    // Anthropic requires tool_result blocks to live in a user message, possibly batched.
    let mut pending_tool_results: Vec<serde_json::Value> = Vec::new();
    // Non-tool_result blocks (images) that arrived on a `role: tool` message.
    // They ride out in the same synthetic user message, after the results —
    // Anthropic only requires that tool_result blocks come first.
    let mut pending_tool_extras: Vec<serde_json::Value> = Vec::new();

    let flush_tool_results = |messages: &mut Vec<serde_json::Value>,
                              pending: &mut Vec<serde_json::Value>,
                              extras: &mut Vec<serde_json::Value>| {
        if pending.is_empty() && extras.is_empty() {
            return;
        }
        let mut content = std::mem::take(pending);
        content.append(extras);
        messages.push(serde_json::json!({
            "role": "user",
            "content": content,
        }));
    };

    for message in &request.messages {
        let mut content_blocks = Vec::new();
        let mut is_tool_result_only = true;
        for block in &message.content {
            match block {
                ProviderContentBlock::Text { text } => {
                    if !text.is_empty() {
                        is_tool_result_only = false;
                        content_blocks.push(serde_json::json!({
                            "type": "text",
                            "text": text,
                        }));
                    }
                }
                ProviderContentBlock::ToolCall { id, name, input } => {
                    is_tool_result_only = false;
                    content_blocks.push(serde_json::json!({
                        "type": "tool_use",
                        "id": id,
                        "name": name,
                        "input": input,
                    }));
                }
                ProviderContentBlock::ToolResult {
                    tool_call_id,
                    content,
                    ..
                } => {
                    pending_tool_results.push(serde_json::json!({
                        "type": "tool_result",
                        "tool_use_id": tool_call_id,
                        "content": content,
                    }));
                }
                ProviderContentBlock::Image { image_url } => {
                    is_tool_result_only = false;
                    content_blocks.push(image_block(image_url));
                }
            }
        }

        if message.role == "tool"
            || (is_tool_result_only
                && !pending_tool_results.is_empty()
                && content_blocks.is_empty())
        {
            // Keep accumulating tool_result blocks; flushed before next non-tool
            // message. Anything else on the same message (an image) rides along
            // instead of being dropped on the floor.
            pending_tool_extras.append(&mut content_blocks);
            continue;
        }

        flush_tool_results(
            &mut messages,
            &mut pending_tool_results,
            &mut pending_tool_extras,
        );

        if content_blocks.is_empty() {
            continue;
        }
        let role = if message.role == "assistant" {
            "assistant"
        } else {
            "user"
        };
        messages.push(serde_json::json!({
            "role": role,
            "content": content_blocks,
        }));
    }
    flush_tool_results(
        &mut messages,
        &mut pending_tool_results,
        &mut pending_tool_extras,
    );

    let max_tokens = model_profile::resolve_max_output(request.max_tokens, &profile)
        .unwrap_or(ANTHROPIC_FALLBACK_MAX_OUTPUT);
    let cache_enabled = controls.prompt_cache_enabled(&profile);
    let mut breakpoints = 0usize;

    let mut body = serde_json::json!({
        "model": request.model,
        "max_tokens": max_tokens,
        "stream": true,
    });

    // Breakpoint 1 — tools (rendered first, therefore the most stable prefix).
    if let Some(tools) = &request.tools {
        if !tools.is_empty() {
            let mut tool_values: Vec<serde_json::Value> = tools
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "name": t.name,
                        "description": t.description,
                        "input_schema": t.input_schema,
                    })
                })
                .collect();
            if cache_enabled && breakpoints < MAX_CACHE_BREAKPOINTS {
                if let Some(last) = tool_values.last_mut() {
                    last["cache_control"] = ephemeral();
                    breakpoints += 1;
                }
            }
            body["tools"] = serde_json::Value::Array(tool_values);
        }
    }

    // Breakpoint 2 — system prompt (covers tools + system).
    if let Some(system) = &request.system_prompt {
        if !system.is_empty() {
            let cacheable = cache_enabled
                && breakpoints < MAX_CACHE_BREAKPOINTS
                && can_reach_cache_minimum(system, &profile);
            if cacheable {
                body["system"] = serde_json::json!([{
                    "type": "text",
                    "text": system,
                    "cache_control": ephemeral(),
                }]);
                breakpoints += 1;
            } else {
                body["system"] = serde_json::json!(system);
            }
        }
    }

    // Breakpoint 3 — newest stable history boundary.
    if cache_enabled && breakpoints < MAX_CACHE_BREAKPOINTS && messages.len() >= 3 {
        let boundary = messages.len() - 2;
        if mark_last_content_block(&mut messages[boundary]) {
            breakpoints += 1;
        }
    }

    debug_assert!(
        breakpoints <= MAX_CACHE_BREAKPOINTS,
        "Anthropic rejects more than {MAX_CACHE_BREAKPOINTS} cache_control breakpoints, emitted {breakpoints}"
    );

    body["messages"] = serde_json::Value::Array(messages);

    // Reasoning. `thinking` and `temperature` are mutually exclusive on
    // Anthropic, so the sampling parameter is dropped when thinking is on.
    let mut thinking_enabled = false;
    if let Some(reasoning) = &controls.reasoning {
        match profile.reasoning {
            ReasoningControl::AnthropicAdaptive => {
                // `budget_tokens` is rejected with a 400 on these models; depth
                // is controlled by the model itself.
                body["thinking"] = serde_json::json!({ "type": "adaptive" });
                thinking_enabled = true;
            }
            // Anthropic requires MIN_THINKING_BUDGET <= budget < max_tokens, so
            // a ceiling that leaves no room simply gets no thinking block
            // rather than a guaranteed 400.
            ReasoningControl::AnthropicBudget if max_tokens > MIN_THINKING_BUDGET => {
                let budget = reasoning
                    .budget()
                    .clamp(MIN_THINKING_BUDGET, max_tokens - 1);
                body["thinking"] = serde_json::json!({
                    "type": "enabled",
                    "budget_tokens": budget,
                });
                thinking_enabled = true;
            }
            _ => {}
        }
    }

    // `temperature` was previously dropped on the floor for every Anthropic
    // request. It is now forwarded, but only where the model still accepts it.
    if let Some(temperature) = request.temperature {
        if profile.sampling_params && !thinking_enabled {
            body["temperature"] = serde_json::json!(temperature);
        }
    }

    // Tool choice / parallelism. Anthropic carries `disable_parallel_tool_use`
    // inside the `tool_choice` object, so a bare parallelism override still has
    // to synthesise `{"type": "auto"}`.
    if body.get("tools").is_some() {
        match (&controls.tool_choice, controls.parallel_tool_calls) {
            (Some(choice), parallel) => {
                body["tool_choice"] = choice.to_anthropic(parallel);
            }
            (None, Some(false)) => {
                body["tool_choice"] = ToolChoice::Auto.to_anthropic(Some(false));
            }
            (None, _) => {}
        }
    }

    body
}

fn ephemeral() -> serde_json::Value {
    serde_json::json!({ "type": "ephemeral" })
}

/// Encode one image as an Anthropic `image` content block.
///
/// The Messages API takes two source shapes: inline `base64` (which needs an
/// explicit `media_type`) and `url` (http/https only, fetched by Anthropic).
/// Anything else — a `data:` URI with no MIME type, a `gs://` object — cannot
/// be expressed, and is turned into a visible text block rather than dropped,
/// so the model and the user both learn the image did not arrive.
fn image_block(image: &ImageSource) -> serde_json::Value {
    match image.payload() {
        ImagePayload::Base64 {
            media_type: Some(media_type),
            data,
        } => serde_json::json!({
            "type": "image",
            "source": {
                "type": "base64",
                "media_type": media_type,
                "data": data,
            },
        }),
        ImagePayload::Base64 {
            media_type: None, ..
        } => serde_json::json!({
            "type": "text",
            "text": image.degraded_note(
                "Anthropic needs an explicit media_type for inline image data",
            ),
        }),
        ImagePayload::Remote { url, .. }
            if url.starts_with("https://") || url.starts_with("http://") =>
        {
            serde_json::json!({
                "type": "image",
                "source": { "type": "url", "url": url },
            })
        }
        ImagePayload::Remote { .. } => serde_json::json!({
            "type": "text",
            "text": image.degraded_note(
                "Anthropic accepts only inline base64 or an http(s) URL",
            ),
        }),
    }
}

/// Whether `text` could possibly reach the model's minimum cacheable prefix.
///
/// One token is never fewer than one character, so `chars < minimum` proves the
/// prefix is uncacheable and the breakpoint slot is better spent elsewhere.
/// The converse is not asserted — a long prompt may still fall short, in which
/// case the marker is a harmless no-op.
fn can_reach_cache_minimum(text: &str, profile: &ModelProfile) -> bool {
    let minimum = profile.prompt_cache_min_tokens.unwrap_or(1024);
    text.chars().count() as u64 >= minimum
}

/// Attach a cache breakpoint to the final content block of a wire message.
///
/// Returns `false` when the message has no content array to mark.
fn mark_last_content_block(message: &mut serde_json::Value) -> bool {
    let Some(blocks) = message.get_mut("content").and_then(|c| c.as_array_mut()) else {
        return false;
    };
    let Some(last) = blocks.last_mut() else {
        return false;
    };
    let Some(object) = last.as_object_mut() else {
        return false;
    };
    object.insert("cache_control".into(), ephemeral());
    true
}

#[async_trait]
impl ProviderAdapter for AnthropicAdapter {
    fn provider_type(&self) -> ProviderType {
        ProviderType::Anthropic
    }
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            provider_type: ProviderType::Anthropic,
            features: vec![
                "streaming".into(),
                "tool_calls".into(),
                "function_calling".into(),
                "reasoning".into(),
                "system_prompt".into(),
                // Real `image` content blocks; see `image_block`.
                "image_input".into(),
                // Caller-placed `cache_control` breakpoints.
                "prompt_cache_explicit".into(),
                "tool_choice".into(),
                // Via `tool_choice.disable_parallel_tool_use`.
                "parallel_tool_calls".into(),
            ],
            max_context_window: 200_000,
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
                message: "Anthropic API key required (offline mock removed)".into(),
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
                    provider_type: Some("anthropic".into()),
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
        Err(ProviderError {
            code: "use_stream".into(),
            message: "Use stream(request, credential) for Anthropic; offline mock removed".into(),
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
                message: "Anthropic API key required".into(),
                category: ProviderErrorCategory::Auth,
                retryable: false,
                retry_after_ms: None,
            })?
        };
        let proxy_url = credential.proxy_url.clone();
        let base = credential.base_url.unwrap_or_else(|| self.base_url.clone());
        // 审计收口 #8：endpoint 规范化——base 已含 `/v1` 时不得重复追加，
        // 否则会得到 `/v1/v1/messages`。与 Chat/Responses 的规范化一致。
        let trimmed = base.trim_end_matches('/');
        let url = if trimmed.ends_with("/v1") {
            format!("{trimmed}/messages")
        } else {
            format!("{trimmed}/v1/messages")
        };
        let body = build_messages_body(&request);

        let client = match proxy_url.as_deref() {
            Some(url) if !url.trim().is_empty() => crate::http_client::client(Some(url))?,
            _ => self.client.clone(),
        };
        let response = client
            .post(&url)
            .header("x-api-key", &key)
            .header("anthropic-version", "2023-06-01")
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
            let mut parser = AnthropicSseParser::new();
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
                                for event in parser.push_data_line(data) {
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
        Ok(vec![ModelInfo {
            id: "claude-sonnet-4-20250514".into(),
            display_name: Some("Claude Sonnet 4".into()),
            context_window: 200_000,
            max_output: 16_384,
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
        }])
    }
    async fn test_connection(&self) -> Result<ProviderTestResult, ProviderError> {
        Ok(ProviderTestResult {
            success: self.api_key.is_some(),
            latency_ms: None,
            message: "Anthropic adapter ready".into(),
        })
    }
}
#[cfg(test)]
#[path = "anthropic_tests.rs"]
mod request_tests;
