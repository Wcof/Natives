//! Real provider adapter seam for the Agent Daemon (ARCH-002).
//!
//! Wraps `provider-adapters` adapters behind the Core's `EngineProvider`
//! interface: streaming, request controls, credential resolution, history
//! translation, and provider error mapping. Extracted from `production.rs`
//! so the runtime facade no longer owns the provider wire seam.

use agent_core::{
    EngineError, EngineMessage, EngineProvider, EngineProviderContext, EngineProviderEvent,
    EngineProviderEventStream, ToolSchema,
};
use futures_util::StreamExt;
use provider_adapters::capabilities::{
    history_message_to_provider, HistoryMessage, ProviderAdapter, ProviderError, ProviderRequest,
    ProviderTool, RequestControls,
};
use provider_adapters::stream::ProviderEvent;
use tokio_util::sync::CancellationToken;

use crate::production_credentials::resolve_credential_for_run;

/// Real HTTP provider adapter wrapper (never returns offline mock tool-call text).
pub struct RealProvider {
    pub provider_id: String,
    pub key_id: Option<String>,
}

#[async_trait::async_trait]
impl EngineProvider for RealProvider {
    /// Stream with provider-default request controls.
    ///
    /// `EngineProvider` has no room for per-run controls, so anything that has
    /// them (the router, which knows the run) calls
    /// [`RealProvider::stream_with_controls`] directly instead.
    async fn stream(
        &self,
        model: &str,
        messages: Vec<EngineMessage>,
        tools: &[ToolSchema],
        system_prompt: Option<&str>,
        cancel: CancellationToken,
    ) -> Result<EngineProviderEventStream, EngineError> {
        self.stream_with_controls(
            &RequestControls::default(),
            model,
            messages,
            tools,
            system_prompt,
            cancel,
        )
        .await
    }

    async fn stream_with_context(
        &self,
        context: EngineProviderContext,
        model: &str,
        messages: Vec<EngineMessage>,
        tools: &[ToolSchema],
        system_prompt: Option<&str>,
        cancel: CancellationToken,
    ) -> Result<EngineProviderEventStream, EngineError> {
        self.stream_with_context_controls(
            &context,
            &RequestControls::default(),
            model,
            messages,
            tools,
            system_prompt,
            cancel,
        )
        .await
    }

    async fn stream_turn(
        &self,
        request: agent_core::ProviderTurnRequest,
        cancel: CancellationToken,
    ) -> Result<EngineProviderEventStream, EngineError> {
        self.stream_with_typed_context_controls(
            &request.context,
            &RequestControls::default(),
            &request.model,
            request.messages,
            &request.tools,
            request.system_prompt.as_deref(),
            cancel,
        )
        .await
    }
}

