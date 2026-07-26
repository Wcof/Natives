//! Agent Engine — single execution authority for a Run.
//!
//! Flow: prepare → provider stream → (tool loop) → terminal event.
//! Tools and credentials are injected via seams so the daemon can supply
//! capability-gateway + credential broker without circular deps.

use crate::compaction::{
    apply_model_summary, choose_summary_split, compact_messages as compact_tool_history,
    render_transcript_for_summary, repair_dangling_tool_calls, CompactResult,
    SUMMARY_SYSTEM_PROMPT,
};
use crate::doom_loop::DoomLoopDetector;
use crate::event_seq::EventSequencer;
use crate::hooks::{HookDecision, HookEvent, HookRegistry, HookRequest};
use assistant_protocol::v2::{RunEventKind};
use futures_util::{Stream, StreamExt};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::pin::Pin;
use std::sync::atomic::{AtomicU32, Ordering as AtomicOrdering};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// Soft budget for in-engine history characters before tool-output compaction.
const HISTORY_COMPACT_CHARS: usize = 48_000;
const TOOL_OUTPUT_MAX_CHARS: usize = 4_000;

/// Messages kept verbatim at the tail when a model summary replaces the prefix.
const SUMMARY_KEEP_TAIL_MESSAGES: usize = 6;
/// Below this many summarizable messages a provider round trip is not worth it.
const SUMMARY_MIN_PREFIX_MESSAGES: usize = 4;
/// Upper bound on the transcript handed to the summarizer (cost boundary).
const SUMMARY_TRANSCRIPT_MAX_CHARS: usize = 60_000;
/// Per-message truncation inside that transcript.
const SUMMARY_MESSAGE_MAX_CHARS: usize = 2_000;
/// Wall clock ceiling for one summarization round trip.
const SUMMARY_TIMEOUT_MS: u64 = 60_000;
/// Total model summarizations attempted by one engine, successful or not.
const SUMMARY_MAX_ATTEMPTS: u32 = 8;
/// After this many failures the engine stops paying for summarization and
/// stays on mechanical compaction for the rest of the run.
const SUMMARY_MAX_FAILURES: u32 = 2;

/// Tool call after PreToolUse hooks, ready for (possibly parallel) execution.
#[derive(Debug, Clone)]
struct PreparedToolCall {
    id: String,
    name: String,
    args: String,
    input: Value,
    denied: bool,
    parallel_safe: bool,
}

/// Tool call after execution (or hook denial), in original order.
#[derive(Debug, Clone)]
struct ExecutedToolCall {
    id: String,
    name: String,
    args: String,
    denied: bool,
    result: Option<ToolExecutionResult>,
}

/// Tool execution seam used by the engine.
#[async_trait::async_trait]
pub trait EngineToolRuntime: Send + Sync {
    async fn list_tool_schemas(&self) -> Vec<ToolSchema>;
    async fn execute_tool(
        &self,
        name: &str,
        input: Value,
        cancel: &CancellationToken,
    ) -> ToolExecutionResult;

    /// Optional batch entry for same-turn `task` tool calls.
    ///
    /// Default falls back to sequential `execute_tool("task", ...)`.
    /// Native Runtime overrides this to emit **one** `subagent_assignment`
    /// interaction for the whole batch, then start children by `call_id`.
    async fn execute_task_batch(
        &self,
        tasks: Vec<(String, Value)>,
        cancel: &CancellationToken,
    ) -> Vec<ToolExecutionResult> {
        let mut out = Vec::with_capacity(tasks.len());
        for (_call_id, input) in tasks {
            if cancel.is_cancelled() {
                out.push(ToolExecutionResult {
                    output: json!({"error": "cancelled"}),
                    is_error: true,
                    duration_ms: 0,
                });
                continue;
            }
            out.push(self.execute_tool("task", input, cancel).await);
        }
        out
    }
}

#[derive(Debug, Clone)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Debug, Clone)]
pub struct ToolExecutionResult {
    pub output: Value,
    pub is_error: bool,
    pub duration_ms: u64,
}

/// Provider stream seam (maps onto provider-adapters without hard dep).
#[async_trait::async_trait]
pub trait EngineProvider: Send + Sync {
    async fn stream(
        &self,
        model: &str,
        messages: Vec<EngineMessage>,
        tools: &[ToolSchema],
        system_prompt: Option<&str>,
        cancel: CancellationToken,
    ) -> Result<EngineProviderEventStream, EngineError>;
}

pub type EngineProviderEventStream =
    Pin<Box<dyn Stream<Item = EngineProviderEvent> + Send + 'static>>;

#[derive(Debug, Clone)]
pub struct EngineMessage {
    pub role: String,
    pub content: String,
    pub tool_call_id: Option<String>,
    /// Tool name for `role: tool` results (needed by Gemini functionResponse).
    pub tool_name: Option<String>,
    pub tool_calls: Option<Vec<EngineToolCall>>,
}

#[derive(Debug, Clone)]
pub struct EngineToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone)]
pub enum EngineProviderEvent {
    TextDelta(String),
    ReasoningDelta(String),
    ToolCallDelta {
        index: usize,
        id: Option<String>,
        name: Option<String>,
        arguments_delta: String,
    },
    Usage {
        input_tokens: u64,
        output_tokens: u64,
        reasoning_tokens: Option<u64>,
        /// Prompt-cache writes reported by the provider, when it reports them.
        ///
        /// `None` (not reported) is deliberately distinct from `Some(0)` (the
        /// provider reported no cache activity) — the daemon persists the two
        /// differently, and collapsing them would make an unsupported provider
        /// indistinguishable from a cache miss.
        cache_creation_tokens: Option<u64>,
        /// Prompt-cache reads reported by the provider, when it reports them.
        cache_read_tokens: Option<u64>,
    },
    Completed,
    Error {
        message: String,
        code: String,
        retryable: bool,
        category: String,
        retry_after_ms: Option<u64>,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("{0}")]
    Message(String),
    #[error("{message}")]
    Provider {
        message: String,
        code: String,
        retryable: bool,
        category: String,
        retry_after_ms: Option<u64>,
    },
    #[error("cancelled")]
    Cancelled,
    #[error("doom loop detected")]
    DoomLoop,
    #[error("max steps exceeded")]
    MaxSteps,
}

impl EngineError {
    pub fn code(&self) -> &str {
        match self {
            Self::Provider { code, .. } => code,
            Self::Cancelled => "cancelled",
            Self::DoomLoop => "doom_loop",
            Self::MaxSteps => "max_steps",
            Self::Message(_) => "provider",
        }
    }

    pub fn retryable(&self) -> bool {
        matches!(self, Self::Provider { retryable: true, .. })
    }

    pub fn is_rate_limited(&self) -> bool {
        matches!(self, Self::Provider { category, .. } if category == "RateLimit")
    }
}

/// Configuration for one engine run.
#[derive(Debug, Clone)]
pub struct EngineRunConfig {
    pub run_id: String,
    pub conversation_id: String,
    pub model: String,
    pub system_prompt: Option<String>,
    pub messages: Vec<EngineMessage>,
    pub user_content: String,
    pub max_steps: u32,
}

/// Live run handle.
pub struct AgentEngine {
    pub events: EventSequencer,
    cancel: CancellationToken,
    hooks: HookRegistry,
    /// Optional session coordinator for interjection / safe-point drain.
    session_harness: Option<Arc<crate::session_coordinator::SessionCoordinator>>,
    /// Optional soft char budget override for history compaction.
    history_compact_chars: Option<usize>,
    /// Optional max chars kept per tool output after compaction.
    tool_output_max_chars: Option<usize>,
    /// Ask the model for a structured summary when compacting (default on).
    model_compaction: bool,
    summary_attempts: AtomicU32,
    summary_failures: AtomicU32,
}

impl AgentEngine {
    pub fn new(events: EventSequencer) -> Self {
        Self {
            events,
            cancel: CancellationToken::new(),
            hooks: HookRegistry::new(),
            session_harness: None,
            history_compact_chars: None,
            tool_output_max_chars: None,
            model_compaction: true,
            summary_attempts: AtomicU32::new(0),
            summary_failures: AtomicU32::new(0),
        }
    }

    /// Disable the summarization round trip and keep compaction mechanical.
    /// Useful for cost-sensitive or offline runs; failure already degrades here.
    pub fn with_model_compaction(mut self, enabled: bool) -> Self {
        self.model_compaction = enabled;
        self
    }

    /// Use a registry-owned cancel token (task-03). Prefer over the engine-local root.
    pub fn with_cancel_token(mut self, cancel: CancellationToken) -> Self {
        self.cancel = cancel;
        self
    }

    pub fn with_hooks(mut self, hooks: HookRegistry) -> Self {
        self.hooks = hooks;
        self
    }

    pub fn with_session_harness(
        mut self,
        harness: Arc<crate::session_coordinator::SessionCoordinator>,
    ) -> Self {
        self.session_harness = Some(harness);
        self
    }

    /// Alias of [`Self::with_session_harness`] using the coordinator name.
    pub fn with_session_coordinator(
        self,
        coordinator: Arc<crate::session_coordinator::SessionCoordinator>,
    ) -> Self {
        self.with_session_harness(coordinator)
    }

    /// Override in-engine history compaction thresholds (Context Budget).
    pub fn with_context_budget(mut self, history_chars: usize, tool_output_chars: usize) -> Self {
        self.history_compact_chars = Some(history_chars.max(1_000));
        self.tool_output_max_chars = Some(tool_output_chars.max(256));
        self
    }

