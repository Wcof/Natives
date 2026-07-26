//! Shared HTTP streaming helper for OpenAI-compatible chat completions.

use crate::capabilities::{
    ProviderContentBlock, ProviderError, ProviderErrorCategory, ProviderMessage, ProviderRequest,
    ProviderTool, RequestControls,
};
use crate::model_profile::{self, ReasoningControl};
use crate::stream::{split_sse_lines, sse_data_payload, OpenAiSseParser, ProviderEvent};
use chrono::{DateTime, Utc};
use futures_util::StreamExt;
use reqwest::Client;
use std::time::Duration;

/// Build the JSON body for OpenAI chat completions using the request's own
/// [`ProviderRequest::controls`].
///
/// This is the entry point every streaming/non-streaming path uses, so a
/// caller only has to populate `controls` on the request it already builds.
pub fn build_chat_completions_body(request: &ProviderRequest) -> serde_json::Value {
    build_chat_completions_body_with_controls(request, &request.controls)
}

/// Build the JSON body for OpenAI chat completions with caller-supplied
/// controls, which override [`ProviderRequest::controls`] entirely.
///
/// # Prompt caching
///
/// OpenAI-compatible providers that cache do so **automatically** on the
/// longest common prefix — there is no request-side parameter to send, so
/// nothing is emitted here. Cache usage is read back from the response
/// (`prompt_tokens_details.cached_tokens` for OpenAI,
/// `prompt_cache_hit_tokens` for DeepSeek); see
/// [`crate::stream::openai_sse`]. Callers get the benefit only if they keep the
/// prefix byte-stable, which the engine already does by appending turns rather
/// than rewriting history.
pub fn build_chat_completions_body_with_controls(
    request: &ProviderRequest,
    controls: &RequestControls,
) -> serde_json::Value {
    let profile = model_profile::resolve(&request.model);
    let mut messages = Vec::new();
    if let Some(system) = &request.system_prompt {
        if !system.is_empty() {
            messages.push(serde_json::json!({
                "role": "system",
                "content": system,
            }));
        }
    }
    for message in &request.messages {
        messages.push(message_to_json(message));
    }

    let mut body = serde_json::json!({
        "model": request.model,
        "messages": messages,
        "stream": request.stream,
    });
    if let Some(max) = model_profile::resolve_max_output(request.max_tokens, &profile) {
        body["max_tokens"] = serde_json::json!(max);
    }
    if let Some(temp) = request.temperature {
        // OpenAI's reasoning models reject a non-default `temperature` with a
        // 400. Unknown models keep `sampling_params: true`, so third-party
        // endpoints are unaffected.
        if profile.sampling_params {
            body["temperature"] = serde_json::json!(temp);
        }
    }
    if let Some(tools) = &request.tools {
        if !tools.is_empty() {
            body["tools"] = serde_json::json!(tools.iter().map(tool_to_json).collect::<Vec<_>>());
            if let Some(choice) = &controls.tool_choice {
                body["tool_choice"] = choice.to_openai();
            }
            if let Some(parallel) = controls.parallel_tool_calls {
                body["parallel_tool_calls"] = serde_json::json!(parallel);
            }
        }
    }
    if let Some(reasoning) = &controls.reasoning {
        if profile.reasoning == ReasoningControl::OpenAiEffort {
            body["reasoning_effort"] = serde_json::json!(reasoning.effort.as_openai_str());
        }
    }
    if request.stream {
        body["stream_options"] = serde_json::json!({ "include_usage": true });
    }
    body
}

