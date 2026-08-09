//! `provider.*` RPC handlers, adapter resolution, and provider model tests.
//!
//! Split out of the former single-file `rpc.rs` (A2-03).

use futures_util::StreamExt;
use provider_adapters::capabilities::{
    Credential, ProviderContentBlock, ProviderMessage, ProviderRequest, ProviderTestResult,
};
use provider_adapters::stream::ProviderEvent;

// {A2-03} resolve_provider_adapter (moved verbatim from rpc.rs)
/// Resolve a Provider adapter by id. Shared with the daemon's creative AI
/// module so provider matching lives in exactly one place (R-B3).
pub(crate) fn resolve_provider_adapter(
    provider_id: &str,
) -> Option<Box<dyn provider_adapters::ProviderAdapter>> {
    let needle = provider_id.to_ascii_lowercase();
    provider_adapters::register_all().into_iter().find(|p| {
        let t = format!("{:?}", p.provider_type()).to_ascii_lowercase();
        t == needle
            || t.contains(&needle)
            || needle.contains(&t)
            || (needle.contains("compatible") && t.contains("compatible"))
            || (needle.contains("openai") && t == "openai")
            || (needle.contains("anthropic") && t.contains("anthropic"))
            || (needle.contains("gemini") && t.contains("gemini"))
            || (needle.contains("deepseek") && t.contains("deepseek"))
            || (needle.contains("ollama") && t.contains("ollama"))
    })
}