impl RealProvider {
    /// Stream one turn, applying caller-supplied [`RequestControls`].
    pub async fn stream_with_controls(
        &self,
        controls: &RequestControls,
        model: &str,
        messages: Vec<EngineMessage>,
        tools: &[ToolSchema],
        system_prompt: Option<&str>,
        cancel: CancellationToken,
    ) -> Result<EngineProviderEventStream, EngineError> {
        self.stream_with_context_controls(
            &EngineProviderContext {
                run_id: "legacy-unbound".into(),
                attempt: 0,
            },
            controls,
            model,
            messages,
            tools,
            system_prompt,
            cancel,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)] // pre-existing: parameter list is fixed
    pub async fn stream_with_context_controls(
        &self,
        context: &EngineProviderContext,
        controls: &RequestControls,
        model: &str,
        messages: Vec<EngineMessage>,
        tools: &[ToolSchema],
        system_prompt: Option<&str>,
        cancel: CancellationToken,
    ) -> Result<EngineProviderEventStream, EngineError> {
        let messages = messages
            .into_iter()
            .map(engine_message_to_history)
            .collect();
        self.stream_with_history_context_controls(
            context,
            controls,
            model,
            messages,
            tools,
            system_prompt,
            cancel,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)] // pre-existing: parameter list is fixed
    pub async fn stream_with_typed_context_controls(
        &self,
        context: &EngineProviderContext,
        controls: &RequestControls,
        model: &str,
        messages: Vec<agent_core::AgentMessage>,
        tools: &[ToolSchema],
        system_prompt: Option<&str>,
        cancel: CancellationToken,
    ) -> Result<EngineProviderEventStream, EngineError> {
        let messages = messages.into_iter().map(agent_message_to_history).collect();
        self.stream_with_history_context_controls(
            context,
            controls,
            model,
            messages,
            tools,
            system_prompt,
            cancel,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)] // pre-existing: parameter list is fixed
    pub(crate) async fn stream_with_history_context_controls(
        &self,
        context: &EngineProviderContext,
        controls: &RequestControls,
        model: &str,
        messages: Vec<HistoryMessage>,
        tools: &[ToolSchema],
        system_prompt: Option<&str>,
        cancel: CancellationToken,
    ) -> Result<EngineProviderEventStream, EngineError> {
        let credential =
            resolve_credential_for_run(&self.provider_id, self.key_id.as_deref(), &context.run_id)
                .map_err(EngineError::Message)?;
        let protocol = credential
            .provider_type
            .clone()
            .unwrap_or_else(|| self.provider_id.clone());
        let key_id = credential.key_id.clone();
        let route_key_id = key_id.as_deref().unwrap_or("default").to_string();
        let base_url = credential.base_url.clone();

        if let Some(governor) = crate::global_governor() {
            governor
                .acquire(&self.provider_id, &route_key_id, cancel.clone())
                .await
                .map_err(|e| {
                    if e == "cancelled" {
                        EngineError::Cancelled
                    } else {
                        EngineError::Message(e)
                    }
                })?;
        }

        let provider_messages: Vec<_> = messages
            .into_iter()
            .map(history_message_to_provider)
            .collect();
        let provider_tools: Vec<ProviderTool> = tools
            .iter()
            .map(|t| ProviderTool {
                name: t.name.clone(),
                description: Some(t.description.clone()),
                input_schema: t.input_schema.clone(),
            })
            .collect();

        let mut request = ProviderRequest {
            model: model.to_string(),
            messages: provider_messages,
            system_prompt: system_prompt.map(str::to_string),
            tools: if provider_tools.is_empty() {
                None
            } else {
                Some(provider_tools)
            },
            // `None` delegates the ceiling to the per-model profile in
            // `provider_adapters::model_profile`, matching what `routing.rs`
            // already does for the pooled path. Two reasons the hardcoded 4096
            // had to go: it silently truncated every model with a larger output
            // window, and Anthropic clamps `thinking.budget_tokens` to
            // `max_tokens - 1` — so on this path low/medium/high reasoning
            // effort all collapsed to 4095 and the effort wiring was inert.
            // Models missing from the profile table still fall back to the
            // adapter's own 4096, so nothing regresses.
            max_tokens: None,
            temperature: None,
            stream: true,
            structured_output: None,
            controls: controls.clone(),
        };
        crate::request_rectifier::rectify_provider_request(
            &mut request,
            crate::routing::rectifier_enabled(),
        );

        // 问题8（审计收口 #8）：自动协议路由。开关开启时按 resolver 决策生成
        // 候选协议顺序，只在首个增量前对 404/405/协议形状不兼容回退下一候选；
        // 401/403/429/配额/权限/网络故障不换协议，首增量后绝不重放。
        // 开关关闭时只用供应商显式协议（既有行为不变）。
        //
        // 关键修复：开启路由时**不得**把当前协议作为 `explicit` 传给 resolver——
        // `candidate_protocols` 遇 explicit 会立即短路返回单候选，自动切换因此
        // 事实上被关闭。只有显式配置（开关关闭）才走 single-candidate 路径。
        let explicit_protocol = provider_adapters::parse_protocol(&protocol)
            .unwrap_or(provider_adapters::Protocol::OpenAiChatCompletions);
        let mut candidates: Vec<provider_adapters::Protocol> = if crate::routing::routing_enabled()
        {
            provider_adapters::candidate_protocols(&provider_adapters::ProtocolContext {
                provider_type: &protocol,
                base_url: base_url.as_deref(),
                explicit: None,
                model,
                previous_success: None,
            })
        } else {
            vec![explicit_protocol]
        };
        if !candidates.contains(&explicit_protocol) {
            candidates.insert(0, explicit_protocol);
        }

        let mut fallback_error: Option<ProviderError> = None;
        let mut established: Option<
            std::pin::Pin<Box<dyn futures_util::Stream<Item = ProviderEvent> + Send>>,
        > = None;
        for candidate in candidates {
            // 第一候选沿用显式协议/供应商字符串匹配（兼容 DeepSeek 等特殊供应商）；
            // 回退候选按协议显式构造。
            let adapter: Box<dyn ProviderAdapter> = if candidate == explicit_protocol {
                resolve_adapter(&protocol)
            } else {
                adapter_for_protocol(candidate)
            };
            match adapter.stream(request.clone(), credential.clone()).await {
                Ok(stream) => {
                    // 审计收口 #8：回退不止处理 `adapter.stream()` 的启动错误——
                    // 若 404/405/形状不兼容作为第一个 `ProviderEvent::Error` 从
                    // stream 到达（首增量前），同样关闭当前流换下一候选；任何
                    // 首增量后的错误原样失败，禁止重放。
                    let mut stream = stream;
                    let first = stream.next().await;
                    if let Some(ProviderEvent::Error(first_error)) = &first {
                        let retryable = provider_adapters::should_retry_next_candidate(
                            &format!("{:?}", first_error.category),
                            &first_error.code,
                        );
                        if retryable {
                            // 首增量前协议不兼容：丢弃当前流，记录 fallback 并换下一候选。
                            fallback_error = Some(first_error.clone());
                            continue;
                        }
                    }
                    // 首事件不是可回退错误：把 peek 的首事件放回流，采用该候选。
                    let stream: std::pin::Pin<
                        Box<dyn futures_util::Stream<Item = ProviderEvent> + Send>,
                    > = match first {
                        Some(event) => Box::pin(
                            futures_util::stream::once(futures_util::future::ready(event))
                                .chain(stream),
                        ),
                        None => Box::pin(stream),
                    };
                    established = Some(stream);
                    break;
                }
                Err(e) => {
                    let retryable = provider_adapters::should_retry_next_candidate(
                        &format!("{:?}", e.category),
                        &e.code,
                    );
                    if !retryable {
                        if matches!(
                            e.category,
                            provider_adapters::capabilities::ProviderErrorCategory::RateLimit
                        ) {
                            if let Some(governor) = crate::global_governor() {
                                governor
                                    .record_rate_limit(
                                        &self.provider_id,
                                        &route_key_id,
                                        e.retry_after_ms,
                                    )
                                    .await;
                            }
                        }
                        return Err(EngineError::Provider {
                            message: provider_error_message(
                                &e,
                                &self.provider_id,
                                &protocol,
                                model,
                                key_id.as_deref(),
                                base_url.as_deref(),
                            ),
                            code: e.code,
                            retryable: e.retryable,
                            category: format!("{:?}", e.category),
                            retry_after_ms: e.retry_after_ms,
                        });
                    }
                    fallback_error = Some(e);
                    // 仅对可回退错误（404/405/形状不兼容）尝试下一候选。
                    // 审计收口 #8：协议不兼容**绝不**记录成 rate limit——
                    // 那会污染 governor 的限流账本，把一次 endpoint 不兼容
                    // 误判成上游限流。回退事件交给上层事件流/日志呈现。
                    continue;
                }
            }
        }
        let stream = established.ok_or_else(|| {
            let e = fallback_error.unwrap_or_else(|| ProviderError {
                code: "no_protocol_candidate".into(),
                message: "no protocol candidate succeeded".into(),
                category: provider_adapters::capabilities::ProviderErrorCategory::BadRequest,
                retryable: false,
                retry_after_ms: None,
            });
            EngineError::Provider {
                message: provider_error_message(
                    &e,
                    &self.provider_id,
                    &protocol,
                    model,
                    key_id.as_deref(),
                    base_url.as_deref(),
                ),
                code: e.code,
                retryable: e.retryable,
                category: format!("{:?}", e.category),
                retry_after_ms: e.retry_after_ms,
            }
        })?;
        let provider_id = self.provider_id.clone();
        let route_key_id = route_key_id.clone();
        let governor = crate::global_governor();
        let model = model.to_string();
        let mapped = futures_util::stream::unfold((stream, cancel), move |(mut stream, cancel)| {
            let provider_id = provider_id.clone();
            let route_key_id = route_key_id.clone();
            let governor = governor.clone();
            let protocol = protocol.clone();
            let model = model.clone();
            let key_id = key_id.clone();
            let base_url = base_url.clone();
            async move {
                if cancel.is_cancelled() {
                    return None;
                }
                tokio::select! {
                    ev = stream.next() => {
                        let ev = ev?;
                        if let ProviderEvent::Error(error) = &ev {
                            if matches!(error.category, provider_adapters::capabilities::ProviderErrorCategory::RateLimit) {
                                if let Some(governor) = &governor {
                                    governor.record_rate_limit(&provider_id, &route_key_id, error.retry_after_ms).await;
                                }
                            }
                        }
                        let event = match ev {
                                ProviderEvent::TextDelta(t) => EngineProviderEvent::TextDelta(t),
                                ProviderEvent::ReasoningDelta(t) => EngineProviderEvent::ReasoningDelta(t),
                                ProviderEvent::ToolCallDelta {
                                    index,
                                    id,
                                    name,
                                    arguments_delta,
                                } => EngineProviderEvent::ToolCallDelta {
                                    index,
                                    id,
                                    name,
                                    arguments_delta,
                                },
                                ProviderEvent::Usage(u) => EngineProviderEvent::Usage {
                                    input_tokens: u.input_tokens,
                                    output_tokens: u.output_tokens,
                                    reasoning_tokens: u.reasoning_tokens,
                                    cache_creation_tokens: u.cache_creation_tokens,
                                    cache_read_tokens: u.cache_read_tokens,
                                },
                                ProviderEvent::Completed { reason } => EngineProviderEvent::CompletedWithReason {
                                    reason: match reason {
                                        provider_adapters::stream::ProviderStopReason::Stop => agent_core::ProviderStopReason::Stop,
                                        provider_adapters::stream::ProviderStopReason::ToolUse => agent_core::ProviderStopReason::ToolUse,
                                        provider_adapters::stream::ProviderStopReason::Length => agent_core::ProviderStopReason::Length,
                                        provider_adapters::stream::ProviderStopReason::Cancelled => agent_core::ProviderStopReason::Cancelled,
                                        provider_adapters::stream::ProviderStopReason::Error => agent_core::ProviderStopReason::Error,
                                        provider_adapters::stream::ProviderStopReason::Unknown(value) => agent_core::ProviderStopReason::Unknown(value),
                                    },
                                },
                                ProviderEvent::Error(e) => EngineProviderEvent::Error {
                                        message: provider_error_message(&e, &provider_id, &protocol, &model, key_id.as_deref(), base_url.as_deref()),
                                        code: e.code,
                                        retryable: e.retryable,
                                        category: format!("{:?}", e.category),
                                        retry_after_ms: e.retry_after_ms,
                                },
                            };
                        Some((event, (stream, cancel)))
                    }
                    _ = cancel.cancelled() => None,
                }
            }
        });
        Ok(Box::pin(mapped))
    }
}