/// Serialize one provider message to OpenAI chat-completions message JSON.
///
/// Preserves multi-tool assistant messages and `role: tool` results with `tool_call_id`.
///
/// # Images
///
/// A user message that carries images becomes the multi-part form
/// (`content: [{type:"text"}, {type:"image_url"}, …]`). Messages without images
/// keep the plain string form so the cached prefix stays byte-stable.
///
/// OpenAI's `tool` and `assistant` messages accept text only, so an image on
/// one of those is replaced by a visible note instead of being dropped.
pub fn message_to_json(message: &ProviderMessage) -> serde_json::Value {
    let mut text_parts = Vec::new();
    let mut tool_calls = Vec::new();
    let mut tool_result: Option<(&str, &str)> = None;
    let mut images: Vec<&crate::capabilities::ImageSource> = Vec::new();

    for block in &message.content {
        match block {
            ProviderContentBlock::Text { text } => text_parts.push(text.as_str()),
            ProviderContentBlock::ToolResult {
                tool_call_id,
                content,
                ..
            } => {
                // OpenAI: one tool-result message per tool_call_id.
                tool_result = Some((tool_call_id.as_str(), content.as_str()));
            }
            ProviderContentBlock::ToolCall { id, name, input } => {
                let arguments = match input {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                tool_calls.push(serde_json::json!({
                    "id": id,
                    "type": "function",
                    "function": {
                        "name": name,
                        "arguments": arguments,
                    }
                }));
            }
            ProviderContentBlock::Image { image_url } => images.push(image_url),
        }
    }

    if let Some((tool_call_id, content)) = tool_result {
        let mut body = content.to_string();
        append_image_notes(&mut body, &images, "OpenAI tool messages carry text only");
        return serde_json::json!({
            "role": "tool",
            "tool_call_id": tool_call_id,
            "content": body,
        });
    }

    if !tool_calls.is_empty() {
        let mut body = text_parts.join("\n");
        append_image_notes(
            &mut body,
            &images,
            "OpenAI assistant messages carry text only",
        );
        let content = if body.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::Value::String(body)
        };
        return serde_json::json!({
            "role": "assistant",
            "content": content,
            "tool_calls": tool_calls,
        });
    }

    if images.is_empty() {
        return serde_json::json!({
            "role": message.role,
            "content": text_parts.join("\n"),
        });
    }

    let mut parts = Vec::new();
    let text = text_parts.join("\n");
    if !text.is_empty() {
        parts.push(serde_json::json!({ "type": "text", "text": text }));
    }
    for image in images {
        parts.push(chat_image_part(image));
    }
    serde_json::json!({
        "role": message.role,
        "content": parts,
    })
}

/// Whether OpenAI's `image_url.url` can carry this reference verbatim.
///
/// The API fetches `http(s)` URLs and decodes `data:` URIs; nothing else.
fn openai_accepts_url(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://") || url.starts_with("data:")
}

/// One `image_url` content part, or a visible note when the URL is unusable.
fn chat_image_part(image: &crate::capabilities::ImageSource) -> serde_json::Value {
    if !openai_accepts_url(&image.url) {
        return serde_json::json!({
            "type": "text",
            "text": image.degraded_note("OpenAI accepts only an http(s) URL or a data: URI"),
        });
    }
    let mut image_url = serde_json::json!({ "url": image.url });
    if let Some(detail) = &image.detail {
        image_url["detail"] = serde_json::json!(detail);
    }
    serde_json::json!({ "type": "image_url", "image_url": image_url })
}

/// One Responses `input_image` part, or a visible note when the URL is unusable.
fn responses_image_part(image: &crate::capabilities::ImageSource) -> serde_json::Value {
    if !openai_accepts_url(&image.url) {
        return serde_json::json!({
            "type": "input_text",
            "text": image.degraded_note("OpenAI accepts only an http(s) URL or a data: URI"),
        });
    }
    let mut part = serde_json::json!({ "type": "input_image", "image_url": image.url });
    if let Some(detail) = &image.detail {
        part["detail"] = serde_json::json!(detail);
    }
    part
}

/// Append a degradation note per image to a text-only message body.
///
/// Used where the wire format has no image slot at all; the alternative would be
/// dropping the attachment without telling anyone.
fn append_image_notes(
    body: &mut String,
    images: &[&crate::capabilities::ImageSource],
    reason: &str,
) {
    for image in images {
        if !body.is_empty() {
            body.push('\n');
        }
        body.push_str(&image.degraded_note(reason));
    }
}

