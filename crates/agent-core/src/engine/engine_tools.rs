//! Tool execution domain (W9 split from engine_core.rs): prepared-call
//! construction, capability-driven batching, and the typed transcript
//! assembly from executed results.

use super::conversion::{core_stop_reason, stop_reason_label, tool_args_fingerprint};
use super::engine_core::AgentEngine;
use super::error::EngineError;
use super::provider::{EngineProvider, EngineRunConfig, ProviderStopReason};
use super::tool_runtime::{
    EngineToolRuntime, ToolCapability, ToolExecutionMode, ToolExecutionResult,
};
use crate::doom_loop::DoomLoopDetector;
use crate::hooks::{HookDecision, HookEvent, HookRequest};
use assistant_protocol::v2::RunEventKind;
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// Tool call after PreToolUse hooks, ready for (possibly parallel) execution.
#[derive(Debug, Clone)]
pub(super) struct PreparedToolCall {
    pub id: String,
    pub name: String,
    pub args: String,
    pub input: Value,
    pub rejected: Option<ToolExecutionResult>,
    pub parallel_safe: bool,
    pub conflict_key: Option<String>,
}

/// Tool call after execution (or hook denial), in original order.
#[derive(Debug, Clone)]
pub(super) struct ExecutedToolCall {
    pub id: String,
    pub name: String,
    pub args: String,
    pub result: Option<ToolExecutionResult>,
    /// Stable ToolResult message id committed in `ToolCallCompleted` so the
    /// daemon's event→SQLite replay reuses it instead of inventing a new one.
    pub result_message_id: Option<String>,
}

pub(super) fn is_long_running_tool_result(result: &ToolExecutionResult) -> bool {
    result
        .output
        .get("status")
        .and_then(Value::as_str)
        .is_some_and(|status| matches!(status, "running" | "pending"))
}

