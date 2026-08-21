//! HTTP transport for OpenAI-compatible APIs.
//!
//! Extracted from `http_stream.rs`: streaming (chat completions / Responses)
//! and non-streaming calls plus status/retry/redaction helpers.

use crate::capabilities::{ProviderError, ProviderErrorCategory, ProviderRequest};
use crate::stream::{split_sse_lines, sse_data_payload, OpenAiSseParser, ProviderEvent};
use chrono::{DateTime, Utc};
use futures_util::StreamExt;
use reqwest::Client;
use std::time::Duration;

use super::body::{build_chat_completions_body, build_responses_body};

const MAX_SSE_BUFFER_BYTES: usize = 1024 * 1024;

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

pub fn transport_error(code: &str, error: &reqwest::Error) -> ProviderError {
    let (message, category) = if error.is_timeout() {
        ("provider request timed out", ProviderErrorCategory::Timeout)
    } else if error.is_connect() {
        ("provider connection failed", ProviderErrorCategory::Network)
    } else if error.is_decode() {
        (
            "provider response decode failed",
            ProviderErrorCategory::ServerError,
        )
    } else {
        (
            "provider network request failed",
            ProviderErrorCategory::Network,
        )
    };

    ProviderError {
        code: code.into(),
        message: message.into(),
        category,
        retryable: true,
        retry_after_ms: None,
    }
}

pub(crate) fn incomplete_stream_error() -> ProviderError {
    ProviderError {
        code: "incomplete_stream".into(),
        message: "provider stream ended before a terminal event".into(),
        category: ProviderErrorCategory::ServerError,
        retryable: true,
        retry_after_ms: None,
    }
}

pub(crate) fn append_sse_bytes(buffer: &mut String, bytes: &[u8]) -> Result<(), ProviderError> {
    if buffer.len().saturating_add(bytes.len()) > MAX_SSE_BUFFER_BYTES {
        return Err(ProviderError {
            code: "stream_buffer_exceeded".into(),
            message: "provider stream event exceeded the size limit".into(),
            category: ProviderErrorCategory::ServerError,
            retryable: false,
            retry_after_ms: None,
        });
    }
    buffer.push_str(&String::from_utf8_lossy(bytes));
    Ok(())
}

fn redact_http_body(body: &str) -> String {
    // Keep message short and free of credentials.
    let truncated: String = body.chars().take(400).collect();
    crate::redact::redact_secrets(&truncated)
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
        .map_err(|error| transport_error("network", &error))?;

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
        tokio::pin!(byte_stream);
        while let Some(chunk) = byte_stream.next().await {
            match chunk {
                Ok(bytes) => {
                    if let Err(error) = append_sse_bytes(&mut buffer, &bytes) {
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
                    yield ProviderEvent::Error(transport_error("stream_error", &err));
                    return;
                }
            }
        }
        yield ProviderEvent::Error(incomplete_stream_error());
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
    use crate::stream::openai_responses::OpenAiResponsesParser;

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
        .map_err(|error| transport_error("network", &error))?;

    let status = response.status().as_u16();
    if !response.status().is_success() {
        let headers = response.headers().clone();
        let text = response.text().await.unwrap_or_default();
        return Err(map_http_status(status, &text, Some(&headers)));
    }

    let byte_stream = response.bytes_stream();
    let stream = async_stream::stream! {
        let mut buffer = String::new();
        let mut parser = OpenAiResponsesParser::new();
        tokio::pin!(byte_stream);
        while let Some(chunk) = byte_stream.next().await {
            match chunk {
                Ok(bytes) => {
                    if let Err(error) = append_sse_bytes(&mut buffer, &bytes) {
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
                    yield ProviderEvent::Error(transport_error("stream_error", &err));
                    return;
                }
            }
        }
        yield ProviderEvent::Error(incomplete_stream_error());
    };

    Ok(Box::pin(stream))
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
        .map_err(|error| transport_error("network", &error))?;

    let status = response.status().as_u16();
    let headers = response.headers().clone();
    let text = response
        .text()
        .await
        .map_err(|error| transport_error("network", &error))?;
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
    let usage =
        serde_json::from_value::<crate::stream::openai_sse::UsageWire>(value["usage"].clone())
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
mod tests {
    use super::*;

    #[tokio::test]
    async fn transport_timeouts_are_not_network_errors() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _stream = listener.accept().await.unwrap().0;
            tokio::time::sleep(Duration::from_millis(50)).await;
        });

        let error = Client::builder()
            .timeout(Duration::from_millis(5))
            .build()
            .unwrap()
            .get(format!("http://{address}"))
            .send()
            .await
            .unwrap_err();
        assert_eq!(
            transport_error("network", &error).category,
            ProviderErrorCategory::Timeout
        );
    }

    #[tokio::test]
    async fn transport_errors_never_expose_request_urls_or_query_secrets() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);

        let error = Client::new()
            .get(format!(
                "http://{address}/models/example?alt=sse&key=query-secret"
            ))
            .send()
            .await
            .unwrap_err();
        let provider_error = transport_error("network", &error);

        assert!(!provider_error.message.contains("query-secret"));
        assert!(!provider_error.message.contains("http://"));
        assert_eq!(provider_error.message, "provider connection failed");
    }

    #[test]
    fn sse_remainder_is_bounded() {
        let mut buffer = String::new();
        let oversized = vec![b'a'; MAX_SSE_BUFFER_BYTES + 1];
        let error = append_sse_bytes(&mut buffer, &oversized).unwrap_err();

        assert_eq!(error.code, "stream_buffer_exceeded");
        assert!(buffer.is_empty());
    }
}