fn tool_to_json(tool: &ProviderTool) -> serde_json::Value {
    serde_json::json!({
        "type": "function",
        "function": {
            "name": tool.name,
            "description": tool.description,
            "parameters": tool.input_schema,
        }
    })
}

pub fn retry_after_ms(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    if let Some(value) = headers
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
    {
        if let Ok(seconds) = value.trim().parse::<u64>() {
            return Some(seconds.saturating_mul(1_000));
        }
        if let Ok(at) = DateTime::parse_from_rfc2822(value) {
            return at
                .with_timezone(&Utc)
                .signed_duration_since(Utc::now())
                .num_milliseconds()
                .try_into()
                .ok();
        }
    }
    headers
        .get("x-ratelimit-reset")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<i128>().ok())
        .and_then(|epoch| {
            let now_ms = i128::from(Utc::now().timestamp_millis());
            let reset_ms = if epoch > 10_000_000_000 {
                epoch
            } else {
                epoch.saturating_mul(1_000)
            };
            reset_ms.saturating_sub(now_ms).try_into().ok()
        })
}

pub fn map_http_status(
    status: u16,
    body: &str,
    headers: Option<&reqwest::header::HeaderMap>,
) -> ProviderError {
    let category = match status {
        401 | 403 => ProviderErrorCategory::Auth,
        429 => ProviderErrorCategory::RateLimit,
        404 => ProviderErrorCategory::ModelNotFound,
        400 if body.to_ascii_lowercase().contains("context") => {
            ProviderErrorCategory::ContextLengthExceeded
        }
        400..=499 => ProviderErrorCategory::BadRequest,
        500..=599 => ProviderErrorCategory::ServerError,
        _ => ProviderErrorCategory::Unknown,
    };
    let retryable = matches!(
        category,
        ProviderErrorCategory::RateLimit
            | ProviderErrorCategory::ServerError
            | ProviderErrorCategory::Timeout
            | ProviderErrorCategory::Network
    );

    let retry_after_ms = (status == 429)
        .then(|| headers.and_then(retry_after_ms))
        .flatten();

    ProviderError {
        code: format!("http_{status}"),
        message: redact_http_body(body),
        category,
        retryable,
        retry_after_ms,
    }
}

fn redact_http_body(body: &str) -> String {
    // Keep message short and free of credentials.
    let truncated: String = body.chars().take(400).collect();
    assistant_protocol::v2::redact_secrets(&truncated)
}

/// Stream chat completions from an OpenAI-compatible endpoint.
pub async fn stream_chat_completions(
    client: &Client,
    base_url: &str,
    api_key: &str,
    request: ProviderRequest,
) -> Result<std::pin::Pin<Box<dyn futures_util::Stream<Item = ProviderEvent> + Send>>, ProviderError>
{
    let mut request = request;
    request.stream = true;
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let body = build_chat_completions_body(&request);

    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {api_key}"))
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

    let status = response.status().as_u16();
    if !response.status().is_success() {
        let headers = response.headers().clone();
        let text = response.text().await.unwrap_or_default();
        return Err(map_http_status(status, &text, Some(&headers)));
    }

    let byte_stream = response.bytes_stream();
    let stream = async_stream::stream! {
        let mut parser = OpenAiSseParser::new();
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
            // Emit completed tool-call assemblies if any, then Completed.
            for (id, name, args) in parser.finished_tool_calls() {
                // Final delta with empty args already streamed; nothing extra.
                let _ = (id, name, args);
            }
            yield ProviderEvent::Completed;
        }
    };

    Ok(Box::pin(stream))
}

/// Stream OpenAI Responses API (`POST /v1/responses`) using the shipped SSE parser.
pub async fn stream_responses(
    client: &Client,
    base_url: &str,
    api_key: &str,
    request: ProviderRequest,
) -> Result<std::pin::Pin<Box<dyn futures_util::Stream<Item = ProviderEvent> + Send>>, ProviderError>
{
    let mut request = request;
    request.stream = true;
    let url = format!("{}/responses", base_url.trim_end_matches('/'));
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::AUTHORIZATION,
        reqwest::header::HeaderValue::from_str(&format!("Bearer {api_key}")).map_err(|_| {
            ProviderError {
                code: "invalid_credential".into(),
                message: "invalid OpenAI credential".into(),
                category: ProviderErrorCategory::Auth,
                retryable: false,
                retry_after_ms: None,
            }
        })?,
    );
    stream_responses_with_headers(client, &url, headers, request).await
}

