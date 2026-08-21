//! Antigravity (Google Cloud Code) adapter — OAuth Bearer + `v1internal` endpoint.
//!
//! Antigravity is Google's Gemini-for-Code surface. Unlike the public Gemini
//! API (`generativelanguage.googleapis.com` + `?key=`), it runs on the Cloud
//! Code host (`cloudcode-pa.googleapis.com`) with an OAuth `Authorization:
//! Bearer` token and an optional `x-goog-cloud-target-resource` project header.
//! The request/response body is the same `generateContent` shape, so this
//! reuses `gemini::build_generate_body` and the shared Gemini SSE parser.

use crate::adapter::{ProviderAdapter, ProviderStreamEvent};
use crate::capabilities::*;
use crate::provider_identity::{ModelCapabilities, ProviderType};
use crate::stream::{parse_gemini_chunk, split_sse_lines, sse_data_payload, ProviderEvent};
use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::Client;
use std::time::Duration;

const DEFAULT_BASE: &str = "https://cloudcode-pa.googleapis.com";
/// Daily host is the primary Cloud Code surface used by CLIProxyAPI (cpa-core);
/// `cloudcode-pa` is the non-daily fallback in its candidate chain. We keep
/// `cloudcode-pa` as the default base (streaming contract was reverse-engineered
/// from cpa-core against it) and try the daily host as a discovery fallback.
const DAILY_BASE: &str = "https://daily-cloudcode-pa.googleapis.com";
/// User-Agent matching cpa-core's antigravity CLI so Google-side checks accept
/// the request (quotaService in EasyCLIProxyAPI uses the same UA).
const ANTIGRAVITY_UA: &str = "antigravity/cli/1.0.13 (aidev_client; os_type=darwin; arch=arm64)";
const STREAM_TIMEOUT: Duration = Duration::from_secs(300);
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(15);

pub struct AntigravityAdapter {
    base_url: String,
    client: Client,
}

impl AntigravityAdapter {
    pub fn new() -> Self {
        AntigravityAdapter {
            base_url: DEFAULT_BASE.to_string(),
            client: Client::new(),
        }
    }

    pub fn with_base_url(mut self, url: String) -> Self {
        self.base_url = url;
        self
    }

    /// Real model discovery via the Cloud Code `v1internal:fetchAvailableModels`
    /// verb (same host as streaming). Falls back to the hardcoded list when the
    /// endpoint is unreachable, auth fails, or the payload shape is unknown —
    /// the renderer must never hard-fail on a model refresh (offline-safe).
    async fn fetch_available_models(
        &self,
        credential: &Credential,
    ) -> Result<Vec<ModelInfo>, ProviderError> {
        let token = credential.api_key.trim().to_string();
        if token.is_empty() {
            return Err(ProviderError {
                code: "missing_key".into(),
                message: "Antigravity OAuth access token required".into(),
                category: ProviderErrorCategory::Auth,
                retryable: false,
                retry_after_ms: None,
            });
        }
        let base = credential
            .base_url
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(&self.base_url);
        let mut candidates = vec![base.trim_end_matches('/').to_string()];
        if base != DAILY_BASE {
            candidates.push(DAILY_BASE.to_string());
        }
        let client = match credential.proxy_url.as_deref() {
            Some(url) if !url.trim().is_empty() => crate::http_client::client(Some(url))?,
            _ => self.client.clone(),
        };
        let mut last_error: Option<ProviderError> = None;
        for host in candidates {
            let url = format!("{host}/v1internal:fetchAvailableModels");
            let mut req = client
                .post(&url)
                .header("Authorization", format!("Bearer {token}"))
                .header("Content-Type", "application/json")
                .header("User-Agent", ANTIGRAVITY_UA);
            if let Some(project_id) = credential.project_id.as_deref() {
                if !project_id.trim().is_empty() {
                    req = req.header("x-goog-cloud-target-resource", project_id);
                }
            }
            let response = req.timeout(DISCOVERY_TIMEOUT).send().await;
            let response = match response {
                Ok(resp) if resp.status().is_success() => resp,
                Ok(resp) => {
                    last_error = Some(crate::http_stream::map_http_status(
                        resp.status().as_u16(),
                        &resp.text().await.unwrap_or_default(),
                        None,
                    ));
                    continue;
                }
                Err(error) => {
                    last_error = Some(crate::http_stream::transport_error("network", &error));
                    continue;
                }
            };
            return parse_available_models(response.json::<serde_json::Value>().await.map_err(
                |_| ProviderError {
                    code: "bad_discovery_response".into(),
                    message: "fetchAvailableModels returned an invalid JSON body".into(),
                    category: ProviderErrorCategory::Unknown,
                    retryable: false,
                    retry_after_ms: None,
                },
            )?);
        }
        Err(last_error.unwrap_or_else(|| ProviderError {
            code: "discovery_unavailable".into(),
            message: "fetchAvailableModels failed on all candidate hosts".into(),
            category: ProviderErrorCategory::Network,
            retryable: false,
            retry_after_ms: None,
        }))
    }
}