// {A2-03} test_provider_model (moved verbatim from rpc.rs)
pub(crate) async fn test_provider_model(
    adapter: &dyn provider_adapters::ProviderAdapter,
    credential: Credential,
    model: &str,
) -> Result<ProviderTestResult, provider_adapters::capabilities::ProviderError> {
    let started = std::time::Instant::now();
    let request = ProviderRequest {
        model: model.to_string(),
        messages: vec![ProviderMessage {
            role: "user".into(),
            content: vec![ProviderContentBlock::Text {
                text: "Reply with exactly: ok".into(),
            }],
        }],
        system_prompt: Some("Be concise.".into()),
        tools: None,
        max_tokens: Some(16),
        temperature: Some(0.0),
        stream: true,
        structured_output: None,
        controls: Default::default(),
    };
    let mut stream = adapter.stream(request, credential).await?;
    let mut text = String::new();
    while let Some(event) = stream.next().await {
        match event {
            ProviderEvent::TextDelta(delta) => text.push_str(&delta),
            ProviderEvent::Completed { .. } => {
                break;
            }
            ProviderEvent::Error(err) => return Err(err),
            _ => {}
        }
    }
    if !text.trim().is_empty() {
        Ok(ProviderTestResult {
            success: true,
            latency_ms: Some(started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64),
            message: format!("Model test passed: {model}"),
        })
    } else {
        Err(provider_adapters::capabilities::ProviderError {
            code: "EMPTY_RESPONSE".into(),
            message: format!("Provider test returned no content: model={model}"),
            category: provider_adapters::capabilities::ProviderErrorCategory::Unknown,
            retryable: true,
            retry_after_ms: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // {A2-03} StaticStreamAdapter test helper (moved verbatim from rpc.rs)
    struct StaticStreamAdapter {
        events: Vec<ProviderEvent>,
    }

    #[async_trait::async_trait]
    impl provider_adapters::ProviderAdapter for StaticStreamAdapter {
        fn provider_type(&self) -> assistant_protocol::v1::provider::ProviderType {
            assistant_protocol::v1::provider::ProviderType::OpenaiCompatible
        }

        fn capabilities(&self) -> provider_adapters::capabilities::ProviderCapabilities {
            provider_adapters::capabilities::ProviderCapabilities {
                provider_type: self.provider_type(),
                features: vec!["streaming".into()],
                max_context_window: 1_000,
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

        async fn chat(
            &self,
            _request: ProviderRequest,
        ) -> Result<
            provider_adapters::capabilities::ProviderResponse,
            provider_adapters::capabilities::ProviderError,
        > {
            unreachable!("provider.test must use stream")
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
            provider_adapters::capabilities::ProviderError,
        > {
            unreachable!("provider.test must use stream")
        }

        async fn stream(
            &self,
            _request: ProviderRequest,
            _credential: Credential,
        ) -> Result<
            std::pin::Pin<Box<dyn futures_util::Stream<Item = ProviderEvent> + Send>>,
            provider_adapters::capabilities::ProviderError,
        > {
            Ok(Box::pin(futures_util::stream::iter(self.events.clone())))
        }

        async fn list_models(
            &self,
        ) -> Result<
            Vec<provider_adapters::capabilities::ModelInfo>,
            provider_adapters::capabilities::ProviderError,
        > {
            Ok(Vec::new())
        }

        async fn test_connection(
            &self,
        ) -> Result<ProviderTestResult, provider_adapters::capabilities::ProviderError> {
            unreachable!("provider.test must not use key-present fake checks")
        }
    }
    // {A2-03} provider_model_test_consumes_stream_content (moved verbatim from rpc.rs)
    #[tokio::test]
    async fn provider_model_test_consumes_stream_content() {
        let adapter = StaticStreamAdapter {
            events: vec![
                ProviderEvent::TextDelta("ok".into()),
                ProviderEvent::Completed {
                    reason: provider_adapters::stream::ProviderStopReason::Stop,
                },
            ],
        };
        let result = test_provider_model(
            &adapter,
            Credential {
                api_key: "test-key".into(),
                base_url: None,
                proxy_url: None,
                key_id: Some("k".into()),
                provider_type: Some("openai_compatible".into()),
            },
            "model-under-test",
        )
        .await
        .unwrap();

        assert!(result.success);
        assert!(result.message.contains("model-under-test"));
    }
    // {A2-03} provider_model_test_empty_stream_is_structured_error (moved verbatim from rpc.rs)
    #[tokio::test]
    async fn provider_model_test_empty_stream_is_structured_error() {
        let adapter = StaticStreamAdapter {
            events: vec![ProviderEvent::Completed {
                reason: provider_adapters::stream::ProviderStopReason::Stop,
            }],
        };
        let error = test_provider_model(
            &adapter,
            Credential {
                api_key: "test-key".into(),
                base_url: None,
                proxy_url: None,
                key_id: Some("k".into()),
                provider_type: Some("openai_compatible".into()),
            },
            "empty-model",
        )
        .await
        .unwrap_err();

        assert_eq!(error.code, "EMPTY_RESPONSE");
        assert!(error.retryable);
        assert!(error.message.contains("Provider test returned no content"));
    }
}

// {A2-03} dispatch_provider: routes the `provider.*` method family. Arms moved verbatim
// from handle_rpc's match in the former single-file rpc.rs; the trailing
// `_ =>` keeps any method that reaches this handler but has no arm here on the
// honest unsupported path (R-B1).
pub(crate) async fn dispatch_provider(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    request: &assistant_protocol::v1::daemon::RpcRequest,
) {
    use crate::rpc::{send_error, send_success};
    use assistant_protocol::error::{error_codes, DaemonError, ErrorCategory};
    use assistant_protocol::v2::creative::CreativeLocalAnalyzeRequest;
    use assistant_protocol::v2::methods::names;
    match request.method.as_str() {
        names::PROVIDER_LIST => {
            let providers = provider_adapters::register_all()
                .into_iter()
                .map(|p| {
                    let caps = p.capabilities();
                    serde_json::json!({
                        "provider_type": format!("{:?}", p.provider_type()).to_ascii_lowercase(),
                        "streaming": caps.streaming,
                        "tool_calls": caps.tool_calls,
                        "reasoning": caps.reasoning,
                    })
                })
                .collect::<Vec<_>>();
            send_success(
                writer,
                &request.request_id,
                &request.client_id,
                &request.session_token,
                serde_json::json!({ "providers": providers }),
            )
            .await;
        }

        names::PROVIDER_DISCOVER_MODELS => {
            let provider_id = request
                .params
                .get("provider_id")
                .or_else(|| request.params.get("provider"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let key_id = request
                .params
                .get("key_id")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let run_id = request
                .params
                .get("run_id")
                .and_then(|v| v.as_str())
                .unwrap_or("discover");
            match resolve_provider_adapter(provider_id) {
                Some(adapter) => {
                    let cred = crate::production::resolve_credential_for_run(
                        provider_id,
                        key_id.as_deref(),
                        run_id,
                    );
                    let result = match cred {
                        Ok(c) => adapter.discover_models(c).await,
                        Err(_) => adapter.list_models().await,
                    };
                    match result {
                        Ok(models) => {
                            send_success(
                                writer,
                                &request.request_id,
                                &request.client_id,
                                &request.session_token,
                                serde_json::json!({ "models": models }),
                            )
                            .await;
                        }
                        Err(e) => {
                            send_error(
                                writer,
                                &DaemonError::new(
                                    error_codes::PROVIDER_ERROR,
                                    ErrorCategory::Provider,
                                    e.retryable,
                                    e.message.clone(),
                                ),
                            )
                            .await;
                        }
                    }
                }
                None => {
                    send_error(
                        writer,
                        &DaemonError::new(
                            error_codes::NOT_FOUND,
                            ErrorCategory::NotFound,
                            false,
                            format!("unknown provider: {provider_id}"),
                        ),
                    )
                    .await;
                }
            }
        }

        names::PROVIDER_TEST => {
            let provider_id = request
                .params
                .get("provider_id")
                .or_else(|| request.params.get("provider"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let key_id = request
                .params
                .get("key_id")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let run_id = request
                .params
                .get("run_id")
                .and_then(|v| v.as_str())
                .unwrap_or("provider-test");
            let model = request
                .params
                .get("model_id")
                .or_else(|| request.params.get("model"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            if model.is_empty() {
                send_error(
                    writer,
                    &DaemonError::new(
                        error_codes::INVALID_INPUT,
                        ErrorCategory::Validation,
                        false,
                        "model_id is required for provider.test",
                    ),
                )
                .await;
                return;
            }
            match resolve_provider_adapter(provider_id) {
                Some(adapter) => {
                    let result = match crate::production::resolve_credential_for_run(
                        provider_id,
                        key_id.as_deref(),
                        run_id,
                    ) {
                        Ok(c) => test_provider_model(adapter.as_ref(), c, model).await,
                        Err(e) => {
                            send_error(
                                writer,
                                &DaemonError::new(
                                    error_codes::UNAUTHORIZED,
                                    ErrorCategory::Auth,
                                    false,
                                    e,
                                ),
                            )
                            .await;
                            return;
                        }
                    };
                    match result {
                        Ok(r) => {
                            send_success(
                                writer,
                                &request.request_id,
                                &request.client_id,
                                &request.session_token,
                                serde_json::to_value(r).unwrap_or_default(),
                            )
                            .await;
                        }
                        Err(e) => {
                            send_error(
                                writer,
                                &DaemonError::new(
                                    e.code.clone(),
                                    ErrorCategory::Provider,
                                    e.retryable,
                                    e.message.clone(),
                                ),
                            )
                            .await;
                        }
                    }
                }
                None => {
                    send_error(
                        writer,
                        &DaemonError::new(
                            error_codes::NOT_FOUND,
                            ErrorCategory::NotFound,
                            false,
                            format!("unknown provider: {provider_id}"),
                        ),
                    )
                    .await;
                }
            }
        }

        names::CREATIVE_LOCAL_ANALYZE => {
            // P0: the Host must not call a Provider directly. This arm is the
            // daemon-controlled path for local-creative AI analysis; the Host
            // sends a sanitized request and validates the returned text.
            let analyze_req: CreativeLocalAnalyzeRequest =
                match serde_json::from_value(request.params.clone()) {
                    Ok(r) => r,
                    Err(e) => {
                        send_error(
                            writer,
                            &DaemonError::new(
                                error_codes::INVALID_INPUT,
                                ErrorCategory::Validation,
                                false,
                                format!("creative.local.analyze: {e}"),
                            ),
                        )
                        .await;
                        return;
                    }
                };
            match crate::creative_ai::analyze_local_creative(analyze_req).await {
                Ok(resp) => {
                    send_success(
                        writer,
                        &request.request_id,
                        &request.client_id,
                        &request.session_token,
                        serde_json::to_value(resp).unwrap_or_default(),
                    )
                    .await;
                }
                Err(e) => {
                    let category = if e.code == "UNAUTHORIZED" {
                        ErrorCategory::Auth
                    } else if e.code == "NOT_FOUND" {
                        ErrorCategory::NotFound
                    } else if e.code == "INVALID_INPUT" {
                        ErrorCategory::Validation
                    } else {
                        ErrorCategory::Provider
                    };
                    let code = if e.code == "UNAUTHORIZED" {
                        error_codes::UNAUTHORIZED
                    } else if e.code == "NOT_FOUND" {
                        error_codes::NOT_FOUND
                    } else if e.code == "INVALID_INPUT" {
                        error_codes::INVALID_INPUT
                    } else {
                        "provider_error"
                    };
                    send_error(writer, &DaemonError::new(code, category, true, e.message)).await;
                }
            }
        }

        _ => {
            let status = assistant_protocol::v2::method_status(&request.method);
            let code = match status {
                assistant_protocol::v2::MethodStatus::Unsupported => "unsupported",
                assistant_protocol::v2::MethodStatus::InvalidRequest => "invalid_request",
                assistant_protocol::v2::MethodStatus::Implemented => "internal_error",
            };
            let err = DaemonError::new(
                code,
                ErrorCategory::Unsupported,
                false,
                format!(
                    "method not implemented: {} (status={status:?}; see daemon.getCapabilities)",
                    request.method
                ),
            );
            send_error(writer, &err).await;
        }
    }
}