/// Stream a Responses endpoint with caller-provided authentication/identity headers.
///
/// This is used by the Codex OAuth adapter, whose upstream has a different URL and
/// required account headers but emits the same Responses SSE vocabulary.
pub async fn stream_responses_with_headers(
    client: &Client,
    url: &str,
    headers: reqwest::header::HeaderMap,
    mut request: ProviderRequest,
) -> Result<std::pin::Pin<Box<dyn futures_util::Stream<Item = ProviderEvent> + Send>>, ProviderError>
{
    use crate::stream::openai_responses::parse_responses_event;

    request.stream = true;
    let body = build_responses_body(&request);

    let response = client
        .post(url)
        .headers(headers)
        .header("Content-Type", "application/json")
        .header("Accept", "text/event-stream")
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

    let status = response.status().as_u16();
    if !response.status().is_success() {
        let headers = response.headers().clone();
        let text = response.text().await.unwrap_or_default();
        return Err(map_http_status(status, &text, Some(&headers)));
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
                            for event in parse_responses_event(data) {
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

/// Build JSON body for OpenAI Responses API using the request's own
/// [`ProviderRequest::controls`].
pub fn build_responses_body(request: &ProviderRequest) -> serde_json::Value {
    build_responses_body_with_controls(request, &request.controls)
}

/// Build JSON body for OpenAI Responses API with caller-supplied controls,
/// which override [`ProviderRequest::controls`] entirely.
pub fn build_responses_body_with_controls(
    request: &ProviderRequest,
    controls: &RequestControls,
) -> serde_json::Value {
    let profile = model_profile::resolve(&request.model);
    let mut input = Vec::new();
    for message in &request.messages {
        // Responses API input is looser than chat completions; still forward tool structure
        // as role+content text plus function_call / function_call_output items when present.
        let mut text_parts = Vec::new();
        let mut pushed_structured = false;
        let mut images: Vec<&crate::capabilities::ImageSource> = Vec::new();
        for block in &message.content {
            match block {
                ProviderContentBlock::Text { text } => text_parts.push(text.as_str()),
                ProviderContentBlock::ToolCall {
                    id,
                    name,
                    input: tool_input,
                } => {
                    pushed_structured = true;
                    let arguments = match tool_input {
                        serde_json::Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    input.push(serde_json::json!({
                        "type": "function_call",
                        "call_id": id,
                        "name": name,
                        "arguments": arguments,
                    }));
                }
                ProviderContentBlock::ToolResult {
                    tool_call_id,
                    content,
                    ..
                } => {
                    pushed_structured = true;
                    input.push(serde_json::json!({
                        "type": "function_call_output",
                        "call_id": tool_call_id,
                        "output": content,
                    }));
                }
                ProviderContentBlock::Image { image_url } => images.push(image_url),
            }
        }
        if !images.is_empty() {
            // Responses input items take a typed content array; only switch to
            // it when there is an image, so text-only turns keep the cheap
            // string form (and its byte-stable cache prefix).
            let mut parts = Vec::new();
            let text = text_parts.join("\n");
            if !text.is_empty() {
                parts.push(serde_json::json!({ "type": "input_text", "text": text }));
            }
            for image in images {
                parts.push(responses_image_part(image));
            }
            input.push(serde_json::json!({
                "role": message.role,
                "content": parts,
            }));
        } else if !text_parts.is_empty() || !pushed_structured {
            input.push(serde_json::json!({
                "role": message.role,
                "content": text_parts.join("\n"),
            }));
        }
    }
    let mut body = serde_json::json!({
        "model": request.model,
        "input": input,
        "stream": true,
    });
    if let Some(system) = &request.system_prompt {
        if !system.is_empty() {
            body["instructions"] = serde_json::json!(system);
        }
    }
    if let Some(max) = model_profile::resolve_max_output(request.max_tokens, &profile) {
        body["max_output_tokens"] = serde_json::json!(max);
    }
    if let Some(tools) = &request.tools {
        if !tools.is_empty() {
            body["tools"] = serde_json::json!(tools
                .iter()
                .map(|t| serde_json::json!({
                    "type": "function",
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.input_schema,
                }))
                .collect::<Vec<_>>());
            if let Some(choice) = &controls.tool_choice {
                body["tool_choice"] = choice.to_openai();
            }
            if let Some(parallel) = controls.parallel_tool_calls {
                body["parallel_tool_calls"] = serde_json::json!(parallel);
            }
        }
    }
    if let Some(reasoning) = &controls.reasoning {
        if profile.reasoning == ReasoningControl::OpenAiEffort {
            // Responses nests the level under `reasoning`, unlike chat
            // completions' flat `reasoning_effort`.
            body["reasoning"] = serde_json::json!({
                "effort": reasoning.effort.as_openai_str(),
            });
        }
    }
    body
}

/// Non-streaming chat completion.
pub async fn chat_completions(
    client: &Client,
    base_url: &str,
    api_key: &str,
    request: ProviderRequest,
) -> Result<
    (
        String,
        Option<Vec<(String, String, String)>>,
        crate::capabilities::ProviderUsage,
    ),
    ProviderError,
> {
    let mut request = request;
    request.stream = false;
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let body = build_chat_completions_body(&request);

    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .timeout(Duration::from_secs(120))
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

    let status = response.status().as_u16();
    let headers = response.headers().clone();
    let text = response.text().await.map_err(|e| ProviderError {
        code: "network".into(),
        message: e.to_string(),
        category: ProviderErrorCategory::Network,
        retryable: true,
        retry_after_ms: None,
    })?;
    if !(200..300).contains(&status) {
        return Err(map_http_status(status, &text, Some(&headers)));
    }

    let value: serde_json::Value = serde_json::from_str(&text).map_err(|e| ProviderError {
        code: "parse_error".into(),
        message: e.to_string(),
        category: ProviderErrorCategory::ServerError,
        retryable: false,
        retry_after_ms: None,
    })?;

    let content = value["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let mut tool_calls = Vec::new();
    if let Some(arr) = value["choices"][0]["message"]["tool_calls"].as_array() {
        for tc in arr {
            let id = tc["id"].as_str().unwrap_or("").to_string();
            let name = tc["function"]["name"].as_str().unwrap_or("").to_string();
            let args = tc["function"]["arguments"]
                .as_str()
                .unwrap_or("{}")
                .to_string();
            tool_calls.push((id, name, args));
        }
    }
    // Reuse the streaming parser's normalisation so the non-streaming path
    // reports cache tokens with identical semantics.
    let usage = serde_json::from_value::<crate::stream::openai_sse::UsageWire>(
        value["usage"].clone(),
    )
    .map(|wire| wire.to_provider())
    .unwrap_or_default();
    let tools = if tool_calls.is_empty() {
        None
    } else {
        Some(tool_calls)
    };
    Ok((content, tools, usage))
}

#[cfg(test)]
mod tool_message_tests {
    use super::*;
    use crate::capabilities::{
        history_message_to_provider, HistoryMessage, HistoryToolCall, ProviderContentBlock,
        ProviderMessage, ProviderRequest,
    };

    fn multi_turn_history() -> Vec<ProviderMessage> {
        vec![
            history_message_to_provider(HistoryMessage {
                role: "user".into(),
                content: "read a and b".into(),
                tool_call_id: None,
                tool_name: None,
                tool_calls: None,
                images: Vec::new(),
            }),
            history_message_to_provider(HistoryMessage {
                role: "assistant".into(),
                content: String::new(),
                tool_call_id: None,
                tool_name: None,
                tool_calls: Some(vec![
                    HistoryToolCall {
                        id: "call_a".into(),
                        name: "read_file".into(),
                        arguments: r#"{"path":"a.txt"}"#.into(),
                    },
                    HistoryToolCall {
                        id: "call_b".into(),
                        name: "read_file".into(),
                        arguments: r#"{"path":"b.txt"}"#.into(),
                    },
                ]),
                images: Vec::new(),
            }),
            history_message_to_provider(HistoryMessage {
                role: "tool".into(),
                content: "contents of a".into(),
                tool_call_id: Some("call_a".into()),
                tool_name: Some("read_file".into()),
                tool_calls: None,
                images: Vec::new(),
            }),
            history_message_to_provider(HistoryMessage {
                role: "tool".into(),
                content: "contents of b".into(),
                tool_call_id: Some("call_b".into()),
                tool_name: Some("read_file".into()),
                tool_calls: None,
                images: Vec::new(),
            }),
        ]
    }

    #[test]
    fn openai_body_preserves_multi_tool_calls_and_tool_results() {
        let body = build_chat_completions_body(&ProviderRequest {
            model: "gpt-4o".into(),
            messages: multi_turn_history(),
            system_prompt: None,
            tools: None,
            max_tokens: Some(100),
            temperature: None,
            stream: true,
            structured_output: None,
            controls: Default::default(),
        });
        let messages = body["messages"].as_array().expect("messages array");
        assert_eq!(messages.len(), 4);

        let assistant = &messages[1];
        assert_eq!(assistant["role"], "assistant");
        let tool_calls = assistant["tool_calls"].as_array().expect("tool_calls");
        assert_eq!(tool_calls.len(), 2);
        assert_eq!(tool_calls[0]["id"], "call_a");
        assert_eq!(tool_calls[1]["id"], "call_b");
        assert_eq!(tool_calls[0]["function"]["name"], "read_file");

        assert_eq!(messages[2]["role"], "tool");
        assert_eq!(messages[2]["tool_call_id"], "call_a");
        assert_eq!(messages[2]["content"], "contents of a");
        assert_eq!(messages[3]["tool_call_id"], "call_b");

        // Must not flatten tool structure into a single text blob.
        let wire = body.to_string();
        assert!(!wire.contains(r#""role":"assistant","content":"call_a"#));
    }

    fn image_message(image: crate::capabilities::ImageSource) -> ProviderMessage {
        ProviderMessage {
            role: "user".into(),
            content: vec![
                ProviderContentBlock::Text {
                    text: "what is this".into(),
                },
                ProviderContentBlock::Image { image_url: image },
            ],
        }
    }

    fn image_request(image: crate::capabilities::ImageSource) -> ProviderRequest {
        ProviderRequest {
            model: "gpt-4o".into(),
            messages: vec![image_message(image)],
            system_prompt: None,
            tools: None,
            max_tokens: Some(256),
            temperature: None,
            stream: true,
            structured_output: None,
            controls: Default::default(),
        }
    }

    #[test]
    fn chat_completions_user_image_becomes_an_image_url_part() {
        let body = build_chat_completions_body(&image_request(
            crate::capabilities::ImageSource {
                url: "https://example.test/cat.png".into(),
                detail: Some("high".into()),
                media_type: None,
            },
        ));
        let content = body["messages"][0]["content"].as_array().unwrap();
        assert_eq!(content.len(), 2);
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[1]["type"], "image_url");
        assert_eq!(content[1]["image_url"]["url"], "https://example.test/cat.png");
        assert_eq!(content[1]["image_url"]["detail"], "high");
    }

    #[test]
    fn chat_completions_data_uri_is_forwarded_verbatim() {
        let body = build_chat_completions_body(&image_request(
            crate::capabilities::ImageSource::new("data:image/png;base64,AAAB"),
        ));
        assert_eq!(
            body["messages"][0]["content"][1]["image_url"]["url"],
            "data:image/png;base64,AAAB"
        );
    }

    #[test]
    fn chat_completions_keeps_the_plain_string_form_without_images() {
        let body = build_chat_completions_body(&ProviderRequest {
            messages: vec![ProviderMessage {
                role: "user".into(),
                content: vec![ProviderContentBlock::Text { text: "hi".into() }],
            }],
            ..image_request(crate::capabilities::ImageSource::new("data:image/png;base64,A"))
        });
        assert_eq!(body["messages"][0]["content"], "hi");
    }

    #[test]
    fn chat_completions_unsupported_image_scheme_is_announced_not_dropped() {
        let body = build_chat_completions_body(&image_request(
            crate::capabilities::ImageSource::new("gs://bucket/cat.png"),
        ));
        let part = &body["messages"][0]["content"][1];
        assert_eq!(part["type"], "text");
        assert!(part["text"]
            .as_str()
            .unwrap()
            .contains("image not sent to the model"));
    }

    #[test]
    fn chat_completions_tool_message_announces_an_image_it_cannot_carry() {
        // OpenAI tool messages are text-only. Silently dropping the attachment
        // is what this note replaces.
        let json = message_to_json(&ProviderMessage {
            role: "tool".into(),
            content: vec![
                ProviderContentBlock::ToolResult {
                    tool_call_id: "t1".into(),
                    content: "captured".into(),
                    name: Some("screenshot".into()),
                },
                ProviderContentBlock::Image {
                    image_url: crate::capabilities::ImageSource::new("data:image/png;base64,AAAB"),
                },
            ],
        });
        assert_eq!(json["role"], "tool");
        let content = json["content"].as_str().unwrap();
        assert!(content.starts_with("captured"), "{content}");
        assert!(content.contains("image not sent to the model"), "{content}");
    }

    #[test]
    fn responses_user_image_becomes_an_input_image_part() {
        let body = build_responses_body(&image_request(crate::capabilities::ImageSource {
            url: "https://example.test/cat.png".into(),
            detail: Some("low".into()),
            media_type: None,
        }));
        let content = body["input"][0]["content"].as_array().unwrap();
        assert_eq!(content.len(), 2);
        assert_eq!(content[0]["type"], "input_text");
        assert_eq!(content[1]["type"], "input_image");
        assert_eq!(content[1]["image_url"], "https://example.test/cat.png");
        assert_eq!(content[1]["detail"], "low");
    }

    #[test]
    fn responses_keeps_the_plain_string_form_without_images() {
        let body = build_responses_body(&ProviderRequest {
            messages: vec![ProviderMessage {
                role: "user".into(),
                content: vec![ProviderContentBlock::Text { text: "hi".into() }],
            }],
            ..image_request(crate::capabilities::ImageSource::new("data:image/png;base64,A"))
        });
        assert_eq!(body["input"][0]["content"], "hi");
    }

    #[test]
    fn message_to_json_single_tool_result() {
        let msg = ProviderMessage {
            role: "tool".into(),
            content: vec![ProviderContentBlock::ToolResult {
                tool_call_id: "t1".into(),
                content: "ok".into(),
                name: Some("echo".into()),
            }],
        };
        let json = message_to_json(&msg);
        assert_eq!(json["role"], "tool");
        assert_eq!(json["tool_call_id"], "t1");
        assert_eq!(json["content"], "ok");
    }

    #[test]
    fn retry_after_parses_seconds_http_date_and_reset_epoch() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(reqwest::header::RETRY_AFTER, "3".parse().unwrap());
        assert_eq!(retry_after_ms(&headers), Some(3_000));

        headers.insert(
            reqwest::header::RETRY_AFTER,
            (Utc::now() + chrono::Duration::seconds(3))
                .to_rfc2822()
                .parse()
                .unwrap(),
        );
        assert!(matches!(retry_after_ms(&headers), Some(ms) if (1_000..=3_000).contains(&ms)));

        headers.remove(reqwest::header::RETRY_AFTER);
        headers.insert(
            "x-ratelimit-reset",
            (Utc::now().timestamp() + 3).to_string().parse().unwrap(),
        );
        assert!(matches!(retry_after_ms(&headers), Some(ms) if (2_000..=3_000).contains(&ms)));
    }

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

    fn with_tool(model: &str) -> ProviderRequest {
        let mut request = plain(model);
        request.tools = Some(vec![crate::capabilities::ProviderTool {
            name: "read_file".into(),
            description: Some("read a file".into()),
            input_schema: serde_json::json!({"type": "object"}),
        }]);
        request
    }

    #[test]
    fn chat_completions_max_tokens_comes_from_the_model_profile() {
        assert_eq!(
            build_chat_completions_body(&plain("gpt-4o"))["max_tokens"],
            16_384
        );
        // Unknown model: omit the field entirely and let the provider default
        // stand, exactly as before.
        assert!(build_chat_completions_body(&plain("some-local-llm"))
            .get("max_tokens")
            .is_none());
        // Explicit values are clamped down, never raised.
        let mut huge = plain("gpt-4o");
        huge.max_tokens = Some(999_999);
        assert_eq!(build_chat_completions_body(&huge)["max_tokens"], 16_384);
    }

    #[test]
    fn temperature_is_dropped_for_openai_reasoning_models_only() {
        let mut gpt4o = plain("gpt-4o");
        gpt4o.temperature = Some(0.3);
        assert_eq!(build_chat_completions_body(&gpt4o)["temperature"], 0.3);

        let mut o3 = plain("o3-mini");
        o3.temperature = Some(0.3);
        assert!(build_chat_completions_body(&o3).get("temperature").is_none());

        // Unknown third-party models keep sampling parameters.
        let mut local = plain("qwen2.5-coder");
        local.temperature = Some(0.3);
        assert_eq!(build_chat_completions_body(&local)["temperature"], 0.3);
    }

    #[test]
    fn tool_choice_and_parallel_flag_encode_to_the_openai_shape() {
        let request = with_tool("gpt-4o");

        let forced = build_chat_completions_body_with_controls(
            &request,
            &RequestControls {
                tool_choice: Some(crate::capabilities::ToolChoice::Tool {
                    name: "read_file".into(),
                }),
                parallel_tool_calls: Some(false),
                ..Default::default()
            },
        );
        assert_eq!(forced["tool_choice"]["type"], "function");
        assert_eq!(forced["tool_choice"]["function"]["name"], "read_file");
        assert_eq!(forced["parallel_tool_calls"], false);

        // OpenAI spells "must call something" as the bare string "required".
        let required = build_chat_completions_body_with_controls(
            &request,
            &RequestControls {
                tool_choice: Some(crate::capabilities::ToolChoice::Required),
                ..Default::default()
            },
        );
        assert_eq!(required["tool_choice"], "required");

        // Default controls change nothing on the wire.
        let body = build_chat_completions_body(&request);
        assert!(body.get("tool_choice").is_none());
        assert!(body.get("parallel_tool_calls").is_none());
    }

    #[test]
    fn reasoning_effort_only_reaches_models_that_accept_it() {
        let controls = RequestControls {
            reasoning: Some(crate::capabilities::ReasoningRequest::new(
                crate::capabilities::ReasoningEffort::High,
            )),
            ..Default::default()
        };
        assert_eq!(
            build_chat_completions_body_with_controls(&plain("o3-mini"), &controls)
                ["reasoning_effort"],
            "high"
        );
        // gpt-4o has no reasoning knob; sending one is a 400.
        assert!(
            build_chat_completions_body_with_controls(&plain("gpt-4o"), &controls)
                .get("reasoning_effort")
                .is_none()
        );

        // The Responses API nests it instead of using a flat field.
        let responses = build_responses_body_with_controls(&plain("o3"), &controls);
        assert_eq!(responses["reasoning"]["effort"], "high");
        assert_eq!(responses["max_output_tokens"], 100_000);
    }

    #[test]
    fn rate_limit_error_includes_retry_after_when_available() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(reqwest::header::RETRY_AFTER, "2".parse().unwrap());
        let error = map_http_status(429, "too many requests", Some(&headers));
        assert_eq!(error.category, ProviderErrorCategory::RateLimit);
        assert_eq!(error.retry_after_ms, Some(2_000));
        assert!(serde_json::to_string(&error).is_ok());
    }
}