    /// Shared cancel token for this engine run (clone freely; cancel is cooperative).
    pub fn cancel_token(&self) -> CancellationToken {
        self.cancel.clone()
    }

    /// Compatibility alias for [`Self::cancel_token`].
    /// Prefer `cancel_token()` / `request_cancel()` / `is_cancelled()`.
    pub fn cancel_flag(&self) -> CancellationToken {
        self.cancel.clone()
    }

    pub fn request_cancel(&self) {
        self.cancel.cancel();
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    /// Apply coordinator action at a safe point: inject interjection into messages.
    fn apply_safe_point(
        &self,
        conversation_id: &str,
        point: crate::session_coordinator::SafePoint,
        messages: &mut Vec<EngineMessage>,
    ) {
        let Some(harness) = &self.session_harness else {
            return;
        };
        match harness.on_safe_point(conversation_id, point) {
            crate::session_coordinator::CoordinatorAction::InjectInterjection { content } => {
                messages.push(EngineMessage {
                    role: "user".into(),
                    content: format!("[interjection]\n{content}"),
                    tool_call_id: None,
                    tool_name: None,
                    tool_calls: None,
                });
            }
            _ => {}
        }
    }

    /// Execute a full agent loop against the given seams.
    pub async fn run(
        &self,
        config: EngineRunConfig,
        provider: &dyn EngineProvider,
        tools: &dyn EngineToolRuntime,
    ) -> Result<crate::EngineOutcome, EngineError> {
        let run_id = config.run_id.clone();
        let result = self.run_inner(config, provider, tools).await;
        if let Err(error) = &result {
            let _ = self
                .hooks
                .dispatch(HookRequest {
                    event: HookEvent::Error,
                    run_id: run_id.clone(),
                    tool_name: None,
                    input: serde_json::json!({
                        "code": error.code(),
                        "message": error.to_string(),
                    }),
                })
                .await;
        }
        let _ = self
            .hooks
            .dispatch(HookRequest {
                event: HookEvent::SessionEnd,
                run_id,
                tool_name: None,
                input: serde_json::json!({
                    "success": result.is_ok(),
                }),
            })
            .await;
        result
    }

    async fn run_inner(
        &self,
        mut config: EngineRunConfig,
        provider: &dyn EngineProvider,
        tools: &dyn EngineToolRuntime,
    ) -> Result<crate::EngineOutcome, EngineError> {
        use crate::EngineOutcome;
        let run_id_owned = config.run_id.clone();
        let run_id = &run_id_owned;
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
        apply_prompt_hook_responses(&mut config, session_start)?;
        let prompt_submit = self
            .hooks
            .dispatch(HookRequest {
                event: HookEvent::UserPromptSubmit,
                run_id: run_id.to_string(),
                tool_name: None,
                input: serde_json::json!({ "content": config.user_content }),
            })
            .await;
        apply_prompt_hook_responses(&mut config, prompt_submit)?;

        let tool_schemas = tools.list_tool_schemas().await;
        // History is prior turns; always ensure the current user prompt appears
        // exactly once (append when history is empty or does not already end
        // with the same user content).
        let mut messages = if config.messages.is_empty() {
            vec![EngineMessage {
                role: "user".into(),
                content: config.user_content.clone(),
                tool_call_id: None,
                tool_name: None,
                tool_calls: None,
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
                });
            }
            msgs
        };
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

            const MAX_PROVIDER_ATTEMPTS: u32 = 3;
            let mut attempt = 1u32;
            let (text_acc, tool_acc) = 'attempts: loop {
                self.events.append(
                    run_id,
                    RunEventKind::GenerationAttemptStarted {
                        attempt,
                        max_attempts: MAX_PROVIDER_ATTEMPTS,
                    },
                );

                let provider_events = match provider
                    .stream(
                        &config.model,
                        messages.clone(),
                        &tool_schemas,
                        config.system_prompt.as_deref(),
                        self.cancel.clone(),
                    )
                    .await
                {
                    Ok(stream) => stream,
                    Err(EngineError::Cancelled) => {
                        return Ok(EngineOutcome::Cancelled);
                    }
                    Err(_e) if self.cancel.is_cancelled() => {
                        return Ok(EngineOutcome::Cancelled);
                    }
                    Err(e) if e.retryable() && attempt < MAX_PROVIDER_ATTEMPTS => {
                        self.events.append(
                            run_id,
                            RunEventKind::GenerationAttemptFailed {
                                attempt,
                                code: e.code().into(),
                                retryable: true,
                                retrying: true,
                            },
                        );
                        if !e.is_rate_limited() {
                            sleep_provider_backoff(attempt).await;
                        }
                        attempt += 1;
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
                            },
                        );
                        return Err(e);
                    }
                };

                let mut text_acc = String::new();
                let mut tool_acc: BTreeMap<usize, (String, String, String)> = BTreeMap::new();
                let mut saw_generation_delta = false;
                tokio::pin!(provider_events);

                while let Some(event) = provider_events.next().await {
                    if self.cancel.is_cancelled() {
                        return Ok(EngineOutcome::Cancelled);
                    }
                    match event {
                        EngineProviderEvent::TextDelta(t) => {
                            saw_generation_delta = true;
                            text_acc.push_str(&t);
                            self.events
                                .append(run_id, RunEventKind::TextDelta { text: t });
                        }
                        EngineProviderEvent::ReasoningDelta(t) => {
                            saw_generation_delta = true;
                            self.events
                                .append(run_id, RunEventKind::ReasoningDelta { text: t });
                        }
                        EngineProviderEvent::ToolCallDelta {
                            index,
                            id,
                            name,
                            arguments_delta,
                        } => {
                            saw_generation_delta = true;
                            let entry = tool_acc.entry(index).or_insert_with(|| {
                                (String::new(), String::new(), String::new())
                            });
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
                            self.events.append(
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
                            if !saw_generation_delta && retryable && attempt < MAX_PROVIDER_ATTEMPTS {
                                self.events.append(
                                    run_id,
                                    RunEventKind::GenerationAttemptFailed {
                                        attempt,
                                        code: code.clone(),
                                        retryable,
                                        retrying: true,
                                    },
                                );
                                if category != "RateLimit" {
                                    sleep_provider_backoff(attempt).await;
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
                                },
                            );
                            return Err(EngineError::Provider {
                                message,
                                code,
                                retryable,
                                category,
                                retry_after_ms,
                            });
                        }
                        EngineProviderEvent::Completed => {}
                    }
                }

                if self.cancel.is_cancelled() {
                    return Ok(EngineOutcome::Cancelled);
                }

                if !saw_generation_delta && attempt < 2 {
                    self.events.append(
                        run_id,
                        RunEventKind::GenerationAttemptFailed {
                            attempt,
                            code: "EMPTY_RESPONSE".into(),
                            retryable: true,
                            retrying: true,
                        },
                    );
                    sleep_provider_backoff(attempt).await;
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
                        },
                    );
                    return Err(EngineError::Provider {
                        message: "provider returned empty response".into(),
                        code: "EMPTY_RESPONSE".into(),
                        retryable: false,
                        category: "unknown".into(),
                        retry_after_ms: None,
                    });
                }

                self.events.append(
                    run_id,
                    RunEventKind::GenerationAttemptCommitted { attempt },
                );
                break (text_acc, tool_acc);
            };

            if !text_acc.is_empty() {
                doom.observe_text(&text_acc);
            }
            if doom.is_doom_loop() {
                return Err(EngineError::DoomLoop);
            }

            if tool_acc.is_empty() {
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
                    return Err(EngineError::Message(format!(
                        "stop hook denied: {reason}"
                    )));
                }
                return Ok(EngineOutcome::completed("stop"));
            }

            // Execute tools and continue loop.
            // Phase 2: parallel_safe readonly tools may run concurrently (max 4);
            // write / process / network stay serial. Results are filled in call order.
            // Safe point: before any tool in this batch.
            self.apply_safe_point(
                &config.conversation_id,
                crate::session_coordinator::SafePoint::BeforeTool,
                &mut messages,
            );
            let mut prepared: Vec<PreparedToolCall> = Vec::new();
            for (_index, (id, name, args)) in tool_acc {
                let id = if id.is_empty() {
                    uuid::Uuid::new_v4().to_string()
                } else {
                    id
                };
                let mut input: Value = serde_json::from_str(&args).unwrap_or(serde_json::json!({
                    "raw": args
                }));
                doom.observe_tool(&name, &args.chars().take(80).collect::<String>());
                if doom.is_doom_loop() {
                    return Err(EngineError::DoomLoop);
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
                let mut denied = false;
                let mut deny_reason: Option<String> = None;
                for response in pre {
                    match response.decision {
                        HookDecision::Deny { reason } => {
                            denied = true;
                            deny_reason = Some(reason);
                        }
                        HookDecision::Modify { payload } => {
                            input = payload;
                        }
                        _ => {}
                    }
                }
                if denied {
                    let reason = deny_reason.unwrap_or_else(|| "denied".into());
                    self.events.append(
                        run_id,
                        RunEventKind::ToolCallCompleted {
                            id: id.clone(),
                            name: name.clone(),
                            output: serde_json::json!({ "error": reason, "denied_by_hook": true }),
                            is_error: true,
                            duration_ms: 0,
                        },
                    );
                    prepared.push(PreparedToolCall {
                        id,
                        name,
                        args,
                        input,
                        denied: true,
                        parallel_safe: false,
                    });
                    continue;
                }

                self.events.append(
                    run_id,
                    RunEventKind::ToolCallRequested {
                        id: id.clone(),
                        name: name.clone(),
                        input: input.clone(),
                    },
                );
                self.events.append(
                    run_id,
                    RunEventKind::ToolCallStarted {
                        id: id.clone(),
                        name: name.clone(),
                    },
                );

                let parallel_safe = crate::session_coordinator::is_parallel_safe_tool(&name);
                prepared.push(PreparedToolCall {
                    id,
                    name,
                    args,
                    input,
                    denied: false,
                    parallel_safe,
                });
            }

            let executed =
                self.execute_prepared_tools(run_id, tools, prepared).await;

            // Safe point: after tool batch completes.
            self.apply_safe_point(
                &config.conversation_id,
                crate::session_coordinator::SafePoint::AfterTool,
                &mut messages,
            );

            let mut assistant_tool_calls = Vec::new();
            let mut tool_results = Vec::new();
            for item in executed {
                // Denied by PreToolUse: already emitted ToolCallCompleted; skip pair.
                if item.denied {
                    continue;
                }
                assistant_tool_calls.push(EngineToolCall {
                    id: item.id.clone(),
                    name: item.name.clone(),
                    arguments: item.args,
                });
                tool_results.push(EngineMessage {
                    role: "tool".into(),
                    content: item
                        .result
                        .as_ref()
                        .map(|r| r.output.to_string())
                        .unwrap_or_else(|| "{}".into()),
                    tool_call_id: Some(item.id),
                    tool_name: Some(item.name),
                    tool_calls: None,
                });
            }

            messages.push(EngineMessage {
                role: "assistant".into(),
                content: text_acc,
                tool_call_id: None,
                tool_name: None,
                tool_calls: Some(assistant_tool_calls),
            });
            messages.extend(tool_results);

            // Compact large tool outputs + repair dangling tool_call_ids before
            // the next provider turn (no isolated tool calls).
            // Safe point: ProviderBatchBoundary — between tool batch and next provider turn.
            messages = self
                .maybe_compact_history(run_id, &config.model, provider, messages)
                .await;
            self.apply_safe_point(
                &config.conversation_id,
                crate::session_coordinator::SafePoint::ProviderBatchBoundary,
                &mut messages,
            );
        }
    }

    /// Execute prepared tool calls with parallel_safe batching (max concurrency 4).
    /// Contiguous same-turn `task` tools are executed via `execute_task_batch`.
    /// Non-parallel tools and denied hooks stay serial. Results keep original order.
    async fn execute_prepared_tools(
        &self,
        run_id: &str,
        tools: &dyn EngineToolRuntime,
        prepared: Vec<PreparedToolCall>,
    ) -> Vec<ExecutedToolCall> {
        use crate::session_coordinator::PARALLEL_SAFE_MAX_CONCURRENCY;
        use futures_util::stream::{self, StreamExt};

        let mut out: Vec<ExecutedToolCall> = Vec::with_capacity(prepared.len());
        let mut i = 0;
        while i < prepared.len() {
            if prepared[i].denied {
                out.push(ExecutedToolCall {
                    id: prepared[i].id.clone(),
                    name: prepared[i].name.clone(),
                    args: prepared[i].args.clone(),
                    denied: true,
                    result: None,
                });
                i += 1;
                continue;
            }

            // Contiguous non-denied `task` tools → one batch assignment.
            if prepared[i].name == "task" {
                let mut batch = Vec::new();
                while i < prepared.len()
                    && prepared[i].name == "task"
                    && !prepared[i].denied
                {
                    batch.push(prepared[i].clone());
                    i += 1;
                }
                let cancel = self.cancel.clone();
                let task_inputs: Vec<(String, Value)> = batch
                    .iter()
                    .map(|c| (c.id.clone(), c.input.clone()))
                    .collect();
                let results = tools.execute_task_batch(task_inputs, &cancel).await;
                for (idx, call) in batch.into_iter().enumerate() {
                    let result = results.get(idx).cloned().unwrap_or(ToolExecutionResult {
                        output: json!({"error": "missing task batch result"}),
                        is_error: true,
                        duration_ms: 0,
                    });
                    let post_event = if result.is_error {
                        HookEvent::PostToolUseFailure
                    } else {
                        HookEvent::PostToolUse
                    };
                    let _ = self
                        .hooks
                        .dispatch(HookRequest {
                            event: post_event,
                            run_id: run_id.to_string(),
                            tool_name: Some(call.name.clone()),
                            input: json!({ "input": call.input, "output": result.output }),
                        })
                        .await;
                    self.events.append(
                        run_id,
                        RunEventKind::ToolCallCompleted {
                            id: call.id.clone(),
                            name: call.name.clone(),
                            output: result.output.clone(),
                            is_error: result.is_error,
                            duration_ms: result.duration_ms,
                        },
                    );
                    out.push(ExecutedToolCall {
                        id: call.id,
                        name: call.name,
                        args: call.args,
                        denied: false,
                        result: Some(result),
                    });
                }
                continue;
            }

            // Gather a contiguous parallel_safe run (cap concurrency).
            if prepared[i].parallel_safe {
                let mut batch = Vec::new();
                while i < prepared.len()
                    && prepared[i].parallel_safe
                    && !prepared[i].denied
                    && batch.len() < PARALLEL_SAFE_MAX_CONCURRENCY
                {
                    batch.push(prepared[i].clone());
                    i += 1;
                }

                let cancel = self.cancel.clone();
                let results: Vec<(usize, ToolExecutionResult)> = stream::iter(
                    batch
                        .iter()
                        .cloned()
                        .enumerate()
                        .map(|(idx, call)| {
                            let cancel = cancel.clone();
                            async move {
                                let result = tools
                                    .execute_tool(&call.name, call.input.clone(), &cancel)
                                    .await;
                                (idx, result)
                            }
                        }),
                )
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
                    let post_event = if result.is_error {
                        HookEvent::PostToolUseFailure
                    } else {
                        HookEvent::PostToolUse
                    };
                    let _ = self
                        .hooks
                        .dispatch(HookRequest {
                            event: post_event,
                            run_id: run_id.to_string(),
                            tool_name: Some(call.name.clone()),
                            input: json!({ "input": call.input, "output": result.output }),
                        })
                        .await;
                    self.events.append(
                        run_id,
                        RunEventKind::ToolCallCompleted {
                            id: call.id.clone(),
                            name: call.name.clone(),
                            output: result.output.clone(),
                            is_error: result.is_error,
                            duration_ms: result.duration_ms,
                        },
                    );
                    out.push(ExecutedToolCall {
                        id: call.id,
                        name: call.name,
                        args: call.args,
                        denied: false,
                        result: Some(result),
                    });
                }
                continue;
            }

            // Serial path for write / process / network.
            let call = &prepared[i];
            let result = tools
                .execute_tool(&call.name, call.input.clone(), &self.cancel)
                .await;
            let post_event = if result.is_error {
                HookEvent::PostToolUseFailure
            } else {
                HookEvent::PostToolUse
            };
            let _ = self
                .hooks
                .dispatch(HookRequest {
                    event: post_event,
                    run_id: run_id.to_string(),
                    tool_name: Some(call.name.clone()),
                    input: json!({ "input": call.input, "output": result.output }),
                })
                .await;
            self.events.append(
                run_id,
                RunEventKind::ToolCallCompleted {
                    id: call.id.clone(),
                    name: call.name.clone(),
                    output: result.output.clone(),
                    is_error: result.is_error,
                    duration_ms: result.duration_ms,
                },
            );
            out.push(ExecutedToolCall {
                id: call.id.clone(),
                name: call.name.clone(),
                args: call.args.clone(),
                denied: false,
                result: Some(result),
            });
            i += 1;
        }
        out
    }

    /// Convert engine history → JSON messages, compact, convert back.
    ///
    /// Over budget the engine first asks the model for a structured summary of
    /// the old prefix (see [`crate::compaction::SUMMARY_SYSTEM_PROMPT`]) and
    /// keeps only `[summary] + recent tail`. Every failure path — provider
    /// error, timeout, cancellation, empty answer, budget exhausted — falls
    /// back to mechanical compaction. Compaction never fails a Run.
    async fn maybe_compact_history(
        &self,
        run_id: &str,
        model: &str,
        provider: &dyn EngineProvider,
        messages: Vec<EngineMessage>,
    ) -> Vec<EngineMessage> {
        let history_limit = self
            .history_compact_chars
            .unwrap_or(HISTORY_COMPACT_CHARS);
        let tool_limit = self.tool_output_max_chars.unwrap_or(TOOL_OUTPUT_MAX_CHARS);
        let before_chars: usize = messages.iter().map(|m| m.content.len()).sum();
        if before_chars < history_limit {
            // Still repair dangling pairs cheaply.
            let values = engine_messages_to_values(&messages);
            let (fixed, repaired) = repair_dangling_tool_calls(&values);
            if repaired == 0 {
                return messages;
            }
            return values_to_engine_messages(&fixed);
        }

        let pre_compact = self
            .hooks
            .dispatch(HookRequest {
                event: HookEvent::PreCompact,
                run_id: run_id.to_string(),
                tool_name: None,
                input: json!({
                    "before_chars": before_chars,
                    "messages": messages.len(),
                    "model_compaction": self.model_compaction,
                }),
            })
            .await;
        if HookRegistry::aggregate_allow(&pre_compact).is_err() {
            return messages;
        }

        let values = engine_messages_to_values(&messages);
        let result = match self
            .try_model_summary(model, provider, &values, tool_limit)
            .await
        {
            Some(summarized) => summarized,
            None => compact_tool_history(&values, tool_limit),
        };
        let mode = if result.summarized_messages > 0 {
            "model"
        } else {
            "mechanical"
        };
        let after_chars: usize = result
            .messages
            .iter()
            .map(|m| {
                m.get("content")
                    .and_then(|c| c.as_str())
                    .map(|s| s.len())
                    .unwrap_or(0)
            })
            .sum();

        self.events.append(
            run_id,
            RunEventKind::ContextCompressed {
                before_tokens: (before_chars as u64 / 4).max(1),
                after_tokens: (after_chars as u64 / 4).max(1),
                summary: result.summary.clone(),
            },
        );

        // PostCompact carries the full compaction record. The dispatch result is
        // intentionally ignored: HookDecision has no channel for writing history
        // back, so a hook cannot (and must not appear to) alter the outcome.
        let _ = self
            .hooks
            .dispatch(HookRequest {
                event: HookEvent::PostCompact,
                run_id: run_id.to_string(),
                tool_name: None,
                input: json!({
                    "mode": mode,
                    "before_chars": before_chars,
                    "after_chars": after_chars,
                    "dropped_tool_outputs": result.dropped_tool_outputs,
                    "repaired_dangling": result.repaired_dangling,
                    "summarized_messages": result.summarized_messages,
                    "summary": result.summary,
                }),
            })
            .await;

        values_to_engine_messages(&result.messages)
    }

    /// Model-backed compaction: `Some` only when a usable summary came back.
    ///
    /// Returning `None` is the documented degradation path and the caller
    /// answers it with mechanical compaction.
    async fn try_model_summary(
        &self,
        model: &str,
        provider: &dyn EngineProvider,
        values: &[Value],
        tool_limit: usize,
    ) -> Option<CompactResult> {
        if !self.model_compaction || self.cancel.is_cancelled() {
            return None;
        }
        // Cost boundary: bounded attempts per engine, and a failure streak
        // permanently drops the run back to mechanical compaction.
        if self.summary_attempts.load(AtomicOrdering::SeqCst) >= SUMMARY_MAX_ATTEMPTS
            || self.summary_failures.load(AtomicOrdering::SeqCst) >= SUMMARY_MAX_FAILURES
        {
            return None;
        }
        let split = choose_summary_split(values, SUMMARY_KEEP_TAIL_MESSAGES);
        if split < SUMMARY_MIN_PREFIX_MESSAGES {
            // Too little history to be worth a round trip.
            return None;
        }

        self.summary_attempts.fetch_add(1, AtomicOrdering::SeqCst);
        let transcript = render_transcript_for_summary(
            &values[..split],
            SUMMARY_TRANSCRIPT_MAX_CHARS,
            SUMMARY_MESSAGE_MAX_CHARS,
        );
        let request = vec![EngineMessage {
            role: "user".into(),
            content: transcript,
            tool_call_id: None,
            tool_name: None,
            tool_calls: None,
        }];

        match self.stream_summary_text(model, provider, request).await {
            Ok(summary) if !summary.trim().is_empty() => {
                Some(apply_model_summary(values, split, &summary, tool_limit))
            }
            _ => {
                self.summary_failures.fetch_add(1, AtomicOrdering::SeqCst);
                None
            }
        }
    }

    /// One isolated provider round trip that yields plain summary text.
    ///
    /// Isolated in three ways: no tools (so it cannot start a tool loop), a
    /// throwaway message vector (so it never touches the run history), and no
    /// event emission (so the summary does not surface as assistant output).
    /// Bounded by the run cancel token and a wall-clock timeout.
    async fn stream_summary_text(
        &self,
        model: &str,
        provider: &dyn EngineProvider,
        messages: Vec<EngineMessage>,
    ) -> Result<String, String> {
        let cancel = self.cancel.clone();
        let collect = async {
            let mut stream = provider
                .stream(
                    model,
                    messages,
                    &[],
                    Some(SUMMARY_SYSTEM_PROMPT),
                    cancel.clone(),
                )
                .await
                .map_err(|e| e.to_string())?;
            let mut text = String::new();
            while let Some(event) = stream.next().await {
                if cancel.is_cancelled() {
                    return Err("cancelled".to_string());
                }
                match event {
                    EngineProviderEvent::TextDelta(delta) => text.push_str(&delta),
                    EngineProviderEvent::Error { message, .. } => return Err(message),
                    _ => {}
                }
            }
            Ok(text)
        };

        tokio::select! {
            biased;
            _ = self.cancel.cancelled() => Err("cancelled".to_string()),
            outcome = tokio::time::timeout(
                std::time::Duration::from_millis(SUMMARY_TIMEOUT_MS),
                collect,
            ) => outcome.unwrap_or_else(|_| Err("summary request timed out".to_string())),
        }
    }
}

