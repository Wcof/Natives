//! Daemon-owned local-creative AI analysis (P0).
//!
//! The Tauri Host must never call a Provider directly. It performs the sanitized
//! project scan, then sends a [`CreativeLocalAnalyzeRequest`] over UDS; this
//! module resolves the credential through the daemon's broker and calls the
//! model. The Host parses and validates the returned text — this file never
//! sees credentials, secrets, or absolute project paths.

use assistant_protocol::v2::creative::{CreativeLocalAnalyzeRequest, CreativeLocalAnalyzeResponse};
use provider_adapters::capabilities::{
    Credential, ProviderAdapter, ProviderContentBlock, ProviderMessage, ProviderRequest,
    ProviderResponse, ProviderResponseBlock,
};
use std::time::Duration;

/// Structured daemon-side analysis error (mapped to the RPC error envelope).
#[derive(Debug, Clone)]
pub struct CreativeAiError {
    pub code: String,
    pub message: String,
}

impl CreativeAiError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

const MAX_TIMEOUT_MS: u64 = 120_000;

/// Full path: resolve credential + adapter through daemon authority, then call.
pub async fn analyze_local_creative(
    request: CreativeLocalAnalyzeRequest,
) -> Result<CreativeLocalAnalyzeResponse, CreativeAiError> {
    if request.provider_id.trim().is_empty() {
        return Err(CreativeAiError::new(
            "INVALID_INPUT",
            "provider_id is required",
        ));
    }
    if request.model.trim().is_empty() {
        return Err(CreativeAiError::new("INVALID_INPUT", "model is required"));
    }
    if request.timeout_ms == 0 {
        return Err(CreativeAiError::new(
            "INVALID_INPUT",
            "timeout_ms is required",
        ));
    }

    let credential = crate::production_credentials::resolve_credential_for_run(
        &request.provider_id,
        None,
        "creative-local-analyze",
    )
    .map_err(|e| CreativeAiError::new("UNAUTHORIZED", e))?;

    let adapter = crate::rpc::resolve_provider_adapter(&request.provider_id).ok_or_else(|| {
        CreativeAiError::new(
            "NOT_FOUND",
            format!("unknown provider: {}", request.provider_id),
        )
    })?;

    analyze_local_creative_with(&request, credential, adapter.as_ref()).await
}

/// Testable core: credential + adapter injected, so tests stub the provider
/// without any network. Bounded by the request timeout.
///
/// `_credential` is the daemon-resolved credential; the adapter embeds the key,
/// so `chat` does not need it re-passed — the parameter documents that credential
/// resolution is daemon-owned (P0) and keeps the test seam explicit.
pub async fn analyze_local_creative_with(
    request: &CreativeLocalAnalyzeRequest,
    _credential: Credential,
    adapter: &dyn ProviderAdapter,
) -> Result<CreativeLocalAnalyzeResponse, CreativeAiError> {
    let user = serde_json::to_string_pretty(&request.payload).unwrap_or_default();
    let provider_request = ProviderRequest {
        model: request.model.clone(),
        messages: vec![ProviderMessage {
            role: "user".into(),
            content: vec![ProviderContentBlock::Text { text: user }],
        }],
        system_prompt: Some(request.system_prompt.clone()),
        tools: None,
        max_tokens: Some(1024),
        temperature: Some(0.1),
        stream: false,
        structured_output: None,
        controls: Default::default(),
    };

    let timeout_ms = request.timeout_ms.clamp(5_000, MAX_TIMEOUT_MS);
    let text = match tokio::time::timeout(
        Duration::from_millis(timeout_ms),
        adapter.chat(provider_request),
    )
    .await
    {
        Ok(Ok(resp)) => response_text(&resp),
        Ok(Err(e)) => {
            return Err(CreativeAiError::new(e.code.clone(), e.message.clone()));
        }
        Err(_) => {
            return Err(CreativeAiError::new(
                "TIMEOUT",
                format!("provider call timed out after {timeout_ms}ms"),
            ));
        }
    };
    if text.trim().is_empty() {
        return Err(CreativeAiError::new(
            "EMPTY_RESPONSE",
            "provider returned no content",
        ));
    }
    Ok(CreativeLocalAnalyzeResponse { text })
}