// 审计收口 W5：history 转换与 provider 错误映射抽到 provider_messages，
// 保持 provider.rs 在 700 行审查阈值内（candidate 相比基线不新增超 700 文件）。
pub(crate) use crate::provider_messages::{
    agent_message_to_history, engine_message_to_history, provider_error_message,
};

#[cfg(test)]
mod typed_provider_history_tests {
    use super::*;

    #[test]
    fn typed_boundary_preserves_blocks_and_tool_identity() {
        let history = agent_message_to_history(agent_core::AgentMessage::Assistant(
            agent_core::AssistantMessage {
                message_id: agent_core::MessageId::from("message-1"),
                content: vec![
                    agent_core::ContentBlock::Thinking {
                        text: "plan".into(),
                        signature: Some("sig".into()),
                    },
                    agent_core::ContentBlock::Text {
                        text: "calling".into(),
                    },
                    agent_core::ContentBlock::Image {
                        source: agent_core::ImageSource {
                            url: "data:image/png;base64,x".into(),
                            media_type: Some("image/png".into()),
                            detail: Some("high".into()),
                        },
                    },
                    agent_core::ContentBlock::ToolCall(agent_core::ToolCall {
                        tool_call_id: agent_core::ToolCallId::from("call-1"),
                        name: "read_file".into(),
                        arguments_json: r#"{"path":"a.txt"}"#.into(),
                    }),
                ],
                stop_reason: Some(agent_core::StopReason::ToolUse),
            },
        ));

        assert_eq!(history.role, "assistant");
        assert_eq!(history.content, "plancalling");
        assert_eq!(history.images.len(), 1);
        let calls = history
            .tool_calls
            .expect("tool call must remain structured");
        assert_eq!(calls[0].id, "call-1");
        assert_eq!(calls[0].name, "read_file");
        assert_eq!(calls[0].arguments, r#"{"path":"a.txt"}"#);

        let result = agent_message_to_history(agent_core::AgentMessage::ToolResult(
            agent_core::ToolResultMessage {
                message_id: agent_core::MessageId::from("result-1"),
                tool_call_id: agent_core::ToolCallId::from("call-1"),
                tool_name: "read_file".into(),
                content: vec![agent_core::ToolResultBlock::Artifact {
                    artifact_id: "artifact-1".into(),
                    preview: Some("preview".into()),
                }],
                is_error: false,
                code: None,
            },
        ));
        assert_eq!(result.role, "tool");
        assert_eq!(result.tool_call_id.as_deref(), Some("call-1"));
        assert_eq!(result.tool_name.as_deref(), Some("read_file"));
        assert_eq!(result.content, "preview");
    }
}

fn resolve_adapter(provider_id: &str) -> Box<dyn ProviderAdapter> {
    let lower = provider_id.to_ascii_lowercase();
    if lower.contains("antigravity") {
        Box::new(provider_adapters::providers::antigravity::AntigravityAdapter::new())
    } else if lower.contains("anthropic") || lower.contains("claude") {
        Box::new(provider_adapters::providers::anthropic::AnthropicAdapter::new())
    } else if lower.contains("gemini") || lower.contains("google") {
        Box::new(provider_adapters::providers::gemini::GeminiAdapter::new())
    } else if lower.contains("deepseek") {
        Box::new(provider_adapters::providers::deepseek::DeepSeekAdapter::new())
    } else if lower.contains("ollama") {
        Box::new(provider_adapters::providers::ollama::OllamaAdapter::new())
    } else if lower.contains("compatible") || lower.contains("chat_completions") {
        Box::new(provider_adapters::providers::openai_compatible::OpenAiCompatibleAdapter::new())
    } else if lower.contains("responses") {
        Box::new(
            provider_adapters::providers::openai::OpenAiAdapter::new()
                .with_api_mode(provider_adapters::providers::openai::OpenAiApiMode::Responses),
        )
    } else {
        Box::new(provider_adapters::providers::openai::OpenAiAdapter::new())
    }
}

/// 问题8：按候选协议显式构造适配器（路由开关开启时的回退候选）。
fn adapter_for_protocol(protocol: provider_adapters::Protocol) -> Box<dyn ProviderAdapter> {
    use provider_adapters::Protocol;
    match protocol {
        Protocol::AnthropicMessages => {
            Box::new(provider_adapters::providers::anthropic::AnthropicAdapter::new())
        }
        Protocol::GeminiGenerateContent => {
            Box::new(provider_adapters::providers::gemini::GeminiAdapter::new())
        }
        Protocol::OllamaChat => {
            Box::new(provider_adapters::providers::ollama::OllamaAdapter::new())
        }
        Protocol::OpenAiResponses => Box::new(
            provider_adapters::providers::openai::OpenAiAdapter::new()
                .with_api_mode(provider_adapters::providers::openai::OpenAiApiMode::Responses),
        ),
        Protocol::OpenAiChatCompletions | Protocol::OpenAiCompatible => {
            Box::new(provider_adapters::providers::openai::OpenAiAdapter::new())
        }
    }
}

#[cfg(test)]
mod provider_error_message_tests {
    use super::*;
    use provider_adapters::capabilities::ProviderErrorCategory;

    #[test]
    fn provider_error_message_includes_context_and_redacts_secrets() {
        let msg = provider_error_message(
            &ProviderError {
                code: "http_401".into(),
                message: "bad key sk-secret123".into(),
                category: ProviderErrorCategory::Auth,
                retryable: false,
                retry_after_ms: None,
            },
            "p1",
            "openai_chat_completions",
            "deepseek-v4-flash",
            Some("k1"),
            Some("https://token.sensenova.cn/v1"),
        );

        assert!(msg.contains("provider=p1"));
        assert!(msg.contains("protocol=openai_chat_completions"));
        assert!(msg.contains("model=deepseek-v4-flash"));
        assert!(msg.contains("retryable=false"));
        assert!(!msg.contains("sk-secret123"));
    }
}