fn apply_prompt_hook_responses(
    config: &mut EngineRunConfig,
    responses: Vec<crate::hooks::HookResponse>,
) -> Result<(), EngineError> {
    for response in responses {
        match response.decision {
            HookDecision::Deny { reason } => {
                return Err(EngineError::Message(format!("hook denied: {reason}")));
            }
            HookDecision::Modify { payload } => {
                if let Some(content) = payload.get("content").and_then(Value::as_str) {
                    config.user_content = content.to_string();
                }
            }
            HookDecision::Inject { messages } => {
                config.messages.extend(messages.into_iter().map(|content| EngineMessage {
                    role: "system".into(),
                    content,
                    tool_call_id: None,
                    tool_name: None,
                    tool_calls: None,
                }));
            }
            HookDecision::Allow | HookDecision::Rewake => {}
        }
    }
    Ok(())
}

async fn sleep_provider_backoff(attempt: u32) {
    let backoff_ms = match attempt {
        1 => 500,
        2 => 1_000,
        _ => 2_000,
    };
    tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;
}

fn engine_messages_to_values(messages: &[EngineMessage]) -> Vec<Value> {
    messages
        .iter()
        .map(|m| {
            let mut obj = serde_json::Map::new();
            obj.insert("role".into(), json!(m.role));
            obj.insert("content".into(), json!(m.content));
            if let Some(id) = &m.tool_call_id {
                obj.insert("tool_call_id".into(), json!(id));
            }
            if let Some(name) = &m.tool_name {
                obj.insert("name".into(), json!(name));
            }
            if let Some(calls) = &m.tool_calls {
                let arr: Vec<Value> = calls
                    .iter()
                    .map(|c| {
                        json!({
                            "id": c.id,
                            "type": "function",
                            "function": {
                                "name": c.name,
                                "arguments": c.arguments,
                            }
                        })
                    })
                    .collect();
                obj.insert("tool_calls".into(), Value::Array(arr));
            }
            Value::Object(obj)
        })
        .collect()
}