fn response_text(resp: &ProviderResponse) -> String {
    let mut out = String::new();
    for b in &resp.content {
        if let ProviderResponseBlock::Text(t) = b {
            out.push_str(t);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use assistant_protocol::v2::creative::CreativeAnalyzeKind;
    use provider_adapters::capabilities::ProviderError;
    use provider_adapters::capabilities::{
        ModelInfo, ProviderCapabilities, ProviderTestResult, ProviderUsage,
    };
    use provider_adapters::stream::ProviderEvent;

    struct StubAdapter {
        text: String,
        fail: bool,
    }

    #[async_trait::async_trait]
    impl ProviderAdapter for StubAdapter {
        fn provider_type(&self) -> assistant_protocol::v1::provider::ProviderType {
            assistant_protocol::v1::provider::ProviderType::OpenaiCompatible
        }

        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities {
                provider_type: self.provider_type(),
                features: vec!["streaming".into()],
                max_context_window: 1000,
                streaming: true,
                tool_calls: false,
                structured_output: false,
                image_input: false,
                file_input: false,
                reasoning: false,
                system_prompt: true,
                function_calling: false,
            }
        }

        async fn chat(&self, _request: ProviderRequest) -> Result<ProviderResponse, ProviderError> {
            if self.fail {
                return Err(ProviderError {
                    code: "UPSTREAM".into(),
                    message: "boom".into(),
                    category: provider_adapters::capabilities::ProviderErrorCategory::ServerError,
                    retryable: true,
                    retry_after_ms: None,
                });
            }
            Ok(ProviderResponse {
                content: vec![ProviderResponseBlock::Text(self.text.clone())],
                usage: ProviderUsage {
                    input_tokens: 1,
                    output_tokens: 1,
                    reasoning_tokens: None,
                    cache_creation_tokens: None,
                    cache_read_tokens: None,
                    cost_usd: None,
                },
            })
        }

        async fn chat_stream(
            &self,
            _request: ProviderRequest,
        ) -> Result<
            Box<
                dyn futures_util::Stream<Item = provider_adapters::ProviderStreamEvent>
                    + Send
                    + Unpin,
            >,
            ProviderError,
        > {
            unimplemented!()
        }

        async fn stream(
            &self,
            _request: ProviderRequest,
            _credential: Credential,
        ) -> Result<
            std::pin::Pin<Box<dyn futures_util::Stream<Item = ProviderEvent> + Send>>,
            ProviderError,
        > {
            unimplemented!()
        }

        async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
            Ok(Vec::new())
        }

        async fn test_connection(&self) -> Result<ProviderTestResult, ProviderError> {
            unimplemented!()
        }
    }

    fn sample_request() -> CreativeLocalAnalyzeRequest {
        CreativeLocalAnalyzeRequest {
            kind: CreativeAnalyzeKind::Launch,
            provider_id: "openai".into(),
            model: "gpt-x".into(),
            system_prompt: "return JSON".into(),
            payload: serde_json::json!({ "virtualRoot": "/project", "kind": "html" }),
            timeout_ms: 10_000,
        }
    }

    #[tokio::test]
    async fn routes_through_adapter_and_returns_text() {
        let adapter = StubAdapter {
            text: r#"{"schemaVersion":1,"program":"internal"}"#.into(),
            fail: false,
        };
        let cred = Credential {
            api_key: "test-key".into(),
            base_url: None,
            proxy_url: None,
            key_id: Some("k".into()),
            provider_type: Some("openai_compatible".into()),
        };
        let out = analyze_local_creative_with(&sample_request(), cred, &adapter)
            .await
            .expect("analysis succeeds");
        assert!(out.text.contains("program"));
    }

    #[tokio::test]
    async fn provider_error_propagates_structured() {
        let adapter = StubAdapter {
            text: String::new(),
            fail: true,
        };
        let cred = Credential {
            api_key: "test-key".into(),
            base_url: None,
            proxy_url: None,
            key_id: None,
            provider_type: None,
        };
        let err = analyze_local_creative_with(&sample_request(), cred, &adapter)
            .await
            .expect_err("upstream error surfaces");
        assert_eq!(err.code, "UPSTREAM");
        assert!(err.message.contains("boom"));
    }
}
