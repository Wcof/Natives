//! Agent Engine — single execution authority for a Run.
//!
//! Flow: prepare → provider stream → (tool loop) → terminal event.
//! Tools and credentials are injected via seams so the daemon can supply
//! capability-gateway + credential broker without circular deps.

use crate::compaction::{compact_messages as compact_tool_history, repair_dangling_tool_calls};
use crate::doom_loop::DoomLoopDetector;
use crate::event_seq::EventSequencer;
use crate::hooks::{HookDecision, HookEvent, HookRegistry, HookRequest};
use crate::run_state::transition;
use assistant_protocol::v1::run::RunStatus;
use assistant_protocol::v2::{RunEventKind, RunStatusV2};
use futures_util::{Stream, StreamExt};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Soft budget for in-engine history characters before tool-output compaction.
const HISTORY_COMPACT_CHARS: usize = 48_000;
const TOOL_OUTPUT_MAX_CHARS: usize = 4_000;

/// Tool execution seam used by the engine.
#[async_trait::async_trait]
pub trait EngineToolRuntime: Send + Sync {
    async fn list_tool_schemas(&self) -> Vec<ToolSchema>;
    async fn execute_tool(
        &self,
        name: &str,
        input: Value,
        cancel: &AtomicBool,
    ) -> ToolExecutionResult;
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
        cancel: Arc<AtomicBool>,
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
    },
    Completed,
    Error {
        message: String,
        code: String,
        retryable: bool,
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
    },
    #[error("cancelled")]
    Cancelled,
    #[error("doom loop detected")]
    DoomLoop,
    #[error("max steps exceeded")]
    MaxSteps,
}

impl EngineError {
    fn code(&self) -> &str {
        match self {
            Self::Provider { code, .. } => code,
            Self::Cancelled => "cancelled",
            Self::DoomLoop => "doom_loop",
            Self::MaxSteps => "max_steps",
            Self::Message(_) => "provider",
        }
    }

    fn retryable(&self) -> bool {
        matches!(self, Self::Provider { retryable: true, .. })
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
    cancel: Arc<AtomicBool>,
    hooks: HookRegistry,
}

impl AgentEngine {
    pub fn new(events: EventSequencer) -> Self {
        Self {
            events,
            cancel: Arc::new(AtomicBool::new(false)),
            hooks: HookRegistry::new(),
        }
    }

    pub fn with_hooks(mut self, hooks: HookRegistry) -> Self {
        self.hooks = hooks;
        self
    }

    pub fn cancel_flag(&self) -> Arc<AtomicBool> {
        self.cancel.clone()
    }