impl AgentEngine {
    /// Execute prepared tool calls with capability-driven batching (max
    /// concurrency 4). Non-parallel tools and rejected calls stay serial;
    /// results keep original source order. The Core does not special-case tool
    /// names when deciding concurrency.
    pub(super) async fn execute_prepared_tools(
        &self,
        run_id: &str,
        turn_id: &str,
        message_id: &str,
        tools: &dyn EngineToolRuntime,
        prepared: Vec<PreparedToolCall>,
    ) -> Result<Vec<ExecutedToolCall>, EngineError> {
        use crate::session_coordinator::PARALLEL_SAFE_MAX_CONCURRENCY;
        use futures_util::stream::{self, StreamExt};

        let mut out: Vec<ExecutedToolCall> = Vec::with_capacity(prepared.len());
        // A batch is parallel only when every non-rejected call is explicitly
        // advertised ParallelSafe. One sequential/exclusive call serializes
        // the whole batch, preserving deterministic side-effect order.
        let mut conflict_keys = std::collections::HashSet::new();
        let batch_parallel = prepared.iter().all(|call| {
            if call.rejected.is_some() {
                return true;
            }
            if !call.parallel_safe {
                return false;
            }
            call.conflict_key
                .as_ref()
                .map(|key| conflict_keys.insert(key.clone()))
                .unwrap_or(true)
        });
        let mut i = 0;
        while i < prepared.len() {
            if let Some(result) = prepared[i].rejected.clone() {
                let keeps_progress = is_long_running_tool_result(&result);
                let result_message_id = crate::MessageId::new().to_string();
                out.push(ExecutedToolCall {
                    id: prepared[i].id.clone(),
                    name: prepared[i].name.clone(),
                    args: prepared[i].args.clone(),
                    result: Some(result.clone()),
                    result_message_id: Some(result_message_id.clone()),
                });
                if let Err(error) = self.append_critical(
                    run_id,
                    RunEventKind::ToolCallCompleted {
                        id: prepared[i].id.clone(),
                        name: prepared[i].name.clone(),
                        output: result.output,
                        is_error: true,
                        duration_ms: 0,
                        result_message_id: Some(result_message_id),
                    },
                ) {
                    let uncertain = tools
                        .mark_tool_call_uncertain(
                            &prepared[i].id,
                            &prepared[i].name,
                            Some(turn_id),
                            &serde_json::from_str(&prepared[i].args)
                                .unwrap_or(Value::String(prepared[i].args.clone())),
                        )
                        .await;
                    self.cancel.cancel();
                    self.progress_sink
                        .mark_tool_call_settled(&prepared[i].id)
                        .await;
                    if let Err(uncertain_error) = uncertain {
                        return Err(EngineError::RecoveryBlocked(format!(
                            "completion fact AND uncertain ledger recording both failed: completion={error}; uncertain={uncertain_error}"
                        )));
                    }
                    return Err(error);
                }
                if !keeps_progress {
                    self.progress_sink
                        .mark_tool_call_settled(&prepared[i].id)
                        .await;
                }
                i += 1;
                continue;
            }

            // Gather a contiguous parallel_safe run (cap concurrency).
            if batch_parallel && prepared[i].parallel_safe {
                let mut batch = Vec::new();
                while i < prepared.len()
                    && prepared[i].parallel_safe
                    && prepared[i].rejected.is_none()
                    && batch.len() < PARALLEL_SAFE_MAX_CONCURRENCY
                {
                    batch.push(prepared[i].clone());
                    i += 1;
                }

                let cancel = self.cancel.clone();
                let results: Vec<(usize, ToolExecutionResult)> =
                    stream::iter(batch.iter().cloned().enumerate().map(|(idx, call)| {
                        let cancel = cancel.clone();
                        async move {
                            let result = tools
                                .execute_tool_with_progress_for_call(
                                    &call.id,
                                    Some(turn_id),
                                    Some(message_id),
                                    &call.name,
                                    call.input.clone(),
                                    &cancel,
                                    self.progress_sink.clone(),
                                )
                                .await;
                            (idx, result)
                        }
                    }))
                    .buffer_unordered(PARALLEL_SAFE_MAX_CONCURRENCY)
                    .collect()
                    .await;

                let mut by_idx: Vec<Option<ToolExecutionResult>> =
                    (0..batch.len()).map(|_| None).collect();
                for (idx, result) in results {
                    if idx < by_idx.len() {
                        by_idx[idx] = Some(result);
                    }
                }

                for (idx, call) in batch.into_iter().enumerate() {
                    let result = by_idx[idx].take().unwrap_or(ToolExecutionResult {
                        output: json!({"error": "missing parallel result"}),
                        is_error: true,
                        duration_ms: 0,
                    });
                    let result_message_id = crate::MessageId::new().to_string();
                    if let Err(error) = self.append_critical(
                        run_id,
                        RunEventKind::ToolCallCompleted {
                            id: call.id.clone(),
                            name: call.name.clone(),
                            output: result.output.clone(),
                            is_error: result.is_error,
                            duration_ms: result.duration_ms,
                            result_message_id: Some(result_message_id.clone()),
                        },
                    ) {
                        let uncertain = tools
                            .mark_tool_call_uncertain(
                                &call.id,
                                &call.name,
                                Some(turn_id),
                                &call.input,
                            )
                            .await;
                        self.cancel.cancel();
                        self.progress_sink.mark_tool_call_settled(&call.id).await;
                        if let Err(uncertain_error) = uncertain {
                            return Err(EngineError::RecoveryBlocked(format!(
                                "completion fact AND uncertain ledger recording both failed: completion={error}; uncertain={uncertain_error}"
                            )));
                        }
                        return Err(error);
                    }
                    if !is_long_running_tool_result(&result) {
                        self.progress_sink.mark_tool_call_settled(&call.id).await;
                    }
                    let post_event = if result.is_error {
                        HookEvent::PostToolUseFailure
                    } else {
                        HookEvent::PostToolUse
                    };
                    self.hooks
                        .observe(HookRequest {
                            event: post_event,
                            run_id: run_id.to_string(),
                            tool_name: Some(call.name.clone()),
                            input: json!({ "input": call.input, "output": result.output }),
                        })
                        .await
                        .map_err(|refusal| match refusal {
                            crate::hooks::ObserveResult::Failed { reason } => {
                                EngineError::HookRefused(reason)
                            }
                            crate::hooks::ObserveResult::Observe => {
                                EngineError::HookRefused("post-hook observation failed".into())
                            }
                        })?;
                    out.push(ExecutedToolCall {
                        id: call.id,
                        name: call.name,
                        args: call.args,
                        result: Some(result),
                        result_message_id: Some(result_message_id),
                    });
                }
                continue;
            }

            // Serial path for write / process / network.
            let call = &prepared[i];
            let result = tools
                .execute_tool_with_progress_for_call(
                    &call.id,
                    Some(turn_id),
                    Some(message_id),
                    &call.name,
                    call.input.clone(),
                    &self.cancel,
                    self.progress_sink.clone(),
                )
                .await;
            let result_message_id = crate::MessageId::new().to_string();
            if let Err(error) = self.append_critical(
                run_id,
                RunEventKind::ToolCallCompleted {
                    id: call.id.clone(),
                    name: call.name.clone(),
                    output: result.output.clone(),
                    is_error: result.is_error,
                    duration_ms: result.duration_ms,
                    result_message_id: Some(result_message_id.clone()),
                },
            ) {
                let uncertain = tools
                    .mark_tool_call_uncertain(&call.id, &call.name, Some(turn_id), &call.input)
                    .await;
                self.cancel.cancel();
                self.progress_sink.mark_tool_call_settled(&call.id).await;
                if let Err(uncertain_error) = uncertain {
                    return Err(EngineError::RecoveryBlocked(format!(
                        "completion fact AND uncertain ledger recording both failed: completion={error}; uncertain={uncertain_error}"
                    )));
                }
                return Err(error);
            }
            if !is_long_running_tool_result(&result) {
                self.progress_sink.mark_tool_call_settled(&call.id).await;
            }
            let post_event = if result.is_error {
                HookEvent::PostToolUseFailure
            } else {
                HookEvent::PostToolUse
            };
            self.hooks
                .observe(HookRequest {
                    event: post_event,
                    run_id: run_id.to_string(),
                    tool_name: Some(call.name.clone()),
                    input: json!({ "input": call.input, "output": result.output }),
                })
                .await
                .map_err(|refusal| match refusal {
                    crate::hooks::ObserveResult::Failed { reason } => {
                        EngineError::HookRefused(reason)
                    }
                    crate::hooks::ObserveResult::Observe => {
                        EngineError::HookRefused("post-hook observation failed".into())
                    }
                })?;
            out.push(ExecutedToolCall {
                id: call.id.clone(),
                name: call.name.clone(),
                args: call.args.clone(),
                result: Some(result),
                result_message_id: Some(result_message_id),
            });
            i += 1;
        }
        Ok(out)
    }