fn values_to_engine_messages(values: &[Value]) -> Vec<EngineMessage> {
    values
        .iter()
        .map(|v| {
            let role = v
                .get("role")
                .and_then(|x| x.as_str())
                .unwrap_or("user")
                .to_string();
            let content = v
                .get("content")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let tool_call_id = v
                .get("tool_call_id")
                .and_then(|x| x.as_str())
                .map(str::to_string);
            let tool_name = v
                .get("name")
                .and_then(|x| x.as_str())
                .map(str::to_string);
            let tool_calls = v.get("tool_calls").and_then(|x| x.as_array()).map(|arr| {
                arr.iter()
                    .filter_map(|c| {
                        let id = c.get("id")?.as_str()?.to_string();
                        let name = c
                            .get("function")
                            .and_then(|f| f.get("name"))
                            .and_then(|n| n.as_str())
                            .unwrap_or("")
                            .to_string();
                        let arguments = c
                            .get("function")
                            .and_then(|f| f.get("arguments"))
                            .and_then(|a| a.as_str())
                            .unwrap_or("{}")
                            .to_string();
                        Some(EngineToolCall {
                            id,
                            name,
                            arguments,
                        })
                    })
                    .collect()
            });
            EngineMessage {
                role,
                content,
                tool_call_id,
                tool_name,
                tool_calls,
            }
        })
        .collect()
}


