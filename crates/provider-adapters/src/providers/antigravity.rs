//! Antigravity (Google Cloud Code) adapter — OAuth Bearer + `v1internal` endpoint.
//!
//! Antigravity is Google's Gemini-for-Code surface. Unlike the public Gemini
//! API (`generativelanguage.googleapis.com` + `?key=`), it runs on the Cloud
//! Code host (`cloudcode-pa.googleapis.com`) with an OAuth `Authorization:
//! Bearer` token and an optional `x-goog-cloud-target-resource` project header.
//! The request/response body is the same `generateContent` shape, so this
//! reuses `gemini::build_generate_body` and the shared Gemini SSE parser.

use crate::capabilities::*;
use crate::stream::{parse_gemini_chunk, split_sse_lines, sse_data_payload, ProviderEvent};
use assistant_protocol::v1::provider::{ModelCapabilities, ProviderType};
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
                                    yield ProviderEvent::Completed {
                                        reason: crate::stream::ProviderStopReason::Unknown("missing_final_event".into()),
                                    };
                                    continue;
                                }
                                for event in parse_gemini_chunk(data) {
                                    if matches!(event, ProviderEvent::Completed { .. }) {
                                        saw_completed = true;
                                    }
                                    yield event;
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
            if !saw_completed {
                yield ProviderEvent::Completed {
                    reason: crate::stream::ProviderStopReason::Unknown("missing_final_event".into()),
                };
            }
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