/// Parse the `fetchAvailableModels` payload tolerantly: the exact shape of
/// cpa-core's response is not fully documented, so both a `models` array of
/// strings/objects and a bare array of ids are accepted. Unknown fields are
/// ignored (`#[serde(default)]` semantics via Value access).
fn parse_available_models(value: serde_json::Value) -> Result<Vec<ModelInfo>, ProviderError> {
    let fallback = || ProviderError {
        code: "unknown_discovery_shape".into(),
        message: "fetchAvailableModels response shape not recognized".into(),
        category: ProviderErrorCategory::Unknown,
        retryable: false,
        retry_after_ms: None,
    };
    let models = value
        .get("models")
        .and_then(|v| v.as_array())
        .or_else(|| value.as_array())
        .ok_or_else(fallback)?;
    if models.is_empty() {
        return Err(ProviderError {
            code: "empty_models".into(),
            message: "fetchAvailableModels returned no models".into(),
            category: ProviderErrorCategory::Unknown,
            retryable: false,
            retry_after_ms: None,
        });
    }
    let mut out = Vec::with_capacity(models.len());
    for item in models {
        // Accept both `"gemini-2.5-pro"` and `{"id"/"modelId"/"name": "...", "displayName": "..."}`.
        let (id, display) = match item {
            serde_json::Value::String(s) => (s.clone(), None),
            serde_json::Value::Object(map) => {
                let id = ["modelId", "id", "name"]
                    .iter()
                    .find_map(|k| map.get(*k))
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim_start_matches("models/").to_string());
                let Some(id) = id.filter(|s| !s.is_empty()) else {
                    continue;
                };
                let display = map
                    .get("displayName")
                    .or_else(|| map.get("display_name"))
                    .and_then(|v| v.as_str())
                    .map(str::to_owned);
                (id, display)
            }
            _ => continue,
        };
        if id.is_empty() {
            continue;
        }
        out.push(ModelInfo {
            display_name: display.or_else(|| Some(id.clone())),
            id,
            context_window: 1_048_576,
            max_output: 65_536,
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
        });
    }
    if out.is_empty() {
        return Err(ProviderError {
            code: "empty_models".into(),
            message: "fetchAvailableModels returned no recognizable models".into(),
            category: ProviderErrorCategory::Unknown,
            retryable: false,
            retry_after_ms: None,
        });
    }
    Ok(out)
}

impl Default for AntigravityAdapter {
    fn default() -> Self {
        Self::new()
    }
}

fn antigravity_models() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            id: "gemini-2.5-pro".into(),
            display_name: Some("Gemini 2.5 Pro".into()),
            context_window: 1_048_576,
            max_output: 65_536,
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
        ModelInfo {
            id: "gemini-2.5-flash".into(),
            display_name: Some("Gemini 2.5 Flash".into()),
            context_window: 1_048_576,
            max_output: 65_536,
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
    ]
}

#[async_trait]
impl ProviderAdapter for AntigravityAdapter {
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
                "prompt_cache_automatic".into(),
                "tool_choice".into(),
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

