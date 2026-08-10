//! Run loop domain (W9 split from engine_core.rs): `run_inner` — the single
//! turn orchestration authority. Preparation, provider streaming, tool
//! execution, safe points and compaction calls all funnel through this method;
//! helpers live in engine_events / engine_input / engine_safe_point /
//! engine_tools / engine_compaction.

use super::conversion::{
    agent_messages_to_values, apply_prompt_hook_responses, core_stop_reason,
    engine_messages_to_agent_messages, provider_backoff_ms, stop_reason_label,
    tool_args_fingerprint, values_to_agent_messages,
};
use super::engine_core::AgentEngine;
use super::engine_tools::{is_long_running_tool_result, ExecutedToolCall, PreparedToolCall};
use super::error::EngineError;
use super::provider::*;
use super::tool_runtime::*;
use crate::doom_loop::DoomLoopDetector;
use crate::hooks::{HookDecision, HookEvent, HookRegistry, HookRequest};
use assistant_protocol::v2::RunEventKind;
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::collections::BTreeMap;

impl AgentEngine {
    pub(super) async fn run_inner(
        &self,
        mut config: EngineRunConfig,
        provider: &dyn EngineProvider,
        tools: &dyn EngineToolRuntime,
        tool_schemas: Vec<ToolSchema>,
        typed_transcript: Option<Vec<crate::AgentMessage>>,
    ) -> Result<crate::EngineOutcome, EngineError> {
        use crate::EngineOutcome;
        let run_id_owned = config.run_id.clone();
        let run_id = &run_id_owned;
        let tool_capabilities: BTreeMap<String, ToolCapability> = tools
            .list_tool_capabilities()
            .await
            .into_iter()
            .map(|capability| (capability.name.clone(), capability))
            .collect();
        // Lifecycle status is owned by RunManager::commit_transition.
        // Engine only emits domain events and returns EngineOutcome.
        let session_start = self
            .hooks
            .dispatch(HookRequest {
                event: HookEvent::SessionStart,
                run_id: run_id.to_string(),
                tool_name: None,
                input: serde_json::json!({ "conversation_id": config.conversation_id }),
            })
            .await;
        let mut injected_hook_messages = apply_prompt_hook_responses(&mut config, session_start)?;
        let prompt_submit = self
            .hooks
            .dispatch(HookRequest {
                event: HookEvent::UserPromptSubmit,
                run_id: run_id.to_string(),
                tool_name: None,
                input: serde_json::json!({ "content": config.user_content }),
            })
            .await;
        injected_hook_messages.extend(apply_prompt_hook_responses(&mut config, prompt_submit)?);

        // History is prior turns; always ensure the current user prompt appears
        // exactly once. Production supplies typed history directly. Legacy
        // callers cross the conversion boundary once here and never re-enter it.
        let mut typed_messages = if let Some(messages) = typed_transcript {
            messages
        } else {
            let initial_messages = if config.messages.is_empty() {
                vec![EngineMessage {
                    role: "user".into(),
                    content: config.user_content.clone(),
                    tool_call_id: None,
                    tool_name: None,
                    tool_calls: None,
                    images: Vec::new(),
                }]
            } else {
                let mut msgs = config.messages.clone();
                let already_has_current = msgs.last().is_some_and(|m| {
                    m.role == "user" && m.content.trim() == config.user_content.trim()
                });
                if !already_has_current && !config.user_content.trim().is_empty() {
                    msgs.push(EngineMessage {
                        role: "user".into(),
                        content: config.user_content.clone(),
                        tool_call_id: None,
                        tool_name: None,
                        tool_calls: None,
                        images: Vec::new(),
                    });
                }
                msgs
            };
            engine_messages_to_agent_messages(&initial_messages)
        };
        if !config.user_content.trim().is_empty()
            && !typed_messages.last().is_some_and(|message| {
                matches!(
                    message,
                    crate::AgentMessage::User(value)
                        if value.content.iter().any(|block| matches!(
                            block,
                            crate::ContentBlock::Text { text } if text.trim() == config.user_content.trim()
                        ))
                )
            })
        {
            typed_messages.push(crate::AgentMessage::User(crate::UserMessage {
                message_id: crate::MessageId::new(),
                content: vec![crate::ContentBlock::Text {
                    text: config.user_content.clone(),
                }],
            }));
        }
        // Hook Inject must reach the typed transcript (and therefore the next
        // ProviderTurnRequest), not a side-channel EngineMessage list. Injected
        // content becomes leading system context: deterministic position, safe
        // for safety instructions, and identical for legacy and typed callers.
        if !injected_hook_messages.is_empty() {
            typed_messages.splice(
                0..0,
                injected_hook_messages.into_iter().map(|text| {
                    crate::AgentMessage::System(crate::SystemMessage {
                        message_id: crate::MessageId::new(),
                        text,
                    })
                }),
            );
        }
        let mut doom = DoomLoopDetector::new();
        let mut step = 0u32;

        loop {
            if self.cancel.is_cancelled() {
                return Ok(EngineOutcome::Cancelled);
            }
            step += 1;
            if step > config.max_steps {
                return Err(EngineError::MaxSteps);
            }

            let turn_id = crate::TurnId::new();
            let assistant_message_id = crate::MessageId::new();
            self.append_critical(
                run_id,
                RunEventKind::TurnStarted {
                    turn_id: turn_id.to_string(),
                },
            )?;
            self.append_critical(
                run_id,
                RunEventKind::MessageStarted {
                    turn_id: turn_id.to_string(),
                    message_id: assistant_message_id.to_string(),
                    role: "assistant".into(),
                },
            )?;

            const MAX_PROVIDER_ATTEMPTS: u32 = 3;
            let mut attempt = 1u32;
            // One-shot overflow policy (C03-I03): each user input may be
            // compacted at most once in response to a context-overflow error;
            // a second overflow fails without looping.
            let mut overflow_compacted = false;
            let (text_acc, reasoning_acc, tool_acc, stop_reason) = 'attempts: loop {
                self.events.append(
                    run_id,
                    RunEventKind::GenerationAttemptStarted {
                        attempt,
                        max_attempts: MAX_PROVIDER_ATTEMPTS,
                    },
                );

                let provider_events = match provider
                    .stream_turn(
                        ProviderTurnRequest {
                            context: EngineProviderContext {
                                run_id: run_id.to_string(),
                                attempt,
                            },
                            model: config.model.clone(),
                            system_prompt: config.system_prompt.clone(),
                            messages: typed_messages.clone(),
                            tools: tool_schemas.clone(),
                        },
                        self.cancel.clone(),
                    )
                    .await
                {
                    Ok(stream) => stream,
                    Err(EngineError::Cancelled) => {
                        self.close_failed_turn(
                            run_id,
                            &turn_id,
                            &assistant_message_id,
                            "cancelled",
                            "",
                            "",
                        )?;
                        return Ok(EngineOutcome::Cancelled);
                    }
                    Err(_e) if self.cancel.is_cancelled() => {
                        self.close_failed_turn(
                            run_id,
                            &turn_id,
                            &assistant_message_id,
                            "cancelled",
                            "",
                            "",
                        )?;
                        return Ok(EngineOutcome::Cancelled);
                    }
                    Err(e) if e.retryable() && attempt < MAX_PROVIDER_ATTEMPTS => {
                        // Rate limits used to skip the backoff entirely and
                        // retry immediately, which is the one case where the
                        // provider explicitly told us not to.
                        let delay = provider_backoff_ms(attempt, e.retry_after_ms());
                        self.events.append(
                            run_id,
                            RunEventKind::GenerationAttemptFailed {
                                attempt,
                                code: e.code().into(),
                                retryable: true,
                                retrying: true,
                                retry_in_ms: Some(delay),
                            },
                        );
                        if !self
                            .sleep_provider_backoff(attempt, e.retry_after_ms())
                            .await
                        {
                            self.close_failed_turn(
                                run_id,
                                &turn_id,
                                &assistant_message_id,
                                "cancelled",
                                "",
                                "",
                            )?;
                            return Ok(EngineOutcome::Cancelled);
                        }
                        attempt += 1;
                        continue 'attempts;
                    }
                    Err(e) if e.is_context_overflow() => {
                        self.events.append(
                            run_id,
                            RunEventKind::GenerationAttemptFailed {
                                attempt,
                                code: e.code().into(),
                                retryable: false,
                                retrying: !overflow_compacted,
                                retry_in_ms: None,
                            },
                        );
                        if overflow_compacted {
                            // One-shot policy: the input was already compacted
                            // once and the provider still rejects the context.
                            // Fail immediately — never an unbounded retry loop.
                            self.close_failed_turn(
                                run_id,
                                &turn_id,
                                &assistant_message_id,
                                "error",
                                "",
                                "",
                            )?;
                            return Err(e);
                        }
                        overflow_compacted = true;
                        attempt += 1;
                        let compacted = self
                            .maybe_compact_typed_history(
                                run_id,
                                &turn_id,
                                &config.model,
                                provider,
                                typed_messages,
                            )
                            .await?;
                        typed_messages = compacted;
                        continue 'attempts;
                    }
                    Err(e) => {
                        self.events.append(
                            run_id,
                            RunEventKind::GenerationAttemptFailed {
                                attempt,
                                code: e.code().into(),
                                retryable: e.retryable(),
                                retrying: false,
                                retry_in_ms: None,
                            },
                        );
                        self.close_failed_turn(
                            run_id,
                            &turn_id,
                            &assistant_message_id,
                            "error",
                            "",
                            "",
                        )?;
                        return Err(e);
                    }
                };

                let mut text_acc = String::new();
                let mut reasoning_acc = String::new();
                let mut tool_acc: BTreeMap<usize, (String, String, String)> = BTreeMap::new();
                let mut saw_generation_delta = false;
                let mut completed_reason: Option<ProviderStopReason> = None;
                tokio::pin!(provider_events);

                while let Some(event) = provider_events.next().await {
                    if self.cancel.is_cancelled() {
                        if !tool_acc.is_empty() {
                            completed_reason = Some(ProviderStopReason::Cancelled);
                            break;
                        }
                        self.close_failed_turn(
                            run_id,
                            &turn_id,
                            &assistant_message_id,
                            "cancelled",
                            &text_acc,
                            &reasoning_acc,
                        )?;
                        return Ok(EngineOutcome::Cancelled);
                    }
                    match event {
                        EngineProviderEvent::TextDelta(t) => {
                            saw_generation_delta = true;
                            text_acc.push_str(&t);
                            // Live delta: ephemeral bus only, never durable.
                            // The cumulative MessageDelta emit is deleted
                            // (contract: live-durable-event-contract.md §Message).
                            self.live
                                .append(run_id, RunEventKind::TextDelta { text: t });
                        }
                        EngineProviderEvent::ReasoningDelta(t) => {
                            saw_generation_delta = true;
                            reasoning_acc.push_str(&t);
                            // Live delta: ephemeral bus only, never durable.
                            self.live
                                .append(run_id, RunEventKind::ReasoningDelta { text: t });
                        }
                        EngineProviderEvent::ToolCallDelta {
                            index,
                            id,
                            name,
                            arguments_delta,
                        } => {
                            saw_generation_delta = true;
                            let entry = tool_acc
                                .entry(index)
                                .or_insert_with(|| (String::new(), String::new(), String::new()));
                            if let Some(id) = id.clone() {
                                if !id.is_empty() {
                                    entry.0 = id;
                                }
                            }
                            if let Some(name) = name.clone() {
                                if !name.is_empty() {
                                    entry.1 = name;
                                }
                            }
                            entry.2.push_str(&arguments_delta);
                            // Live delta: ephemeral bus only, never durable.
                            self.live.append(
                                run_id,
                                RunEventKind::ToolCallDelta {
                                    index,
                                    id,
                                    name,
                                    arguments_delta,
                                },
                            );
                        }
                        EngineProviderEvent::Usage {
                            input_tokens,
                            output_tokens,
                            reasoning_tokens,
                            cache_creation_tokens,
                            cache_read_tokens,
                        } => {
                            self.events.append(
                                run_id,
                                RunEventKind::UsageUpdated {
                                    input_tokens,
                                    output_tokens,
                                    reasoning_tokens,
                                    cache_creation_tokens,
                                    cache_read_tokens,
                                },
                            );
                        }
                        EngineProviderEvent::Error {
                            message,
                            code,
                            retryable,
                            category,
                            retry_after_ms,
                        } => {
                            if !saw_generation_delta && retryable && attempt < MAX_PROVIDER_ATTEMPTS
                            {
                                // Same fix as the connect path above: honour the
                                // provider's own delay instead of special-casing
                                // RateLimit into a zero-wait retry.
                                let delay = provider_backoff_ms(attempt, retry_after_ms);
                                self.events.append(
                                    run_id,
                                    RunEventKind::GenerationAttemptFailed {
                                        attempt,
                                        code: code.clone(),
                                        retryable,
                                        retrying: true,
                                        retry_in_ms: Some(delay),
                                    },
                                );
                                if !self.sleep_provider_backoff(attempt, retry_after_ms).await {
                                    self.close_failed_turn(
                                        run_id,
                                        &turn_id,
                                        &assistant_message_id,
                                        "cancelled",
                                        &text_acc,
                                        &reasoning_acc,
                                    )?;
                                    return Ok(EngineOutcome::Cancelled);
                                }
                                attempt += 1;
                                continue 'attempts;
                            }
                            if saw_generation_delta {
                                self.events.append(
                                    run_id,
                                    RunEventKind::GenerationAttemptDiscarded {
                                        attempt,
                                        reason: code.clone(),
                                    },
                                );
                            }
                            self.events.append(
                                run_id,
                                RunEventKind::GenerationAttemptFailed {
                                    attempt,
                                    code: code.clone(),
                                    retryable,
                                    retrying: false,
                                    retry_in_ms: None,
                                },
                            );
                            if !tool_acc.is_empty() {
                                completed_reason = Some(ProviderStopReason::Error);
                                break;
                            }
                            self.close_failed_turn(
                                run_id,
                                &turn_id,
                                &assistant_message_id,
                                "error",
                                &text_acc,
                                &reasoning_acc,
                            )?;
                            return Err(EngineError::Provider {
                                message,
                                code,
                                retryable,
                                category,
                                retry_after_ms,
                            });
                        }
                        EngineProviderEvent::CompletedWithReason { reason } => {
                            completed_reason = Some(reason);
                        }
                        EngineProviderEvent::Completed => {
                            completed_reason = Some(ProviderStopReason::Stop);
                        }
                    }
                }

                if self.cancel.is_cancelled() && tool_acc.is_empty() {
                    self.close_failed_turn(
                        run_id,
                        &turn_id,
                        &assistant_message_id,
                        "cancelled",
                        &text_acc,
                        &reasoning_acc,
                    )?;
                    return Ok(EngineOutcome::Cancelled);
                }

                if completed_reason.is_none() && !tool_acc.is_empty() {
                    self.events.append(
                        run_id,
                        RunEventKind::GenerationAttemptDiscarded {
                            attempt,
                            reason: "INCOMPLETE_TOOL_CALL".into(),
                        },
                    );
                    break (
                        text_acc,
                        reasoning_acc,
                        tool_acc,
                        Some(ProviderStopReason::Unknown("INCOMPLETE_TOOL_CALL".into())),
                    );
                }

                if !saw_generation_delta && attempt < 2 {
                    self.events.append(
                        run_id,
                        RunEventKind::GenerationAttemptFailed {
                            attempt,
                            code: "EMPTY_RESPONSE".into(),
                            retryable: true,
                            retrying: true,
                            retry_in_ms: Some(provider_backoff_ms(attempt, None)),
                        },
                    );
                    if !self.sleep_provider_backoff(attempt, None).await {
                        self.close_failed_turn(
                            run_id,
                            &turn_id,
                            &assistant_message_id,
                            "cancelled",
                            &text_acc,
                            &reasoning_acc,
                        )?;
                        return Ok(EngineOutcome::Cancelled);
                    }
                    attempt += 1;
                    continue 'attempts;
                }
                if !saw_generation_delta {
                    self.events.append(
                        run_id,
                        RunEventKind::GenerationAttemptFailed {
                            attempt,
                            code: "EMPTY_RESPONSE".into(),
                            retryable: false,
                            retrying: false,
                            retry_in_ms: None,
                        },
                    );
                    self.close_failed_turn(
                        run_id,
                        &turn_id,
                        &assistant_message_id,
                        "error",
                        &text_acc,
                        &reasoning_acc,
                    )?;
                    return Err(EngineError::Provider {
                        message: "provider returned empty response".into(),
                        code: "EMPTY_RESPONSE".into(),
                        retryable: false,
                        category: "unknown".into(),
                        retry_after_ms: None,
                    });
                }

                self.append_critical(run_id, RunEventKind::GenerationAttemptCommitted { attempt })?;
                break (text_acc, reasoning_acc, tool_acc, completed_reason);
            };

            if !text_acc.is_empty() {
                doom.observe_text(&text_acc);
            }
            if let Some(reason) = doom.diagnose() {
                self.close_failed_turn(
                    run_id,
                    &turn_id,
                    &assistant_message_id,
                    "error",
                    &text_acc,
                    &reasoning_acc,
                )?;
                return Err(EngineError::DoomLoop(reason));
            }

            if tool_acc.is_empty() {
                // Commit the assistant message before consuming a follow-up.
                // Otherwise the next provider request would lose the response
                // that caused the safe-point transition.
                let mut assistant_content = Vec::new();
                if !reasoning_acc.is_empty() {
                    assistant_content.push(crate::ContentBlock::Thinking {
                        text: reasoning_acc.clone(),
                        signature: None,
                    });
                }
                if !text_acc.is_empty() {
                    assistant_content.push(crate::ContentBlock::Text {
                        text: text_acc.clone(),
                    });
                }
                typed_messages.push(crate::AgentMessage::Assistant(crate::AssistantMessage {
                    message_id: assistant_message_id.clone(),
                    content: assistant_content.clone(),
                    stop_reason: Some(
                        match stop_reason.clone().unwrap_or(ProviderStopReason::Stop) {
                            ProviderStopReason::Stop => crate::StopReason::Stop,
                            ProviderStopReason::ToolUse => crate::StopReason::ToolUse,
                            ProviderStopReason::Length => crate::StopReason::Length,
                            ProviderStopReason::Cancelled => crate::StopReason::Cancelled,
                            ProviderStopReason::Error => crate::StopReason::Error,
                            ProviderStopReason::Unknown(raw) => crate::StopReason::Provider(raw),
                        },
                    ),
                }));
                // Close the response before consuming a follow-up. The next
                // provider turn must never begin with an uncommitted prior
                // MessageStarted/TurnStarted pair.
                let stop_label = stop_reason_label(stop_reason.as_ref());
                self.append_critical(
                    run_id,
                    RunEventKind::MessageCompleted {
                        turn_id: turn_id.to_string(),
                        message_id: assistant_message_id.to_string(),
                        role: "assistant".into(),
                        content: Some(json!({
                            "message_id": assistant_message_id.to_string(),
                            "role": "assistant",
                            "content": assistant_content,
                        })),
                    },
                )?;
                self.append_critical(
                    run_id,
                    RunEventKind::TurnCompleted {
                        turn_id: turn_id.to_string(),
                        stop_reason: stop_label,
                        input_tokens: 0,
                        output_tokens: 0,
                    },
                )?;
                let follow_up_consumed = match self
                    .drain_inputs(
                        crate::PendingInputKind::FollowUp,
                        crate::DrainMode::All,
                        crate::InputSafePoint::BeforeRunEnd,
                        &mut typed_messages,
                        Some(turn_id.0.as_str()),
                    )
                    .await
                {
                    Ok(consumed) => consumed,
                    Err(error) => return Err(error),
                };
                if follow_up_consumed {
                    // Live-lane signal (STREAM-CONTRACT-V2): high-frequency
                    // Progress is ephemeral — never write it to the durable
                    // EventSequencer.
                    self.live.append(
                        run_id,
                        RunEventKind::Progress {
                            message: "follow_up_consumed".into(),
                            percentage: None,
                        },
                    );
                    continue;
                }
                // No tools — complete. Status commit is RunManager's job.
                let stop = self
                    .hooks
                    .dispatch(HookRequest {
                        event: HookEvent::Stop,
                        run_id: run_id.to_string(),
                        tool_name: None,
                        input: serde_json::json!({ "reason": "stop" }),
                    })
                    .await;
                if let Err(reason) = HookRegistry::aggregate_allow(&stop) {
                    let _ = self
                        .hooks
                        .dispatch(HookRequest {
                            event: HookEvent::StopFailure,
                            run_id: run_id.to_string(),
                            tool_name: None,
                            input: serde_json::json!({ "reason": reason }),
                        })
                        .await;
                    return Err(EngineError::Message(format!("stop hook denied: {reason}")));
                }
                return Ok(match stop_reason.as_ref() {
                    Some(ProviderStopReason::Cancelled) => EngineOutcome::Cancelled,
                    Some(ProviderStopReason::Error) => EngineOutcome::failed(
                        "PROVIDER_STOP_ERROR",
                        "provider ended the response with an error stop reason",
                        false,
                    ),
                    Some(ProviderStopReason::Unknown(raw)) => EngineOutcome::failed(
                        "UNKNOWN_PROVIDER_STOP_REASON",
                        format!("provider ended with unknown stop reason: {raw}"),
                        false,
                    ),
                    Some(reason) => EngineOutcome::completed(stop_reason_label(Some(reason))),
                    None => EngineOutcome::completed("unknown"),
                });
            }

            // Execute tools and continue loop.
            // (W9: the tool-batch execution moved to `execute_tool_turn` in
            // engine_tools.rs — prepared-call construction, hooks, safe points,
            // transcript commit, steering drain, compaction.)
            self.execute_tool_turn(
                run_id,
                &turn_id,
                &assistant_message_id,
                tool_acc,
                &stop_reason,
                &mut doom,
                &tool_capabilities,
                &config,
                provider,
                tools,
                &mut typed_messages,
                &text_acc,
                &reasoning_acc,
            )
            .await?;
        }
    }
}