    /// Execute the tool batch assembled from a provider turn and continue the
    /// run loop: prepare calls (fail-closed on stop reason), run hooks and
    /// safe points, commit the assistant/tool-result transcript, then drain
    /// steering inputs and compact before the next provider turn.
    pub(super) async fn execute_tool_turn(
        &self,
        run_id: &str,
        turn_id: &crate::TurnId,
        assistant_message_id: &crate::MessageId,
        tool_acc: BTreeMap<usize, (String, String, String)>,
        stop_reason: &Option<ProviderStopReason>,
        doom: &mut DoomLoopDetector,
        tool_capabilities: &BTreeMap<String, ToolCapability>,
        config: &EngineRunConfig,
        provider: &dyn EngineProvider,
        tools: &dyn EngineToolRuntime,
        typed_messages: &mut Vec<crate::AgentMessage>,
        text_acc: &str,
        reasoning_acc: &str,
    ) -> Result<(), EngineError> {
        // Phase 2: parallel_safe readonly tools may run concurrently (max 4);
        // write / process / network stay serial. Results are filled in call order.
        // No user input is injected while a provider response is being
        // prepared; the next safe point is after the complete tool batch.
        let mut prepared: Vec<PreparedToolCall> = Vec::new();
        // A tool call is executable only when the provider explicitly says
        // that the turn ended for tool use. Every other stop reason is a
        // fail-closed boundary, including provider errors/cancellation and
        // legacy streams that omit a final event.
        let fail_closed_reason = match stop_reason.as_ref() {
            Some(ProviderStopReason::ToolUse) => None,
            Some(ProviderStopReason::Length) => Some((
                "TRUNCATED_TOOL_CALL",
                "provider output was truncated before the tool call could be executed",
            )),
            Some(ProviderStopReason::Cancelled) => Some((
                "CANCELLED_TOOL_CALL",
                "provider cancelled before the tool call could be executed",
            )),
            Some(ProviderStopReason::Error) => Some((
                "PROVIDER_ERROR_TOOL_CALL",
                "provider ended with an error before the tool call could be executed",
            )),
            Some(ProviderStopReason::Stop) => Some((
                "INVALID_TOOL_CALL_STOP_REASON",
                "provider stopped without authorizing tool execution",
            )),
            Some(ProviderStopReason::Unknown(_)) | None => Some((
                "UNKNOWN_PROVIDER_STOP_REASON",
                "provider did not provide a reliable stop reason; tool call was not executed",
            )),
        };
        for (_index, (id, name, args)) in tool_acc {
            let id = if id.is_empty() {
                uuid::Uuid::new_v4().to_string()
            } else {
                id
            };
            let (mut input, mut rejected) = match serde_json::from_str(&args) {
                Ok(value) => (value, None),
                Err(error) => (
                    json!({}),
                    Some(ToolExecutionResult {
                        output: json!({
                            "error_code": "INVALID_TOOL_ARGUMENTS",
                            "error": format!("tool arguments are not valid JSON: {error}"),
                        }),
                        is_error: true,
                        duration_ms: 0,
                    }),
                ),
            };
            if let Some((code, message)) = fail_closed_reason {
                rejected = Some(ToolExecutionResult {
                    output: json!({"error_code": code, "error": message}),
                    is_error: true,
                    duration_ms: 0,
                });
            }
            // PreToolUse hooks may deny or modify arguments (always serial).
            let pre = self
                .hooks
                .dispatch(HookRequest {
                    event: HookEvent::PreToolUse,
                    run_id: run_id.to_string(),
                    tool_name: Some(name.clone()),
                    input: input.clone(),
                })
                .await;
            let mut deny_reason: Option<String> = None;
            for response in pre {
                match response.decision {
                    HookDecision::Deny { reason } => {
                        deny_reason = Some(reason);
                    }
                    HookDecision::Modify { payload } => {
                        input = payload;
                    }
                    _ => {}
                }
            }
            if let Some(reason) = deny_reason {
                rejected = Some(ToolExecutionResult {
                    output: json!({
                        "error_code": "HOOK_DENIED",
                        "error": reason,
                        "denied_by_hook": true
                    }),
                    is_error: true,
                    duration_ms: 0,
                });
            }
            let args = serde_json::to_string(&input).map_err(|error| {
                EngineError::Message(format!("tool arguments are not JSON: {error}"))
            })?;
            doom.observe_tool(&name, &tool_args_fingerprint(&args));
            if let Some(reason) = doom.diagnose() {
                return Err(EngineError::DoomLoop(reason));
            }

            self.append_critical(
                run_id,
                RunEventKind::ToolCallRequested {
                    id: id.clone(),
                    name: name.clone(),
                    input: input.clone(),
                },
            )?;
            let capability = tool_capabilities.get(&name);
            self.append_critical(
                run_id,
                RunEventKind::ToolCallPrepared {
                    id: id.clone(),
                    name: name.clone(),
                    input: input.clone(),
                    execution_mode: capability
                        .map(|value| format!("{:?}", value.execution_mode))
                        .unwrap_or_else(|| "Sequential".into()),
                    side_effect: capability
                        .map(|value| format!("{:?}", value.side_effect))
                        .unwrap_or_else(|| "Destructive".into()),
                },
            )?;
            let parallel_safe = matches!(
                capability.map(|value| value.execution_mode),
                Some(ToolExecutionMode::ParallelSafe)
            );
            prepared.push(PreparedToolCall {
                id,
                name,
                args,
                input,
                rejected,
                parallel_safe,
                conflict_key: capability.and_then(|value| value.conflict_key.clone()),
            });
        }

        // Safe point: BeforeTool — immediately before the tool batch
        // executes. Distinct from AfterTool (below), which fires after the
        // batch and its transcript are committed. A queued interjection is
        // injected into provider history before the tool calls appear. This
        // is a real dispatch, and the topology truth test
        // (harness_topology_truth.rs) fails if this call site disappears.
        self.apply_safe_point(
            &config.conversation_id,
            crate::session_coordinator::SafePoint::BeforeTool,
            typed_messages,
        )
        .await?;

        let executed = self
            .execute_prepared_tools(
                run_id,
                turn_id.0.as_str(),
                assistant_message_id.0.as_str(),
                tools,
                prepared,
            )
            .await?;
        let persistence_failed = executed.iter().any(|item| {
            item.result.as_ref().is_some_and(|result| {
                result.output.get("error_code").and_then(Value::as_str)
                    == Some("PERSISTENCE_FAILED")
            })
        });

        let mut typed_tool_calls = Vec::new();
        let mut typed_tool_results = Vec::new();
        for item in executed {
            typed_tool_calls.push(crate::ContentBlock::ToolCall(crate::ToolCall {
                tool_call_id: crate::ToolCallId::from(item.id.clone()),
                name: item.name.clone(),
                arguments_json: item.args.clone(),
            }));
            if let Some(result) = item.result {
                let code = result
                    .output
                    .get("error_code")
                    .or_else(|| result.output.get("code"))
                    .and_then(Value::as_str)
                    .map(str::to_string);
                let output = result.output;
                let result_block = output
                    .get("artifact_id")
                    .and_then(Value::as_str)
                    .map(|artifact_id| crate::ToolResultBlock::Artifact {
                        artifact_id: artifact_id.to_string(),
                        preview: output
                            .get("preview")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                    })
                    .unwrap_or(crate::ToolResultBlock::Json { value: output });
                typed_tool_results.push(crate::AgentMessage::ToolResult(
                    crate::ToolResultMessage {
                        message_id: item
                            .result_message_id
                            .map(crate::MessageId::from)
                            .unwrap_or_else(crate::MessageId::new),
                        tool_call_id: crate::ToolCallId::from(item.id),
                        tool_name: item.name,
                        content: vec![result_block],
                        is_error: result.is_error,
                        code,
                    },
                ));
            }
        }

        let assistant_content = {
            let mut content = Vec::new();
            if !reasoning_acc.is_empty() {
                content.push(crate::ContentBlock::Thinking {
                    text: reasoning_acc.to_string(),
                    signature: None,
                });
            }
            if !text_acc.is_empty() {
                content.push(crate::ContentBlock::Text {
                    text: text_acc.to_string(),
                });
            }
            content.extend(typed_tool_calls);
            content
        };
        typed_messages.push(crate::AgentMessage::Assistant(crate::AssistantMessage {
            message_id: assistant_message_id.clone(),
            content: assistant_content.clone(),
            stop_reason: Some(core_stop_reason(stop_reason.as_ref())),
        }));
        typed_messages.extend(typed_tool_results);

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
                stop_reason: stop_reason_label(stop_reason.as_ref()),
                input_tokens: 0,
                output_tokens: 0,
            },
        )?;

        if persistence_failed {
            self.cancel.cancel();
            return Err(EngineError::Message(
                "critical tool persistence failed; run stopped fail-closed".into(),
            ));
        }

        // Safe point: the tool batch and its assistant/tool-result
        // transcript are fully committed before steering is leased. A
        // queue failure therefore cannot leave an open turn behind.
        self.apply_safe_point(
            &config.conversation_id,
            crate::session_coordinator::SafePoint::AfterTool,
            typed_messages,
        )
        .await?;
        self.drain_inputs(
            crate::PendingInputKind::Steering,
            crate::DrainMode::All,
            crate::InputSafePoint::AfterToolBatch,
            typed_messages,
            Some(turn_id.0.as_str()),
        )
        .await?;

        // Compact large tool outputs + repair dangling tool_call_ids before
        // the next provider turn (no isolated tool calls).
        // Safe point: ProviderBatchBoundary — between tool batch and next provider turn.
        let compacted = self
            .maybe_compact_typed_history(
                run_id,
                turn_id,
                &config.model,
                provider,
                std::mem::take(typed_messages),
            )
            .await?;
        *typed_messages = compacted;
        self.apply_safe_point(
            &config.conversation_id,
            crate::session_coordinator::SafePoint::ProviderBatchBoundary,
            typed_messages,
        )
        .await?;
        Ok(())
    }
}