    async fn chat(&self, _request: ProviderRequest) -> Result<ProviderResponse, ProviderError> {
        // OAuth token only arrives via `stream(request, credential)`; there is no
        // static API-key path to synthesise a non-streaming response from.
        Err(ProviderError {
            code: "use_stream".into(),
            message: "Use stream(request, credential) for Antigravity".into(),
            category: ProviderErrorCategory::Auth,
            retryable: false,
            retry_after_ms: None,
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
            message: "Use stream(request, credential) for Antigravity".into(),
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
        let token = credential.api_key.trim().to_string();
        if token.is_empty() {
            return Err(ProviderError {
                code: "missing_key".into(),
                message: "Antigravity OAuth access token required".into(),
                category: ProviderErrorCategory::Auth,
                retryable: false,
                retry_after_ms: None,
            });
        }
        let proxy_url = credential.proxy_url.clone();
        let base = credential.base_url.unwrap_or_else(|| self.base_url.clone());
        let model = if request.model.is_empty() {
            "gemini-2.5-pro".to_string()
        } else {
            request.model.clone()
        };
        // Cloud Code uses `v1internal:*` verbs, not the public `/v1beta/models/*`.
        // The model travels in the body (unlike the public Gemini URL-path form).
        let url = format!(
            "{}/v1internal:streamGenerateContent?alt=sse",
            base.trim_end_matches('/')
        );
        let mut body = super::gemini::build_generate_body(&request);
        body["model"] = serde_json::json!(model);

        let client = match proxy_url.as_deref() {
            Some(url) if !url.trim().is_empty() => crate::http_client::client(Some(url))?,
            _ => self.client.clone(),
        };
        let mut req = client
            .post(&url)
            .header("Authorization", format!("Bearer {token}"))
            .header("Content-Type", "application/json")
            .header("User-Agent", ANTIGRAVITY_UA);
        if let Some(project_id) = credential.project_id.as_deref() {
            if !project_id.trim().is_empty() {
                req = req.header("x-goog-cloud-target-resource", project_id);
            }
        }
        let response = req
            .timeout(STREAM_TIMEOUT)
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
        Ok(antigravity_models())
    }

    /// Real discovery with OAuth credentials; `list_models` (hardcoded) remains
    /// the offline fallback.
    async fn discover_models(
        &self,
        credential: Credential,
    ) -> Result<Vec<ModelInfo>, ProviderError> {
        self.fetch_available_models(&credential)
            .await
            .or_else(|_| Ok(antigravity_models()))
    }

    async fn test_connection(&self) -> Result<ProviderTestResult, ProviderError> {
        Ok(ProviderTestResult {
            success: false,
            latency_ms: None,
            message: "Antigravity uses OAuth — test via a live stream".into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::{ProviderContentBlock, ProviderMessage};
    use futures_util::StreamExt;
    use serde_json::Value;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn request() -> ProviderRequest {
        ProviderRequest {
            model: "gemini-2.5-pro".into(),
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

    async fn read_http_request(socket: &mut tokio::net::TcpStream) -> String {
        let mut bytes = Vec::new();
        let mut chunk = [0_u8; 2048];
        loop {
            let read = socket.read(&mut chunk).await.unwrap();
            assert!(read > 0, "client closed before completing request");
            bytes.extend_from_slice(&chunk[..read]);
            let Some(header_end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") else {
                continue;
            };
            let header_end = header_end + 4;
            let headers = String::from_utf8_lossy(&bytes[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .and_then(|value| value.trim().parse::<usize>().ok())
                })
                .unwrap_or(0);
            if bytes.len() >= header_end + content_length {
                return String::from_utf8(bytes).unwrap();
            }
        }
    }

    #[tokio::test]
    async fn stream_uses_cloud_code_path_bearer_and_project_header() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let request = read_http_request(&mut socket).await;
            let body = concat!(
                "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"ok\"}]},",
                "\"finishReason\":\"STOP\"}],\"usageMetadata\":{\"promptTokenCount\":3,",
                "\"candidatesTokenCount\":1}}\n\n"
            );
            socket
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            request
        });

        let adapter = AntigravityAdapter::new().with_base_url(format!("http://{address}"));
        let credential = Credential {
            api_key: "fixture-access-token".into(),
            base_url: None,
            proxy_url: None,
            key_id: Some("fixture-key".into()),
            provider_type: Some("antigravity".into()),
            project_id: Some("projects/fixture-project".into()),
        };
        let events = adapter
            .stream(request(), credential)
            .await
            .unwrap()
            .collect::<Vec<_>>()
            .await;
        assert!(events
            .iter()
            .any(|event| matches!(event, ProviderEvent::TextDelta(text) if text == "ok")));
        assert!(matches!(
            events.last(),
            Some(ProviderEvent::Completed { .. })
        ));

        let wire = server.await.unwrap();
        let lower = wire.to_ascii_lowercase();
        assert!(wire.starts_with("POST /v1internal:streamGenerateContent?alt=sse HTTP/1.1\r\n"));
        assert!(lower.contains("authorization: bearer fixture-access-token\r\n"));
        assert!(lower.contains("x-goog-cloud-target-resource: projects/fixture-project\r\n"));
        let body = wire.split_once("\r\n\r\n").unwrap().1;
        assert_eq!(
            serde_json::from_str::<Value>(body).unwrap()["model"],
            "gemini-2.5-pro"
        );
        assert!(!body.contains("fixture-access-token"));
    }
}