#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use super::*;
    use std::sync::{Arc, Mutex};

    struct FakeProvider {
        rounds: Mutex<Vec<Vec<EngineProviderEvent>>>,
    }

    #[async_trait::async_trait]
    impl EngineProvider for FakeProvider {
        async fn stream(
            &self,
            _model: &str,
            _messages: Vec<EngineMessage>,
            _tools: &[ToolSchema],
            _system_prompt: Option<&str>,
            _cancel: CancellationToken,
        ) -> Result<EngineProviderEventStream, EngineError> {
            let mut rounds = self.rounds.lock().unwrap();
            let events = if rounds.is_empty() {
                vec![
                    EngineProviderEvent::TextDelta("done".into()),
                    EngineProviderEvent::Completed,
                ]
            } else {
                rounds.remove(0)
            };
            Ok(Box::pin(futures_util::stream::iter(events)))
        }
    }

    struct FakeTools;

    #[async_trait::async_trait]
    impl EngineToolRuntime for FakeTools {
        async fn list_tool_schemas(&self) -> Vec<ToolSchema> {
            vec![ToolSchema {
                name: "echo".into(),
                description: "echo".into(),
                input_schema: serde_json::json!({"type":"object"}),
            }]
        }
        async fn execute_tool(
            &self,
            name: &str,
            input: Value,
            _cancel: &CancellationToken,
        ) -> ToolExecutionResult {
            ToolExecutionResult {
                output: serde_json::json!({"tool": name, "input": input}),
                is_error: false,
                duration_ms: 1,
            }
        }
    }

    #[tokio::test]
    async fn session_end_hook_fires_after_success() {
        struct RecordingHook(Arc<Mutex<Vec<HookEvent>>>);

        #[async_trait::async_trait]
        impl crate::hooks::HookHandler for RecordingHook {
            async fn handle(
                &self,
                request: HookRequest,
            ) -> crate::hooks::HookResponse {
                self.0.lock().unwrap().push(request.event);
                crate::hooks::HookResponse {
                    decision: HookDecision::Allow,
                }
            }
        }

        let seen = Arc::new(Mutex::new(Vec::new()));
        let mut hooks = HookRegistry::new();
        hooks.register(
            HookEvent::SessionEnd,
            Box::new(RecordingHook(seen.clone())),
        );
        let engine = AgentEngine::new(EventSequencer::new()).with_hooks(hooks);
        let provider = FakeProvider {
            rounds: Mutex::new(Vec::new()),
        };
        engine
            .run(
                EngineRunConfig {
                    run_id: "run-hook-end".into(),
                    conversation_id: "conversation-hook-end".into(),
                    model: "model".into(),
                    system_prompt: None,
                    messages: Vec::new(),
                    user_content: "hello".into(),
                    max_steps: 2,
                },
                &provider,
                &FakeTools,
            )
            .await
            .unwrap();
        assert_eq!(*seen.lock().unwrap(), vec![HookEvent::SessionEnd]);
    }

    #[tokio::test]
    async fn executes_task_batch_collects_all_call_ids() {
        use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

        struct BatchTools {
            batch_calls: AtomicUsize,
            single_task_calls: AtomicUsize,
        }

        #[async_trait::async_trait]
        impl EngineToolRuntime for BatchTools {
            async fn list_tool_schemas(&self) -> Vec<ToolSchema> {
                vec![ToolSchema {
                    name: "task".into(),
                    description: "task".into(),
                    input_schema: serde_json::json!({"type":"object"}),
                }]
            }
            async fn execute_tool(
                &self,
                name: &str,
                input: Value,
                _cancel: &CancellationToken,
            ) -> ToolExecutionResult {
                if name == "task" {
                    self.single_task_calls.fetch_add(1, AtomicOrdering::SeqCst);
                }
                ToolExecutionResult {
                    output: serde_json::json!({"tool": name, "input": input}),
                    is_error: false,
                    duration_ms: 1,
                }
            }
            async fn execute_task_batch(
                &self,
                tasks: Vec<(String, Value)>,
                _cancel: &CancellationToken,
            ) -> Vec<ToolExecutionResult> {
                self.batch_calls.fetch_add(1, AtomicOrdering::SeqCst);
                tasks
                    .into_iter()
                    .map(|(call_id, input)| ToolExecutionResult {
                        output: serde_json::json!({
                            "task_id": call_id,
                            "prompt": input.get("prompt"),
                            "status": "running",
                        }),
                        is_error: false,
                        duration_ms: 1,
                    })
                    .collect()
            }
        }

        let tools = BatchTools {
            batch_calls: AtomicUsize::new(0),
            single_task_calls: AtomicUsize::new(0),
        };
        let engine = AgentEngine::new(EventSequencer::new());
        let provider = FakeProvider {
            rounds: Mutex::new(vec![
                vec![
                    EngineProviderEvent::ToolCallDelta {
                        index: 0,
                        id: Some("c1".into()),
                        name: Some("task".into()),
                        arguments_delta: r#"{"prompt":"a"}"#.into(),
                    },
                    EngineProviderEvent::ToolCallDelta {
                        index: 1,
                        id: Some("c2".into()),
                        name: Some("task".into()),
                        arguments_delta: r#"{"prompt":"b"}"#.into(),
                    },
                    EngineProviderEvent::ToolCallDelta {
                        index: 2,
                        id: Some("c3".into()),
                        name: Some("task".into()),
                        arguments_delta: r#"{"prompt":"c"}"#.into(),
                    },
                    EngineProviderEvent::Completed,
                ],
                vec![
                    EngineProviderEvent::TextDelta("done".into()),
                    EngineProviderEvent::Completed,
                ],
            ]),
        };
        let run_id = format!("r-batch-{}", uuid::Uuid::new_v4());
        let conversation_id = format!("c-batch-{}", uuid::Uuid::new_v4());
        let status = engine
            .run(
                EngineRunConfig {
                    run_id: run_id.clone(),
                    conversation_id,
                    model: "m".into(),
                    system_prompt: None,
                    messages: Vec::new(),
                    user_content: "spawn 3".into(),
                    max_steps: 5,
                },
                &provider,
                &tools,
            )
            .await
            .unwrap();
        assert!(matches!(status, crate::EngineOutcome::Completed { .. }), "{status:?}");
        assert_eq!(tools.batch_calls.load(AtomicOrdering::SeqCst), 1);
        assert_eq!(tools.single_task_calls.load(AtomicOrdering::SeqCst), 0);
        let events = engine.events.replay_after(&run_id, 0);
        let completed: Vec<_> = events
            .iter()
            .filter_map(|e| match &e.payload {
                RunEventKind::ToolCallCompleted { id, output, .. } => {
                    Some((id.clone(), output.clone()))
                }
                _ => None,
            })
            .collect();
        assert_eq!(completed.len(), 3);
        assert_eq!(completed[0].0, "c1");
        assert_eq!(completed[1].0, "c2");
        assert_eq!(completed[2].0, "c3");
    }

    #[tokio::test]
    async fn completes_simple_text_turn() {
        let engine = AgentEngine::new(EventSequencer::new());
        let provider = FakeProvider {
            rounds: Mutex::new(vec![vec![
                EngineProviderEvent::TextDelta("hello".into()),
                EngineProviderEvent::Completed,
            ]]),
        };
        let status = engine
            .run(
                EngineRunConfig {
                    run_id: "r1".into(),
                    conversation_id: "c1".into(),
                    model: "m".into(),
                    system_prompt: None,
                    messages: Vec::new(),
                    user_content: "hi".into(),
                    max_steps: 5,
                },
                &provider,
                &FakeTools,
            )
            .await
            .unwrap();
        assert!(matches!(status, crate::EngineOutcome::Completed { .. }), "{status:?}");
        let events = engine.events.replay_after("r1", 0);
        assert!(events.iter().any(|e| matches!(e.payload, RunEventKind::Started)));
        assert!(events
            .iter()
            .any(|e| matches!(e.payload, RunEventKind::TextDelta { .. })));
        assert!(events
            .iter()
            .any(|e| matches!(e.payload, RunEventKind::Completed { .. }))
            || matches!(status, crate::EngineOutcome::Completed { .. }));
    }

    #[tokio::test]
    async fn provider_receives_configured_history_messages() {
        struct CapturingProvider {
            seen: Mutex<Vec<EngineMessage>>,
        }
        #[async_trait::async_trait]
        impl EngineProvider for CapturingProvider {
            async fn stream(
                &self,
                _model: &str,
                messages: Vec<EngineMessage>,
                _tools: &[ToolSchema],
                _system_prompt: Option<&str>,
                _cancel: CancellationToken,
            ) -> Result<EngineProviderEventStream, EngineError> {
                *self.seen.lock().unwrap() = messages;
                Ok(Box::pin(futures_util::stream::iter(vec![
                    EngineProviderEvent::TextDelta("ok".into()),
                    EngineProviderEvent::Completed,
                ])))
            }
        }

        let engine = AgentEngine::new(EventSequencer::new());
        let provider = CapturingProvider {
            seen: Mutex::new(Vec::new()),
        };
        engine
            .run(
                EngineRunConfig {
                    run_id: "history-run".into(),
                    conversation_id: "c1".into(),
                    model: "m".into(),
                    system_prompt: None,
                    messages: vec![
                        EngineMessage {
                            role: "user".into(),
                            content: "first fact".into(),
                            tool_call_id: None,
                            tool_name: None,
                            tool_calls: None,
                        },
                        EngineMessage {
                            role: "assistant".into(),
                            content: "ack".into(),
                            tool_call_id: None,
                            tool_name: None,
                            tool_calls: None,
                        },
                    ],
                    user_content: "fallback should not be used".into(),
                    max_steps: 5,
                },
                &provider,
                &FakeTools,
            )
            .await
            .unwrap();
        let seen = provider.seen.lock().unwrap();
        assert_eq!(seen.len(), 3);
        assert_eq!(seen[0].content, "first fact");
        assert_eq!(seen[1].content, "ack");
        assert_eq!(seen[2].content, "fallback should not be used");
        assert_eq!(seen[2].role, "user");
    }

    #[tokio::test]
    async fn executes_tool_then_completes() {
        let engine = AgentEngine::new(EventSequencer::new());
        let provider = FakeProvider {
            rounds: Mutex::new(vec![
                vec![
                    EngineProviderEvent::ToolCallDelta {
                        index: 0,
                        id: Some("t1".into()),
                        name: Some("echo".into()),
                        arguments_delta: r#"{"x":1}"#.into(),
                    },
                    EngineProviderEvent::Completed,
                ],
                vec![
                    EngineProviderEvent::TextDelta("after tool".into()),
                    EngineProviderEvent::Completed,
                ],
            ]),
        };
        let status = engine
            .run(
                EngineRunConfig {
                    run_id: "r2".into(),
                    conversation_id: "c1".into(),
                    model: "m".into(),
                    system_prompt: None,
                    messages: Vec::new(),
                    user_content: "use tool".into(),
                    max_steps: 5,
                },
                &provider,
                &FakeTools,
            )
            .await
            .unwrap();
        assert!(matches!(status, crate::EngineOutcome::Completed { .. }), "{status:?}");
        let events = engine.events.replay_after("r2", 0);
        assert!(events
            .iter()
            .any(|e| matches!(e.payload, RunEventKind::ToolCallCompleted { .. })));
    }

    #[tokio::test]
    async fn emits_text_delta_before_provider_stream_completes() {
        struct DelayedCompletionProvider;
        #[async_trait::async_trait]
        impl EngineProvider for DelayedCompletionProvider {
            async fn stream(
                &self,
                _model: &str,
                _messages: Vec<EngineMessage>,
                _tools: &[ToolSchema],
                _system_prompt: Option<&str>,
                _cancel: CancellationToken,
            ) -> Result<EngineProviderEventStream, EngineError> {
                Ok(Box::pin(futures_util::stream::unfold(0, |state| async move {
                    match state {
                        0 => Some((EngineProviderEvent::TextDelta("early".into()), 1)),
                        1 => {
                            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                            Some((EngineProviderEvent::Completed, 2))
                        }
                        _ => None,
                    }
                })))
            }
        }

        let engine = AgentEngine::new(EventSequencer::new());
        let events = engine.events.clone();
        let run_id = format!("r-stream-{}", uuid::Uuid::new_v4());
        let run_id_bg = run_id.clone();
        let handle = tokio::spawn(async move {
            engine
                .run(
                    EngineRunConfig {
                        run_id: run_id_bg,
                        conversation_id: "c1".into(),
                        model: "m".into(),
                        system_prompt: None,
                        messages: Vec::new(),
                        user_content: "hi".into(),
                        max_steps: 5,
                    },
                    &DelayedCompletionProvider,
                    &FakeTools,
                )
                .await
        });

        let mut saw_text_before_done = false;
        for _ in 0..100 {
            let current = events.replay_after(&run_id, 0);
            if current
                .iter()
                .any(|e| matches!(e.payload, RunEventKind::TextDelta { .. }))
                && !current
                    .iter()
                    .any(|e| matches!(e.payload, RunEventKind::Completed { .. }))
            {
                saw_text_before_done = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert!(saw_text_before_done, "text delta must be emitted before stream completion");
        handle.await.expect("join").expect("run");
    }

    #[tokio::test]
    async fn provider_stream_receives_cancel_and_run_interrupts_promptly() {
        struct CancelAwareProvider {
            seen: Arc<AtomicBool>,
        }

        #[async_trait::async_trait]
        impl EngineProvider for CancelAwareProvider {
            async fn stream(
                &self,
                _model: &str,
                _messages: Vec<EngineMessage>,
                _tools: &[ToolSchema],
                _system_prompt: Option<&str>,
                cancel: CancellationToken,
            ) -> Result<EngineProviderEventStream, EngineError> {
                let seen = self.seen.clone();
                Ok(Box::pin(futures_util::stream::unfold(
                    (0u8, cancel, seen),
                    |(state, cancel, seen)| async move {
                        if state == 0 {
                            return Some((
                                EngineProviderEvent::TextDelta("early".into()),
                                (1, cancel, seen),
                            ));
                        }
                        loop {
                            if cancel.is_cancelled() {
                                seen.store(true, Ordering::SeqCst);
                                return None;
                            }
                            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                        }
                    },
                )))
            }
        }

        let engine = AgentEngine::new(EventSequencer::new());
        let cancel = engine.cancel_flag();
        let events = engine.events.clone();
        let run_id = format!("r-provider-cancel-{}", uuid::Uuid::new_v4());
        let run_id_bg = run_id.clone();
        let provider_cancel_seen = Arc::new(AtomicBool::new(false));
        let provider_cancel_seen_bg = provider_cancel_seen.clone();
        let handle = tokio::spawn(async move {
            let provider = CancelAwareProvider {
                seen: provider_cancel_seen_bg,
            };
            engine
                .run(
                    EngineRunConfig {
                        run_id: run_id_bg,
                        conversation_id: "c1".into(),
                        model: "m".into(),
                        system_prompt: None,
                        messages: Vec::new(),
                        user_content: "hi".into(),
                        max_steps: 5,
                    },
                    &provider,
                    &FakeTools,
                )
                .await
        });

        let mut saw_text = false;
        for _ in 0..100 {
            if events
                .replay_after(&run_id, 0)
                .iter()
                .any(|e| matches!(e.payload, RunEventKind::TextDelta { .. }))
            {
                saw_text = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert!(saw_text, "test must observe first text delta before cancelling");
        cancel.cancel();

        let status = tokio::time::timeout(std::time::Duration::from_secs(1), handle)
            .await
            .expect("run should stop promptly after cancel")
            .expect("join")
            .expect("run");
        assert!(matches!(status, crate::EngineOutcome::Cancelled | crate::EngineOutcome::Interrupted { .. }), "{status:?}");
        assert!(
            provider_cancel_seen.load(Ordering::SeqCst),
            "provider stream must observe engine cancel flag"
        );
        let current = events.replay_after(&run_id, 0);
        // Lifecycle terminal events are owned by RunManager; engine only returns outcome.
        assert!(!current
            .iter()
            .any(|e| matches!(e.payload, RunEventKind::Completed { .. })));
    }

    #[tokio::test]
    async fn retries_retryable_provider_stream_open_errors() {
        struct FlakyProvider {
            attempts: std::sync::atomic::AtomicUsize,
        }

        #[async_trait::async_trait]
        impl EngineProvider for FlakyProvider {
            async fn stream(
                &self,
                _model: &str,
                _messages: Vec<EngineMessage>,
                _tools: &[ToolSchema],
                _system_prompt: Option<&str>,
                _cancel: CancellationToken,
            ) -> Result<EngineProviderEventStream, EngineError> {
                let attempt = self
                    .attempts
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                    + 1;
                if attempt < 3 {
                    return Err(EngineError::Provider {
                        message: "temporary provider failure".into(),
                        code: "http_503".into(),
                        retryable: true,
                        category: "ServerError".into(),
                        retry_after_ms: None,
                    });
                }
                Ok(Box::pin(futures_util::stream::iter(vec![
                    EngineProviderEvent::TextDelta("ok".into()),
                    EngineProviderEvent::Completed,
                ])))
            }
        }

        let engine = AgentEngine::new(EventSequencer::new());
        let run_id = format!("r-provider-retry-{}", uuid::Uuid::new_v4());
        let provider = FlakyProvider {
            attempts: std::sync::atomic::AtomicUsize::new(0),
        };
        let status = engine
            .run(
                EngineRunConfig {
                    run_id: run_id.clone(),
                    conversation_id: "c1".into(),
                    model: "m".into(),
                    system_prompt: None,
                    messages: Vec::new(),
                    user_content: "hi".into(),
                    max_steps: 5,
                },
                &provider,
                &FakeTools,
            )
            .await
            .unwrap();
        assert!(matches!(status, crate::EngineOutcome::Completed { .. }), "{status:?}");
        assert_eq!(
            provider.attempts.load(std::sync::atomic::Ordering::SeqCst),
            3
        );
        let events = engine.events.replay_after(&run_id, 0);
        let attempt_events = events
            .iter()
            .filter(|e| {
                matches!(
                    &e.payload,
                    RunEventKind::GenerationAttemptStarted { .. }
                )
            })
            .count();
        assert_eq!(attempt_events, 3);
        assert!(events.iter().any(|e| {
            matches!(
                &e.payload,
                RunEventKind::GenerationAttemptFailed {
                    attempt: 1,
                    code,
                    retryable: true,
                    retrying: true,
                } if code == "http_503"
            )
        }));
        assert!(events
            .iter()
            .any(|e| matches!(e.payload, RunEventKind::Completed { .. }))
            || matches!(status, crate::EngineOutcome::Completed { .. }));
        assert!(events.iter().any(|e| {
            matches!(
                e.payload,
                RunEventKind::GenerationAttemptCommitted { attempt: 3 }
            )
        }));
    }

    #[tokio::test]
    async fn retries_empty_provider_response_once() {
        let engine = AgentEngine::new(EventSequencer::new());
        let run_id = format!("r-empty-retry-{}", uuid::Uuid::new_v4());
        let provider = FakeProvider {
            rounds: Mutex::new(vec![
                vec![EngineProviderEvent::Completed],
                vec![
                    EngineProviderEvent::TextDelta("after empty".into()),
                    EngineProviderEvent::Completed,
                ],
            ]),
        };

        let status = engine
            .run(
                EngineRunConfig {
                    run_id: run_id.clone(),
                    conversation_id: "c1".into(),
                    model: "m".into(),
                    system_prompt: None,
                    messages: Vec::new(),
                    user_content: "hi".into(),
                    max_steps: 5,
                },
                &provider,
                &FakeTools,
            )
            .await
            .unwrap();
        assert!(matches!(status, crate::EngineOutcome::Completed { .. }), "{status:?}");
        let events = engine.events.replay_after(&run_id, 0);
        assert!(events.iter().any(|e| {
            matches!(
                &e.payload,
                RunEventKind::GenerationAttemptFailed {
                    attempt: 1,
                    code,
                    retrying: true,
                    ..
                } if code == "EMPTY_RESPONSE"
            )
        }));
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e.payload, RunEventKind::GenerationAttemptStarted { .. }))
                .count(),
            2
        );
        assert!(events.iter().any(|e| {
            matches!(
                e.payload,
                RunEventKind::GenerationAttemptCommitted { attempt: 2 }
            )
        }));
    }

    #[tokio::test]
    async fn retries_stream_error_before_first_delta() {
        let engine = AgentEngine::new(EventSequencer::new());
        let run_id = format!("r-pre-delta-error-{}", uuid::Uuid::new_v4());
        let provider = FakeProvider {
            rounds: Mutex::new(vec![
                vec![EngineProviderEvent::Error {
                    message: "upstream unavailable".into(),
                    code: "http_503".into(),
                    retryable: true,
                    category: "ServerError".into(),
                    retry_after_ms: None,
                }],
                vec![
                    EngineProviderEvent::TextDelta("ok".into()),
                    EngineProviderEvent::Completed,
                ],
            ]),
        };

        let status = engine
            .run(
                EngineRunConfig {
                    run_id: run_id.clone(),
                    conversation_id: "c1".into(),
                    model: "m".into(),
                    system_prompt: None,
                    messages: Vec::new(),
                    user_content: "hi".into(),
                    max_steps: 5,
                },
                &provider,
                &FakeTools,
            )
            .await
            .unwrap();
        assert!(matches!(status, crate::EngineOutcome::Completed { .. }), "{status:?}");
        let events = engine.events.replay_after(&run_id, 0);
        assert!(events.iter().any(|e| {
            matches!(
                &e.payload,
                RunEventKind::GenerationAttemptFailed {
                    attempt: 1,
                    code,
                    retryable: true,
                    retrying: true,
                } if code == "http_503"
            )
        }));
    }

    #[tokio::test]
    async fn discards_partial_generation_error_without_retrying() {
        let engine = AgentEngine::new(EventSequencer::new());
        let run_id = format!("r-partial-error-{}", uuid::Uuid::new_v4());
        let provider = FakeProvider {
            rounds: Mutex::new(vec![vec![
                EngineProviderEvent::TextDelta("partial".into()),
                EngineProviderEvent::Error {
                    message: "stream dropped".into(),
                    code: "http_503".into(),
                    retryable: true,
                    category: "ServerError".into(),
                    retry_after_ms: None,
                },
            ]]),
        };

        let err = engine
            .run(
                EngineRunConfig {
                    run_id: run_id.clone(),
                    conversation_id: "c1".into(),
                    model: "m".into(),
                    system_prompt: None,
                    messages: Vec::new(),
                    user_content: "hi".into(),
                    max_steps: 5,
                },
                &provider,
                &FakeTools,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, EngineError::Provider { .. }));
        let events = engine.events.replay_after(&run_id, 0);
        assert!(events
            .iter()
            .any(|e| matches!(e.payload, RunEventKind::GenerationAttemptDiscarded { .. })));
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e.payload, RunEventKind::GenerationAttemptStarted { .. }))
                .count(),
            1
        );
        assert!(!events
            .iter()
            .any(|e| matches!(e.payload, RunEventKind::Completed { .. })));
    }

    // -----------------------------------------------------------------------
    // Model-backed compaction
    // -----------------------------------------------------------------------

    const CANNED_SUMMARY: &str = "## Goal\nfix the parser\n## Completed\npatched lexer.rs\n\
## Current state\ntests green\n## Open questions\nnone\n## Next steps\nship";

    enum SummaryBehavior {
        /// Answer the summarization request with `CANNED_SUMMARY`.
        Answer,
        /// Fail the summarization request at stream open.
        Fail,
        /// Never answer; used to prove cancellation interrupts the request.
        Hang,
    }

    /// Provider that tells the summarization round trip apart from normal turns
    /// by its system prompt, and records both sides for assertions.
    struct CompactionProvider {
        rounds: Mutex<Vec<Vec<EngineProviderEvent>>>,
        behavior: SummaryBehavior,
        summary_requests: Mutex<Vec<String>>,
        summary_tools_empty: Arc<AtomicBool>,
        summary_started: Arc<AtomicBool>,
        main_requests: Mutex<Vec<Vec<EngineMessage>>>,
    }

    impl CompactionProvider {
        fn new(behavior: SummaryBehavior, rounds: Vec<Vec<EngineProviderEvent>>) -> Self {
            Self {
                rounds: Mutex::new(rounds),
                behavior,
                summary_requests: Mutex::new(Vec::new()),
                summary_tools_empty: Arc::new(AtomicBool::new(true)),
                summary_started: Arc::new(AtomicBool::new(false)),
                main_requests: Mutex::new(Vec::new()),
            }
        }

        fn summary_count(&self) -> usize {
            self.summary_requests.lock().unwrap().len()
        }

        fn last_main_history(&self) -> Vec<EngineMessage> {
            self.main_requests.lock().unwrap().last().cloned().unwrap()
        }
    }

    #[async_trait::async_trait]
    impl EngineProvider for CompactionProvider {
        async fn stream(
            &self,
            _model: &str,
            messages: Vec<EngineMessage>,
            tools: &[ToolSchema],
            system_prompt: Option<&str>,
            _cancel: CancellationToken,
        ) -> Result<EngineProviderEventStream, EngineError> {
            if system_prompt == Some(SUMMARY_SYSTEM_PROMPT) {
                self.summary_requests
                    .lock()
                    .unwrap()
                    .push(messages.first().map(|m| m.content.clone()).unwrap_or_default());
                if !tools.is_empty() {
                    self.summary_tools_empty.store(false, Ordering::SeqCst);
                }
                self.summary_started.store(true, Ordering::SeqCst);
                return match self.behavior {
                    SummaryBehavior::Answer => Ok(Box::pin(futures_util::stream::iter(vec![
                        EngineProviderEvent::TextDelta(CANNED_SUMMARY.into()),
                        EngineProviderEvent::Completed,
                    ]))),
                    SummaryBehavior::Fail => Err(EngineError::Provider {
                        message: "summarizer unavailable".into(),
                        code: "http_500".into(),
                        retryable: false,
                        category: "ServerError".into(),
                        retry_after_ms: None,
                    }),
                    SummaryBehavior::Hang => Ok(Box::pin(futures_util::stream::pending())),
                };
            }

            self.main_requests.lock().unwrap().push(messages);
            let mut rounds = self.rounds.lock().unwrap();
            let events = if rounds.is_empty() {
                vec![
                    EngineProviderEvent::TextDelta("done".into()),
                    EngineProviderEvent::Completed,
                ]
            } else {
                rounds.remove(0)
            };
            Ok(Box::pin(futures_util::stream::iter(events)))
        }
    }

    /// Tool runtime whose output is far larger than any test tool budget.
    struct BigOutputTools;

    #[async_trait::async_trait]
    impl EngineToolRuntime for BigOutputTools {
        async fn list_tool_schemas(&self) -> Vec<ToolSchema> {
            vec![ToolSchema {
                name: "echo".into(),
                description: "echo".into(),
                input_schema: serde_json::json!({"type":"object"}),
            }]
        }
        async fn execute_tool(
            &self,
            _name: &str,
            _input: Value,
            _cancel: &CancellationToken,
        ) -> ToolExecutionResult {
            ToolExecutionResult {
                output: serde_json::json!({ "body": "y".repeat(5_000) }),
                is_error: false,
                duration_ms: 1,
            }
        }
    }

    /// Prior turns long enough to blow a small history budget.
    fn long_history(turns: usize) -> Vec<EngineMessage> {
        (0..turns)
            .map(|i| EngineMessage {
                role: if i % 2 == 0 { "user".into() } else { "assistant".into() },
                content: format!("turn {i}: {}", "detail ".repeat(40)),
                tool_call_id: None,
                tool_name: None,
                tool_calls: None,
            })
            .collect()
    }

    /// One provider turn that calls `echo`. `n` keeps successive rounds
    /// distinct so the doom-loop detector stays out of these tests.
    fn tool_round_n(n: usize) -> Vec<EngineProviderEvent> {
        vec![
            EngineProviderEvent::ToolCallDelta {
                index: 0,
                id: Some(format!("t{n}")),
                name: Some("echo".into()),
                arguments_delta: format!(r#"{{"x":{n}}}"#),
            },
            EngineProviderEvent::Completed,
        ]
    }

    fn tool_round() -> Vec<EngineProviderEvent> {
        tool_round_n(1)
    }

    /// Every tool result must still be preceded by the assistant call that made it.
    fn assert_tool_pairs_intact(messages: &[EngineMessage]) {
        let mut open: std::collections::HashSet<String> = std::collections::HashSet::new();
        for m in messages {
            if let Some(calls) = &m.tool_calls {
                for call in calls {
                    open.insert(call.id.clone());
                }
            }
            if m.role == "tool" {
                let id = m.tool_call_id.clone().unwrap_or_default();
                assert!(
                    open.contains(&id),
                    "tool result {id} has no preceding assistant tool_call"
                );
            }
        }
    }

    #[tokio::test]
    async fn compaction_requests_model_summary_and_injects_it_into_history() {
        let engine = AgentEngine::new(EventSequencer::new()).with_context_budget(1_000, 512);
        let provider = CompactionProvider::new(
            SummaryBehavior::Answer,
            vec![
                tool_round(),
                vec![
                    EngineProviderEvent::TextDelta("done".into()),
                    EngineProviderEvent::Completed,
                ],
            ],
        );
        let run_id = format!("r-compact-model-{}", uuid::Uuid::new_v4());
        let status = engine
            .run(
                EngineRunConfig {
                    run_id: run_id.clone(),
                    conversation_id: "c-compact".into(),
                    model: "m".into(),
                    system_prompt: None,
                    messages: long_history(10),
                    user_content: "keep going".into(),
                    max_steps: 5,
                },
                &provider,
                &BigOutputTools,
            )
            .await
            .unwrap();
        assert!(matches!(status, crate::EngineOutcome::Completed { .. }), "{status:?}");

        assert_eq!(provider.summary_count(), 1, "exactly one summarization round trip");
        assert!(
            provider.summary_tools_empty.load(Ordering::SeqCst),
            "summarization must be sent without tools so it cannot start a tool loop"
        );
        let asked = provider.summary_requests.lock().unwrap()[0].clone();
        assert!(asked.contains("turn 0"), "transcript must carry the oldest turn");

        // The next provider turn sees the summary instead of the old prefix.
        let after = provider.last_main_history();
        let summary_msgs: Vec<_> = after
            .iter()
            .filter(|m| m.content.starts_with(crate::compaction::SUMMARY_MARKER))
            .collect();
        assert_eq!(summary_msgs.len(), 1, "history: {after:?}");
        assert!(summary_msgs[0].content.contains("## Next steps"));
        assert!(!after.iter().any(|m| m.content.contains("turn 0")), "prefix must be gone");
        assert!(after.len() <= SUMMARY_KEEP_TAIL_MESSAGES + 1);
        assert_tool_pairs_intact(&after);

        let events = engine.events.replay_after(&run_id, 0);
        let compressed: Vec<_> = events
            .iter()
            .filter_map(|e| match &e.payload {
                RunEventKind::ContextCompressed {
                    before_tokens,
                    after_tokens,
                    summary,
                } => Some((*before_tokens, *after_tokens, summary.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(compressed.len(), 1);
        assert!(compressed[0].2.contains("## Goal"), "event must carry the real summary");
        assert!(compressed[0].1 < compressed[0].0, "compaction must shrink the history");
    }

    #[tokio::test]
    async fn compaction_falls_back_to_mechanical_when_summary_fails() {
        let engine = AgentEngine::new(EventSequencer::new()).with_context_budget(1_000, 512);
        let provider = CompactionProvider::new(
            SummaryBehavior::Fail,
            vec![
                tool_round(),
                vec![
                    EngineProviderEvent::TextDelta("done".into()),
                    EngineProviderEvent::Completed,
                ],
            ],
        );
        let run_id = format!("r-compact-fallback-{}", uuid::Uuid::new_v4());
        let status = engine
            .run(
                EngineRunConfig {
                    run_id: run_id.clone(),
                    conversation_id: "c-compact".into(),
                    model: "m".into(),
                    system_prompt: None,
                    messages: long_history(10),
                    user_content: "keep going".into(),
                    max_steps: 5,
                },
                &provider,
                &BigOutputTools,
            )
            .await
            .unwrap();
        // The hard requirement: a failed summary never fails the Run.
        assert!(matches!(status, crate::EngineOutcome::Completed { .. }), "{status:?}");
        assert_eq!(provider.summary_count(), 1);

        let after = provider.last_main_history();
        assert!(
            !after
                .iter()
                .any(|m| m.content.starts_with(crate::compaction::SUMMARY_MARKER)),
            "no summary may be injected when the summarizer failed"
        );
        assert!(
            after.iter().any(|m| m.content.contains("turn 0")),
            "mechanical compaction keeps the turns it cannot summarize"
        );
        assert!(
            after
                .iter()
                .any(|m| m.role == "tool" && m.content.contains("truncated")),
            "mechanical compaction must still trim oversized tool output"
        );
        assert_tool_pairs_intact(&after);

        let events = engine.events.replay_after(&run_id, 0);
        assert!(
            events
                .iter()
                .any(|e| matches!(e.payload, RunEventKind::ContextCompressed { .. })),
            "the fallback is still an observable compaction"
        );
    }

    #[tokio::test]
    async fn compaction_summary_is_interrupted_by_cancel() {
        let engine = AgentEngine::new(EventSequencer::new()).with_context_budget(1_000, 512);
        let cancel = engine.cancel_token();
        let provider = Arc::new(CompactionProvider::new(
            SummaryBehavior::Hang,
            vec![tool_round()],
        ));
        let summary_started = provider.summary_started.clone();
        let provider_bg = provider.clone();
        let handle = tokio::spawn(async move {
            engine
                .run(
                    EngineRunConfig {
                        run_id: format!("r-compact-cancel-{}", uuid::Uuid::new_v4()),
                        conversation_id: "c-compact".into(),
                        model: "m".into(),
                        system_prompt: None,
                        messages: long_history(10),
                        user_content: "keep going".into(),
                        max_steps: 5,
                    },
                    provider_bg.as_ref(),
                    &BigOutputTools,
                )
                .await
        });

        let mut started = false;
        for _ in 0..200 {
            if summary_started.load(Ordering::SeqCst) {
                started = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert!(started, "test must observe the summarization request before cancelling");
        cancel.cancel();

        let status = tokio::time::timeout(std::time::Duration::from_secs(2), handle)
            .await
            .expect("a cancelled summary must not hold the run open")
            .expect("join")
            .expect("run");
        assert!(matches!(status, crate::EngineOutcome::Cancelled), "{status:?}");
    }

    #[tokio::test]
    async fn repeated_summary_failures_stop_paying_for_summarization() {
        let engine = AgentEngine::new(EventSequencer::new()).with_context_budget(1_000, 512);
        let provider = CompactionProvider::new(
            SummaryBehavior::Fail,
            vec![
                tool_round_n(1),
                tool_round_n(2),
                tool_round_n(3),
                tool_round_n(4),
            ],
        );
        let status = engine
            .run(
                EngineRunConfig {
                    run_id: format!("r-compact-budget-{}", uuid::Uuid::new_v4()),
                    conversation_id: "c-compact".into(),
                    model: "m".into(),
                    system_prompt: None,
                    messages: long_history(10),
                    user_content: "keep going".into(),
                    max_steps: 8,
                },
                &provider,
                &BigOutputTools,
            )
            .await
            .unwrap();
        assert!(matches!(status, crate::EngineOutcome::Completed { .. }), "{status:?}");
        assert_eq!(
            provider.summary_count(),
            SUMMARY_MAX_FAILURES as usize,
            "the engine must stop retrying a failing summarizer"
        );
    }

    #[tokio::test]
    async fn no_summary_round_trip_when_history_is_within_budget() {
        let engine = AgentEngine::new(EventSequencer::new());
        let provider = CompactionProvider::new(
            SummaryBehavior::Answer,
            vec![
                tool_round(),
                vec![
                    EngineProviderEvent::TextDelta("done".into()),
                    EngineProviderEvent::Completed,
                ],
            ],
        );
        let run_id = format!("r-compact-none-{}", uuid::Uuid::new_v4());
        engine
            .run(
                EngineRunConfig {
                    run_id: run_id.clone(),
                    conversation_id: "c-compact".into(),
                    model: "m".into(),
                    system_prompt: None,
                    messages: Vec::new(),
                    user_content: "hi".into(),
                    max_steps: 5,
                },
                &provider,
                &FakeTools,
            )
            .await
            .unwrap();
        assert_eq!(provider.summary_count(), 0);
        let events = engine.events.replay_after(&run_id, 0);
        assert!(!events
            .iter()
            .any(|e| matches!(e.payload, RunEventKind::ContextCompressed { .. })));
    }

    #[tokio::test]
    async fn model_compaction_can_be_disabled() {
        let engine = AgentEngine::new(EventSequencer::new())
            .with_context_budget(1_000, 512)
            .with_model_compaction(false);
        let provider = CompactionProvider::new(
            SummaryBehavior::Answer,
            vec![
                tool_round(),
                vec![
                    EngineProviderEvent::TextDelta("done".into()),
                    EngineProviderEvent::Completed,
                ],
            ],
        );
        engine
            .run(
                EngineRunConfig {
                    run_id: format!("r-compact-off-{}", uuid::Uuid::new_v4()),
                    conversation_id: "c-compact".into(),
                    model: "m".into(),
                    system_prompt: None,
                    messages: long_history(10),
                    user_content: "keep going".into(),
                    max_steps: 5,
                },
                &provider,
                &BigOutputTools,
            )
            .await
            .unwrap();
        assert_eq!(provider.summary_count(), 0);
        assert!(!provider
            .last_main_history()
            .iter()
            .any(|m| m.content.starts_with(crate::compaction::SUMMARY_MARKER)));
    }
}
