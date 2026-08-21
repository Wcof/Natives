//! Host 内嵌 Native ProxyEngine（ADR-0020 §4 / P0-A 生产接入）。
//!
//! 复用 provider-adapters 的三协议 codec / SSE parser / transport：
//! - `openai_chat_completions` → `http_stream::stream_chat_completions`
//! - `openai_responses`        → `http_stream::stream_responses`
//! - `anthropic_messages`      → `AnthropicAdapter::stream`
//!
//! Secret 经 `secrets::SecretStore`（OS Keychain）的 `secret_ref` 解析，
//! 请求体为规范化 `ProviderRequest` JSON。事件流被聚合为
//! `EngineOutcome`（usage + stop reason），错误映射为可分类的
//! `EngineError`（不携带 Secret / 原始响应体）。

use async_trait::async_trait;
use futures_util::StreamExt;

use crate::proxy::engine::{EngineCall, EngineError, EngineOutcome, ProxyEngine};
use crate::secrets::store::{SecretRef, SecretStore};
use provider_adapters::adapter::ProviderAdapter;
use provider_adapters::capabilities::{
    Credential as ProviderCredential, ProviderErrorCategory, ProviderRequest,
};
use provider_adapters::http_stream;
use provider_adapters::providers::anthropic::AnthropicAdapter;
use provider_adapters::stream::{ProviderEvent, ProviderStopReason};

/// Host 内嵌 Engine：协议转换/传输/usage 归一化全部复用 provider-adapters。
pub struct NativeProxyEngine {
    store: std::sync::Arc<dyn SecretStore>,
    client: reqwest::Client,
}

impl NativeProxyEngine {
    pub fn new(store: std::sync::Arc<dyn SecretStore>) -> Self {
        let client =
            provider_adapters::http_client::client(None).unwrap_or_else(|_| reqwest::Client::new());
        Self { store, client }
    }

    /// 测试/诊断用：可注入自定义 HTTP client。
    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn with_client(
        store: std::sync::Arc<dyn SecretStore>,
        client: reqwest::Client,
    ) -> Self {
        Self { store, client }
    }

    fn resolve_api_key(&self, reference: &SecretRef) -> Result<String, EngineError> {
        let bytes = self
            .store
            .read(reference)
            .map_err(|e| EngineError::new("secret", "secret_unavailable", false, format!("{e}")))?;
        String::from_utf8(bytes).map_err(|_| {
            EngineError::new(
                "secret",
                "secret_invalid_encoding",
                false,
                "key is not UTF-8",
            )
        })
    }
}

fn category_str(category: ProviderErrorCategory) -> &'static str {
    match category {
        ProviderErrorCategory::Auth => "auth",
        ProviderErrorCategory::RateLimit => "rate_limit",
        ProviderErrorCategory::QuotaExceeded => "quota_exceeded",
        ProviderErrorCategory::ModelNotFound => "model_not_found",
        ProviderErrorCategory::ContextLengthExceeded => "context_length_exceeded",
        ProviderErrorCategory::BadRequest => "bad_request",
        ProviderErrorCategory::ServerError => "server_error",
        ProviderErrorCategory::Timeout => "timeout",
        ProviderErrorCategory::Network => "network",
        ProviderErrorCategory::Unknown => "unknown",
    }
}

fn stop_reason_str(reason: ProviderStopReason) -> String {
    match reason {
        ProviderStopReason::Stop => "stop".to_string(),
        ProviderStopReason::ToolUse => "tool_use".to_string(),
        ProviderStopReason::Length => "length".to_string(),
        ProviderStopReason::Cancelled => "cancelled".to_string(),
        ProviderStopReason::Error => "error".to_string(),
        ProviderStopReason::Unknown(other) => other,
    }
}