    pub fn request_cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    /// Execute a full agent loop against the given seams.
    pub async fn run(
        &self,
        config: EngineRunConfig,
        provider: &dyn EngineProvider,
        tools: &dyn EngineToolRuntime,
    ) -> Result<RunStatusV2, EngineError> {
        let run_id = &config.run_id;
        let mut status = RunStatus::Queued;
        status = self.transition_emit(run_id, status, RunStatus::Preparing, RunEventKind::Preparing)?;
        let _ = self
            .hooks
            .dispatch(HookRequest {
                event: HookEvent::SessionStart,
                run_id: run_id.to_string(),
                tool_name: None,
                input: serde_json::json!({ "conversation_id": config.conversation_id }),
            })
            .await;
        let _ = self
            .hooks
            .dispatch(HookRequest {
                event: HookEvent::UserPromptSubmit,
                run_id: run_id.to_string(),
                tool_name: None,
                input: serde_json::json!({ "content": config.user_content }),
            })
            .await;
        status = self.transition_emit(run_id, status, RunStatus::Running, RunEventKind::Started)?;

        let tool_schemas = tools.list_tool_schemas().await;
        let mut messages = if config.messages.is_empty() {
            vec![EngineMessage {
                role: "user".into(),
                content: config.user_content.clone(),
                tool_call_id: None,
                tool_name: None,
                tool_calls: None,
            }]
        } else {
            config.messages.clone()
        };
        let mut doom = DoomLoopDetector::new();
        let mut step = 0u32;

        loop {
            if self.cancel.load(Ordering::SeqCst) {
                self.events.append(
                    run_id,
                    RunEventKind::Interrupted {
                        reason: "cancelled".into(),
                    },
                );
                return Ok(RunStatusV2::Interrupted);
            }
            step += 1;
            if step > config.max_steps {
                self.events.append(
                    run_id,
                    RunEventKind::Failed {
                        error: "max steps exceeded".into(),
                        code: "max_steps".into(),
                    },
                );
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
                        self.events.append(
                            run_id,
                            RunEventKind::Interrupted {
                                reason: "cancelled".into(),
                            },
                        );
                        return Ok(RunStatusV2::Interrupted);
                    }
                    Err(_e) if self.cancel.load(Ordering::SeqCst) => {
                        self.events.append(
                            run_id,
                            RunEventKind::Interrupted {
                                reason: "cancelled".into(),
                            },
                        );
                        return Ok(RunStatusV2::Interrupted);
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
                        sleep_provider_backoff(attempt).await;
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
                        self.events.append(
                            run_id,
                            RunEventKind::Failed {
                                error: e.to_string(),
                                code: e.code().into(),
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
                    if self.cancel.load(Ordering::SeqCst) {
                        self.events.append(
                            run_id,
                            RunEventKind::Interrupted {
                                reason: "cancelled".into(),
                            },
                        );
                        return Ok(RunStatusV2::Interrupted);
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
                        } => {
                            self.events.append(
                                run_id,
                                RunEventKind::UsageUpdated {
                                    input_tokens,
                                    output_tokens,
                                    reasoning_tokens,
                                },
                            );
                        }
                        EngineProviderEvent::Error {
                            message,
                            code,
                            retryable,
                        } => {
                            if !saw_generation_delta && retryable && attempt < MAX_PROVIDER_ATTEMPTS {
                                self.events.append(
                                    run_id,
                                    RunEventKind::GenerationAttemptFailed {
                                        attempt,
                                        code,
                                        retryable,
                                        retrying: true,
                                    },
                            );
                            sleep_provider_backoff(attempt).await;
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
                            self.events.append(
                                run_id,
                                RunEventKind::Failed {
                                    error: message.clone(),
                                    code: code.clone(),
                                },
                            );
                            return Err(EngineError::Provider {
                                message,
                                code,
                                retryable,
                            });
                        }
                        EngineProviderEvent::Completed => {}
                    }
                }

                if self.cancel.load(Ordering::SeqCst) {
                    self.events.append(
                        run_id,
                        RunEventKind::Interrupted {
                            reason: "cancelled".into(),
                        },
                    );
                    return Ok(RunStatusV2::Interrupted);
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
                    self.events.append(
                        run_id,
                        RunEventKind::Failed {
                            error: "provider returned empty response".into(),
                            code: "EMPTY_RESPONSE".into(),
                        },
                    );
                    return Err(EngineError::Provider {
                        message: "provider returned empty response".into(),
                        code: "EMPTY_RESPONSE".into(),
                        retryable: false,
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
                self.events.append(
                    run_id,
                    RunEventKind::Failed {
                        error: "doom loop detected".into(),
                        code: "doom_loop".into(),
                    },
                );
                return Err(EngineError::DoomLoop);
            }

            if tool_acc.is_empty() {
                // No tools — complete.
                let _ = transition(status, RunStatus::Completed);
                let _ = self
                    .hooks
                    .dispatch(HookRequest {
                        event: HookEvent::Stop,
                        run_id: run_id.to_string(),
                        tool_name: None,
                        input: serde_json::json!({ "reason": "stop" }),
                    })
                    .await;
                self.events.append(
                    run_id,
                    RunEventKind::Completed {
                        reason: "stop".into(),
                    },
                );
                return Ok(RunStatusV2::Completed);
            }

            // Execute tools and continue loop.
            let mut assistant_tool_calls = Vec::new();
            let mut tool_results = Vec::new();
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
                    self.events.append(
                        run_id,
                        RunEventKind::Failed {
                            error: "doom loop detected".into(),
                            code: "doom_loop".into(),
                        },
                    );
                    return Err(EngineError::DoomLoop);
                }

                // PreToolUse hooks may deny or modify arguments.
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
                for response in pre {
                    match response.decision {
                        HookDecision::Deny { reason } => {
                            denied = true;
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
                        }
                        HookDecision::Modify { payload } => {
                            input = payload;
                        }
                        _ => {}
                    }
                }
                if denied {
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

                let result = tools
                    .execute_tool(&name, input.clone(), &self.cancel)
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
                        tool_name: Some(name.clone()),
                        input: serde_json::json!({ "input": input, "output": result.output }),
                    })
                    .await;
                self.events.append(
                    run_id,
                    RunEventKind::ToolCallCompleted {
                        id: id.clone(),
                        name: name.clone(),
                        output: result.output.clone(),
                        is_error: result.is_error,
                        duration_ms: result.duration_ms,
                    },
                );

                assistant_tool_calls.push(EngineToolCall {
                    id: id.clone(),
                    name: name.clone(),
                    arguments: args,
                });
                tool_results.push(EngineMessage {
                    role: "tool".into(),
                    content: result.output.to_string(),
                    tool_call_id: Some(id),
                    tool_name: Some(name),
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
            messages = self.maybe_compact_history(run_id, messages).await;
        }
    }

    /// Convert engine history → JSON messages, compact, convert back.
    async fn maybe_compact_history(
        &self,
        run_id: &str,
        messages: Vec<EngineMessage>,
    ) -> Vec<EngineMessage> {
        let before_chars: usize = messages.iter().map(|m| m.content.len()).sum();
        if before_chars < HISTORY_COMPACT_CHARS {
            // Still repair dangling pairs cheaply.
            let values = engine_messages_to_values(&messages);
            let (fixed, repaired) = repair_dangling_tool_calls(&values);
            if repaired == 0 {
                return messages;
            }
            return values_to_engine_messages(&fixed);
        }

        let _ = self
            .hooks
            .dispatch(HookRequest {
                event: HookEvent::PreCompact,
                run_id: run_id.to_string(),
                tool_name: None,
                input: json!({ "before_chars": before_chars }),
            })
            .await;

        let values = engine_messages_to_values(&messages);
        let result = compact_tool_history(&values, TOOL_OUTPUT_MAX_CHARS);
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

        let _ = self
            .hooks
            .dispatch(HookRequest {
                event: HookEvent::PostCompact,
                run_id: run_id.to_string(),
                tool_name: None,
                input: json!({
                    "after_chars": after_chars,
                    "dropped_tool_outputs": result.dropped_tool_outputs,
                    "repaired_dangling": result.repaired_dangling,
                }),
            })
            .await;

        values_to_engine_messages(&result.messages)
    }

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

impl AgentEngine {
    fn transition_emit(
        &self,
        run_id: &str,
        current: RunStatus,
        next: RunStatus,
        event: RunEventKind,
    ) -> Result<RunStatus, EngineError> {
        transition(current, next).map_err(|e| EngineError::Message(e.to_string()))?;
        self.events.append(run_id, event);
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

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
            _cancel: Arc<AtomicBool>,
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
            _cancel: &AtomicBool,
        ) -> ToolExecutionResult {
            ToolExecutionResult {
                output: serde_json::json!({"tool": name, "input": input}),
                is_error: false,
                duration_ms: 1,
            }
        }
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
        assert_eq!(status, RunStatusV2::Completed);
        let events = engine.events.replay_after("r1", 0);
        assert!(events.iter().any(|e| matches!(e.payload, RunEventKind::Started)));
        assert!(events
            .iter()
            .any(|e| matches!(e.payload, RunEventKind::TextDelta { .. })));
        assert!(events
            .iter()
            .any(|e| matches!(e.payload, RunEventKind::Completed { .. })));
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
                _cancel: Arc<AtomicBool>,
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
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0].content, "first fact");
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
        assert_eq!(status, RunStatusV2::Completed);
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
                _cancel: Arc<AtomicBool>,
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
                cancel: Arc<AtomicBool>,
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
                            if cancel.load(Ordering::SeqCst) {
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
        cancel.store(true, Ordering::SeqCst);

        let status = tokio::time::timeout(std::time::Duration::from_secs(1), handle)
            .await
            .expect("run should stop promptly after cancel")
            .expect("join")
            .expect("run");
        assert_eq!(status, RunStatusV2::Interrupted);
        assert!(
            provider_cancel_seen.load(Ordering::SeqCst),
            "provider stream must observe engine cancel flag"
        );
        let current = events.replay_after(&run_id, 0);
        assert!(current
            .iter()
            .any(|e| matches!(e.payload, RunEventKind::Interrupted { .. })));
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
                _cancel: Arc<AtomicBool>,
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
        assert_eq!(status, RunStatusV2::Completed);
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
            .any(|e| matches!(e.payload, RunEventKind::Completed { .. })));
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
        assert_eq!(status, RunStatusV2::Completed);
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
        assert_eq!(status, RunStatusV2::Completed);
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
}
