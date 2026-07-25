//! Shared HTTP streaming helper for OpenAI-compatible chat completions.

use crate::capabilities::{
    ProviderContentBlock, ProviderError, ProviderErrorCategory, ProviderMessage, ProviderRequest,
    ProviderTool,
};
use crate::stream::{split_sse_lines, sse_data_payload, OpenAiSseParser, ProviderEvent};
use chrono::{DateTime, Utc};
use futures_util::StreamExt;
use reqwest::Client;
use std::time::Duration;

/// Build the JSON body for OpenAI chat completions.
pub fn build_chat_completions_body(request: &ProviderRequest) -> serde_json::Value {
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
    if let Some(max) = request.max_tokens {
        body["max_tokens"] = serde_json::json!(max);
    }
    if let Some(temp) = request.temperature {
        body["temperature"] = serde_json::json!(temp);
    }
    if let Some(tools) = &request.tools {
        if !tools.is_empty() {
            body["tools"] = serde_json::json!(tools.iter().map(tool_to_json).collect::<Vec<_>>());
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
pub fn message_to_json(message: &ProviderMessage) -> serde_json::Value {
    let mut text_parts = Vec::new();
    let mut tool_calls = Vec::new();
    let mut tool_result: Option<(&str, &str)> = None;

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
            ProviderContentBlock::Image { .. } => {}
        }
    }

    if let Some((tool_call_id, content)) = tool_result {
        return serde_json::json!({
            "role": "tool",
            "tool_call_id": tool_call_id,
            "content": content,
        });
    }

    if !tool_calls.is_empty() {
        let content = if text_parts.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::Value::String(text_parts.join("\n"))
        };
        return serde_json::json!({
            "role": "assistant",
            "content": content,
            "tool_calls": tool_calls,
        });
    }

    serde_json::json!({
        "role": message.role,
        "content": text_parts.join("\n"),
    })
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

/// Build JSON body for OpenAI Responses API.
pub fn build_responses_body(request: &ProviderRequest) -> serde_json::Value {
    let mut input = Vec::new();
    for message in &request.messages {
        // Responses API input is looser than chat completions; still forward tool structure
        // as role+content text plus function_call / function_call_output items when present.
        let mut text_parts = Vec::new();
        let mut pushed_structured = false;
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
                ProviderContentBlock::Image { .. } => {}
            }
        }
        if !text_parts.is_empty() || !pushed_structured {
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
    if let Some(max) = request.max_tokens {
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
    let usage = crate::capabilities::ProviderUsage {
        input_tokens: value["usage"]["prompt_tokens"].as_u64().unwrap_or(0),
        output_tokens: value["usage"]["completion_tokens"].as_u64().unwrap_or(0),
        reasoning_tokens: value["usage"]["completion_tokens_details"]["reasoning_tokens"].as_u64(),
        cost_usd: None,
    };
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
            }),
            history_message_to_provider(HistoryMessage {
                role: "tool".into(),
                content: "contents of a".into(),
                tool_call_id: Some("call_a".into()),
                tool_name: Some("read_file".into()),
                tool_calls: None,
            }),
            history_message_to_provider(HistoryMessage {
                role: "tool".into(),
                content: "contents of b".into(),
                tool_call_id: Some("call_b".into()),
                tool_name: Some("read_file".into()),
                tool_calls: None,
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