#[async_trait]
impl ProxyEngine for NativeProxyEngine {
    fn name(&self) -> &'static str {
        "host-native"
    }

    async fn call(&self, request: EngineCall) -> Result<EngineOutcome, EngineError> {
        let api_key = self.resolve_api_key(&request.secret_ref)?;
        let provider_request: ProviderRequest = serde_json::from_str(&request.request_json)
            .map_err(|e| {
                EngineError::new("bad_request", "invalid_request_json", false, format!("{e}"))
            })?;

        // 按目标协议分发到 provider-adapters transport。
        let mut stream = match request.protocol.as_str() {
            "openai_chat_completions" => {
                http_stream::stream_chat_completions(
                    &self.client,
                    &request.base_url,
                    &api_key,
                    provider_request,
                )
                .await
            }
            "openai_responses" => {
                http_stream::stream_responses(
                    &self.client,
                    &request.base_url,
                    &api_key,
                    provider_request,
                )
                .await
            }
            "anthropic_messages" => {
                let adapter = AnthropicAdapter::new();
                let credential = ProviderCredential {
                    api_key,
                    base_url: Some(request.base_url.clone()),
                    proxy_url: None,
                    key_id: None,
                    provider_type: Some("anthropic".into()),
                    project_id: None,
                };
                adapter.stream(provider_request, credential).await
            }
            other => {
                return Err(EngineError::new(
                    "unsupported",
                    "protocol_unsupported",
                    false,
                    format!("unsupported protocol: {other}"),
                ))
            }
        }
        .map_err(|e| EngineError::new(category_str(e.category), e.code, e.retryable, e.message))?;

        // 聚合事件：usage + stop reason（工具/文本增量在本层不展开，
        // 由上层 ProxyService 按事件流消费；此处为确定性聚合结果）。
        let mut usage_input = 0_u64;
        let mut usage_output = 0_u64;
        let mut stop_reason = String::from("unknown");
        while let Some(event) = stream.next().await {
            match event {
                ProviderEvent::Usage(usage) => {
                    usage_input = usage.input_tokens;
                    usage_output = usage.output_tokens;
                }
                ProviderEvent::Completed { reason } => {
                    stop_reason = stop_reason_str(reason);
                }
                ProviderEvent::Error(e) => {
                    return Err(EngineError::new(
                        category_str(e.category),
                        e.code,
                        e.retryable,
                        e.message,
                    ))
                }
                _ => {}
            }
        }

        Ok(EngineOutcome::Completed {
            usage_input_tokens: usage_input,
            usage_output_tokens: usage_output,
            stop_reason,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::store::MemorySecretStore;
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    fn request_json() -> String {
        serde_json::json!({
            "model": "fixture-model",
            "messages": [{ "role": "user", "content": [{ "text": "hi" }] }],
            "system_prompt": null,
            "tools": null,
            "max_tokens": 16,
            "temperature": null,
            "stream": true,
            "structured_output": null,
            "controls": {}
        })
        .to_string()
    }

    async fn read_request(socket: &mut TcpStream) {
        let mut request = Vec::new();
        let mut buffer = [0_u8; 4096];
        let (header_end, content_length) = loop {
            let count = socket.read(&mut buffer).await.expect("read request");
            assert!(count > 0, "client closed before sending request headers");
            request.extend_from_slice(&buffer[..count]);
            if let Some(offset) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                let header_end = offset + 4;
                let headers = String::from_utf8_lossy(&request[..header_end]);
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        line.strip_prefix("content-length:")
                            .or_else(|| line.strip_prefix("Content-Length:"))
                    })
                    .and_then(|value| value.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                break (header_end, content_length);
            }
        };
        while request.len() < header_end + content_length {
            let count = socket.read(&mut buffer).await.expect("read request body");
            assert!(count > 0, "client closed before sending request body");
            request.extend_from_slice(&buffer[..count]);
        }
    }

    /// 本地 TCP fixture server：验证 NativeProxyEngine 走真实 Chat Completions
    /// SSE 传输，聚合 usage + stop reason（复用 P0-A fixture 语义）。
    #[tokio::test]
    async fn native_engine_streams_chat_completions_from_local_fixture() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            read_request(&mut socket).await;
            let payload =
                "data: {\"choices\":[{\"delta\":{\"content\":\"hello\"},\"finish_reason\":null}]}\n\n\
                 data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":5}}\n\n\
                 data: [DONE]\n\n";
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                payload.len(),
                payload
            );
            socket.write_all(response.as_bytes()).await.unwrap();
        });

        let store = Arc::new(MemorySecretStore::new());
        let reference = SecretRef::new("cred:provider:p1:k1");
        store.write(&reference, b"fixture-secret").unwrap();
        let engine = NativeProxyEngine::new(store);

        let outcome = engine
            .call(EngineCall {
                protocol: "openai_chat_completions".into(),
                base_url: format!("http://{address}"),
                secret_ref: reference,
                model: "fixture-model".into(),
                request_json: request_json(),
            })
            .await
            .expect("engine call");

        assert_eq!(
            outcome,
            EngineOutcome::Completed {
                usage_input_tokens: 10,
                usage_output_tokens: 5,
                stop_reason: "stop".into()
            }
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn native_engine_rejects_unsupported_protocol() {
        let store = Arc::new(MemorySecretStore::new());
        let reference = SecretRef::new("cred:provider:p1:k1");
        store.write(&reference, b"key").unwrap();
        let engine = NativeProxyEngine::new(store);
        let error = engine
            .call(EngineCall {
                protocol: "unknown_protocol".into(),
                base_url: "http://127.0.0.1:1".into(),
                secret_ref: reference,
                model: "m".into(),
                request_json: request_json(),
            })
            .await
            .unwrap_err();
        assert_eq!(error.category, "unsupported");
        assert_eq!(error.code, "protocol_unsupported");
    }

    #[tokio::test]
    async fn native_engine_missing_secret_fails_closed() {
        let store = Arc::new(MemorySecretStore::new());
        let engine = NativeProxyEngine::new(store);
        let error = engine
            .call(EngineCall {
                protocol: "openai_chat_completions".into(),
                base_url: "http://127.0.0.1:1".into(),
                secret_ref: SecretRef::new("missing"),
                model: "m".into(),
                request_json: request_json(),
            })
            .await
            .unwrap_err();
        assert_eq!(error.category, "secret");
        assert!(!error.retryable);
    }

    /// 把 SSE fixture 内容作为 HTTP chunked 响应发回，验证 Engine 真实解析。
    async fn serve_sse_fixture(payload: String) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            read_request(&mut socket).await;
            let headers =
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\nconnection: close\r\n\r\n";
            socket.write_all(headers.as_bytes()).await.unwrap();
            let chunk = format!("{:x}\r\n{payload}\r\n", payload.len());
            socket.write_all(chunk.as_bytes()).await.unwrap();
            socket.write_all(b"0\r\n\r\n").await.unwrap();
            socket.flush().await.unwrap();
        });
        let _ = server;
        format!("http://{address}")
    }

    /// anthropic_messages：真实 fixture 驱动，聚合 usage + tool_use stop reason。
    #[tokio::test]
    async fn native_engine_anthropic_messages_fixture() {
        let fixture =
            include_str!("../../../crates/provider-adapters/tests/fixtures/anthropic-messages.sse");
        let base_url = serve_sse_fixture(fixture.to_string()).await;

        let store = Arc::new(MemorySecretStore::new());
        let reference = SecretRef::new("cred:provider:p1:k1");
        store.write(&reference, b"fixture-secret").unwrap();
        let engine = NativeProxyEngine::new(store);

        let outcome = engine
            .call(EngineCall {
                protocol: "anthropic_messages".into(),
                base_url,
                secret_ref: reference,
                model: "claude-sonnet-4".into(),
                request_json: request_json(),
            })
            .await
            .expect("engine call");

        assert_eq!(
            outcome,
            EngineOutcome::Completed {
                usage_input_tokens: 40,
                usage_output_tokens: 18,
                stop_reason: "tool_use".into()
            }
        );
    }

    /// openai_responses：真实 fixture 驱动，聚合 usage + tool_use stop reason。
    #[tokio::test]
    async fn native_engine_openai_responses_fixture() {
        let fixture =
            include_str!("../../../crates/provider-adapters/tests/fixtures/openai-responses.sse");
        let base_url = serve_sse_fixture(fixture.to_string()).await;

        let store = Arc::new(MemorySecretStore::new());
        let reference = SecretRef::new("cred:provider:p1:k1");
        store.write(&reference, b"fixture-secret").unwrap();
        let engine = NativeProxyEngine::new(store);

        let outcome = engine
            .call(EngineCall {
                protocol: "openai_responses".into(),
                base_url,
                secret_ref: reference,
                model: "gpt-4o".into(),
                request_json: request_json(),
            })
            .await
            .expect("engine call");

        assert_eq!(
            outcome,
            EngineOutcome::Completed {
                // Responses fixture: input_tokens=120, cached=80 → parser 归一化
                // input_tokens 为 40（非缓存部分），与 protocol_stream_fixtures
                // 的既有断言（input_tokens==40, total_prompt_tokens==120）一致。
                usage_input_tokens: 40,
                usage_output_tokens: 18,
                stop_reason: "tool_use".into()
            }
        );
    }
}
