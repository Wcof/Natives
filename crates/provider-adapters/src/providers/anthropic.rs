//! Anthropic Messages API adapter — real HTTP streaming.

use crate::capabilities::*;
use crate::model_profile::{self, ModelProfile, ReasoningControl};
use crate::stream::{split_sse_lines, sse_data_payload, AnthropicSseParser, ProviderEvent};
use assistant_protocol::v1::provider::{ModelCapabilities, ProviderType};
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

/// Build the Messages API body with default request controls.
pub fn build_messages_body(request: &ProviderRequest) -> serde_json::Value {
    build_messages_body_with_controls(request, &RequestControls::default())
}

/// Build the Messages API body.
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
        let url = format!("{}/v1/messages", base.trim_end_matches('/'));
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
            let mut parser = AnthropicSseParser::new();
            let mut buffer = String::new();
            let mut saw_completed = false;
            tokio::pin!(byte_stream);
            while let Some(chunk) = byte_stream.next().await {
                match chunk {
                    Ok(bytes) => {
                        buffer.push_str(&String::from_utf8_lossy(&bytes));
                        for line in split_sse_lines(&mut buffer) {
                            if let Some(data) = sse_data_payload(&line) {
                                for event in parser.push_data_line(data) {
                                    if matches!(event, ProviderEvent::Completed) {
                                        saw_completed = true;
                                    }
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
mod request_tests {
    use super::*;

    fn tool(name: &str) -> ProviderTool {
        ProviderTool {
            name: name.into(),
            description: Some(format!("{name} description")),
            input_schema: serde_json::json!({"type": "object"}),
        }
    }

    fn text(role: &str, body: &str) -> ProviderMessage {
        ProviderMessage {
            role: role.into(),
            content: vec![ProviderContentBlock::Text { text: body.into() }],
        }
    }

    fn request(model: &str) -> ProviderRequest {
        ProviderRequest {
            model: model.into(),
            messages: vec![text("user", "hi")],
            system_prompt: None,
            tools: None,
            max_tokens: None,
            temperature: None,
            stream: true,
            structured_output: None,
        }
    }

    /// Long enough to clear every model's minimum cacheable prefix, since one
    /// token is never fewer than one character.
    fn long_system() -> String {
        "You are a careful engineer. ".repeat(400)
    }

    #[test]
    fn caches_system_and_tools_with_ephemeral_breakpoints() {
        let mut req = request("claude-sonnet-4-5");
        req.system_prompt = Some(long_system());
        req.tools = Some(vec![tool("read_file"), tool("write_file")]);
        let body = build_messages_body(&req);

        // System becomes a block array carrying the breakpoint.
        let system = body["system"].as_array().expect("system block array");
        assert_eq!(system.len(), 1);
        assert_eq!(system[0]["cache_control"]["type"], "ephemeral");
        assert_eq!(system[0]["type"], "text");

        // Only the *last* tool is marked — one breakpoint covers all tools.
        let tools = body["tools"].as_array().expect("tools");
        assert_eq!(tools.len(), 2);
        assert!(tools[0].get("cache_control").is_none());
        assert_eq!(tools[1]["cache_control"]["type"], "ephemeral");
    }

    #[test]
    fn never_exceeds_the_four_breakpoint_limit() {
        let mut req = request("claude-sonnet-4-5");
        req.system_prompt = Some(long_system());
        req.tools = Some(vec![tool("a"), tool("b")]);
        req.messages = (0..40)
            .map(|i| {
                text(
                    if i % 2 == 0 { "user" } else { "assistant" },
                    &format!("turn {i}"),
                )
            })
            .collect();
        let body = build_messages_body(&req);
        let marks = body.to_string().matches("cache_control").count();
        assert!(
            marks <= MAX_CACHE_BREAKPOINTS,
            "emitted {marks} breakpoints, Anthropic accepts at most {MAX_CACHE_BREAKPOINTS}"
        );
        assert_eq!(marks, 3, "tools + system + one stable history boundary");
    }

    #[test]
    fn history_breakpoint_lands_on_the_stable_boundary_not_the_last_turn() {
        let mut req = request("claude-sonnet-4-5");
        req.messages = vec![
            text("user", "first"),
            text("assistant", "second"),
            text("user", "third"),
        ];
        let body = build_messages_body(&req);
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 3);
        assert!(messages[0]["content"][0].get("cache_control").is_none());
        // Second-to-last: the prefix the next request will extend.
        assert_eq!(
            messages[1]["content"][0]["cache_control"]["type"],
            "ephemeral"
        );
        // The final turn is left unmarked on purpose.
        assert!(messages[2]["content"][0].get("cache_control").is_none());
    }

    #[test]
    fn short_conversation_gets_no_history_breakpoint() {
        let body = build_messages_body(&request("claude-sonnet-4-5"));
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1);
        assert!(messages[0]["content"][0].get("cache_control").is_none());
    }

    #[test]
    fn short_system_prompt_does_not_burn_a_breakpoint() {
        let mut req = request("claude-opus-4-6"); // 4096-token minimum
        req.system_prompt = Some("Be brief.".into());
        let body = build_messages_body(&req);
        // Falls back to the plain-string form: provably below the minimum, so
        // marking it would consume a slot for a guaranteed no-op.
        assert!(body["system"].is_string());
    }

    #[test]
    fn caching_can_be_disabled_per_request() {
        let mut req = request("claude-sonnet-4-5");
        req.system_prompt = Some(long_system());
        req.tools = Some(vec![tool("read_file")]);
        req.messages = vec![
            text("user", "a"),
            text("assistant", "b"),
            text("user", "c"),
        ];
        let controls = RequestControls {
            prompt_cache: Some(false),
            ..Default::default()
        };
        let body = build_messages_body_with_controls(&req, &controls);
        assert!(!body.to_string().contains("cache_control"));
        assert!(body["system"].is_string());
    }

    #[test]
    fn resolves_max_tokens_from_the_model_instead_of_a_hardcoded_4096() {
        // Known model, no caller value: use the real ceiling.
        assert_eq!(build_messages_body(&request("claude-sonnet-4-5"))["max_tokens"], 64_000);
        assert_eq!(build_messages_body(&request("claude-opus-5"))["max_tokens"], 128_000);
        // Unknown model: unchanged historical fallback, no invented ceiling.
        assert_eq!(build_messages_body(&request("claude-unreleased-99"))["max_tokens"], 4096);

        // An explicit caller value is honoured and clamped, never raised.
        let mut small = request("claude-sonnet-4-5");
        small.max_tokens = Some(256);
        assert_eq!(build_messages_body(&small)["max_tokens"], 256);
        let mut huge = request("claude-sonnet-4-5");
        huge.max_tokens = Some(10_000_000);
        assert_eq!(build_messages_body(&huge)["max_tokens"], 64_000);
    }

    #[test]
    fn thinking_uses_the_dialect_the_model_accepts() {
        let controls = RequestControls {
            reasoning: Some(ReasoningRequest::new(ReasoningEffort::High)),
            ..Default::default()
        };

        // Claude 4.5 and older: explicit token budget.
        let budgeted = build_messages_body_with_controls(&request("claude-sonnet-4-5"), &controls);
        assert_eq!(budgeted["thinking"]["type"], "enabled");
        assert_eq!(budgeted["thinking"]["budget_tokens"], 32_768);

        // Claude 4.6+: `budget_tokens` is a 400 there, so it must be absent.
        let adaptive = build_messages_body_with_controls(&request("claude-opus-5"), &controls);
        assert_eq!(adaptive["thinking"]["type"], "adaptive");
        assert!(adaptive["thinking"].get("budget_tokens").is_none());

        // No reasoning requested: no `thinking` key at all.
        assert!(build_messages_body(&request("claude-sonnet-4-5"))
            .get("thinking")
            .is_none());
    }

    #[test]
    fn thinking_budget_stays_below_max_tokens() {
        let mut req = request("claude-sonnet-4-5");
        req.max_tokens = Some(2_000);
        let controls = RequestControls {
            reasoning: Some(ReasoningRequest::new(ReasoningEffort::High)),
            ..Default::default()
        };
        let body = build_messages_body_with_controls(&req, &controls);
        let budget = body["thinking"]["budget_tokens"].as_u64().unwrap();
        assert!(
            budget < 2_000,
            "budget {budget} must stay under max_tokens or Anthropic returns 400"
        );
        assert!(budget >= MIN_THINKING_BUDGET);
    }

    #[test]
    fn temperature_is_forwarded_only_where_the_model_accepts_it() {
        let mut older = request("claude-sonnet-4-5");
        older.temperature = Some(0.2);
        assert_eq!(build_messages_body(&older)["temperature"], 0.2);

        // Opus 5 rejects sampling parameters outright.
        let mut newer = request("claude-opus-5");
        newer.temperature = Some(0.2);
        assert!(build_messages_body(&newer).get("temperature").is_none());

        // Thinking and temperature are mutually exclusive.
        let controls = RequestControls {
            reasoning: Some(ReasoningRequest::new(ReasoningEffort::Low)),
            ..Default::default()
        };
        let body = build_messages_body_with_controls(&older, &controls);
        assert!(body.get("temperature").is_none());
    }

    #[test]
    fn tool_choice_and_parallelism_encode_to_the_anthropic_shape() {
        let mut req = request("claude-sonnet-4-5");
        req.tools = Some(vec![tool("read_file")]);

        let forced = build_messages_body_with_controls(
            &req,
            &RequestControls {
                tool_choice: Some(ToolChoice::Tool {
                    name: "read_file".into(),
                }),
                ..Default::default()
            },
        );
        assert_eq!(forced["tool_choice"]["type"], "tool");
        assert_eq!(forced["tool_choice"]["name"], "read_file");

        // `Required` is spelled `any` on Anthropic.
        let required = build_messages_body_with_controls(
            &req,
            &RequestControls {
                tool_choice: Some(ToolChoice::Required),
                ..Default::default()
            },
        );
        assert_eq!(required["tool_choice"]["type"], "any");

        // Parallelism rides inside tool_choice, so a bare override still needs
        // an object.
        let serial = build_messages_body_with_controls(
            &req,
            &RequestControls {
                parallel_tool_calls: Some(false),
                ..Default::default()
            },
        );
        assert_eq!(serial["tool_choice"]["type"], "auto");
        assert_eq!(serial["tool_choice"]["disable_parallel_tool_use"], true);

        // Nothing requested: no key, so the provider default applies.
        assert!(build_messages_body(&req).get("tool_choice").is_none());
    }

    #[test]
    fn tool_choice_is_omitted_when_no_tools_are_present() {
        let body = build_messages_body_with_controls(
            &request("claude-sonnet-4-5"),
            &RequestControls {
                tool_choice: Some(ToolChoice::Required),
                ..Default::default()
            },
        );
        assert!(body.get("tool_choice").is_none());
    }

    #[test]
    fn anthropic_body_uses_tool_use_and_tool_result_blocks() {
        let body = build_messages_body(&ProviderRequest {
            model: "claude-sonnet-4".into(),
            messages: vec![
                ProviderMessage {
                    role: "user".into(),
                    content: vec![ProviderContentBlock::Text {
                        text: "read it".into(),
                    }],
                },
                ProviderMessage {
                    role: "assistant".into(),
                    content: vec![ProviderContentBlock::ToolCall {
                        id: "toolu_1".into(),
                        name: "read_file".into(),
                        input: serde_json::json!({"path": "x"}),
                    }],
                },
                ProviderMessage {
                    role: "tool".into(),
                    content: vec![ProviderContentBlock::ToolResult {
                        tool_call_id: "toolu_1".into(),
                        content: "file data".into(),
                        name: Some("read_file".into()),
                    }],
                },
            ],
            system_prompt: Some("sys".into()),
            tools: None,
            max_tokens: Some(256),
            temperature: None,
            stream: true,
            structured_output: None,
        });

        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[1]["role"], "assistant");
        assert_eq!(messages[1]["content"][0]["type"], "tool_use");
        assert_eq!(messages[1]["content"][0]["id"], "toolu_1");
        assert_eq!(messages[2]["role"], "user");
        assert_eq!(messages[2]["content"][0]["type"], "tool_result");
        assert_eq!(messages[2]["content"][0]["tool_use_id"], "toolu_1");
        assert_eq!(messages[2]["content"][0]["content"], "file data");
    }

    fn image_request(image: ImageSource) -> ProviderRequest {
        ProviderRequest {
            model: "claude-sonnet-4-5".into(),
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
            max_tokens: Some(256),
            temperature: None,
            stream: true,
            structured_output: None,
        }
    }

    #[test]
    fn data_uri_becomes_a_base64_image_source() {
        let body = build_messages_body(&image_request(ImageSource::new(
            "data:image/png;base64,AAAB",
        )));
        let block = &body["messages"][0]["content"][1];
        assert_eq!(block["type"], "image");
        assert_eq!(block["source"]["type"], "base64");
        assert_eq!(block["source"]["media_type"], "image/png");
        assert_eq!(block["source"]["data"], "AAAB");
    }

    #[test]
    fn https_url_becomes_a_url_image_source() {
        let body = build_messages_body(&image_request(ImageSource::new(
            "https://example.test/cat.png",
        )));
        let block = &body["messages"][0]["content"][1];
        assert_eq!(block["type"], "image");
        assert_eq!(block["source"]["type"], "url");
        assert_eq!(block["source"]["url"], "https://example.test/cat.png");
    }

    #[test]
    fn unsupported_image_reference_is_announced_not_dropped() {
        let body = build_messages_body(&image_request(ImageSource::new("gs://bucket/cat.png")));
        let content = body["messages"][0]["content"].as_array().unwrap();
        assert_eq!(content.len(), 2, "the block must survive as something");
        assert_eq!(content[1]["type"], "text");
        let note = content[1]["text"].as_str().unwrap();
        assert!(note.contains("image not sent to the model"), "{note}");
        assert!(note.contains("gs://bucket/cat.png"), "{note}");
    }

    #[test]
    fn inline_data_without_a_media_type_is_announced_not_dropped() {
        let body = build_messages_body(&image_request(ImageSource::new("data:;base64,AAAB")));
        let block = &body["messages"][0]["content"][1];
        assert_eq!(block["type"], "text");
        assert!(block["text"]
            .as_str()
            .unwrap()
            .contains("media_type"));
    }

    #[test]
    fn image_on_a_tool_message_rides_out_with_the_tool_results() {
        let body = build_messages_body(&ProviderRequest {
            model: "claude-sonnet-4-5".into(),
            messages: vec![
                ProviderMessage {
                    role: "assistant".into(),
                    content: vec![ProviderContentBlock::ToolCall {
                        id: "toolu_1".into(),
                        name: "screenshot".into(),
                        input: serde_json::json!({}),
                    }],
                },
                ProviderMessage {
                    role: "tool".into(),
                    content: vec![
                        ProviderContentBlock::ToolResult {
                            tool_call_id: "toolu_1".into(),
                            content: "captured".into(),
                            name: Some("screenshot".into()),
                        },
                        ProviderContentBlock::Image {
                            image_url: ImageSource::new("data:image/png;base64,AAAB"),
                        },
                    ],
                },
            ],
            system_prompt: None,
            tools: None,
            max_tokens: Some(256),
            temperature: None,
            stream: true,
            structured_output: None,
        });
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2);
        let results = messages[1]["content"].as_array().unwrap();
        // tool_result first (Anthropic's requirement), image after it.
        assert_eq!(results.len(), 2);
        assert_eq!(results[0]["type"], "tool_result");
        assert_eq!(results[1]["type"], "image");
    }

    #[test]
    fn capability_flag_matches_the_encoder() {
        let caps = AnthropicAdapter::new().capabilities();
        assert!(caps.image_input);
        assert!(caps.features.iter().any(|f| f == "image_input"));
        // The flag is only honest because an image really is encoded.
        let body = build_messages_body(&image_request(ImageSource::new(
            "data:image/jpeg;base64,AAAB",
        )));
        assert_eq!(body["messages"][0]["content"][1]["type"], "image");
    }
}
