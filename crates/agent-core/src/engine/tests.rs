use super::*;
use crate::EventPersistence;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[test]
fn strict_snapshot_decode_rejects_missing_identity_and_unknown_blocks() {
    let missing_id = serde_json::json!([{"role":"assistant","blocks":[]}]);
    assert!(try_agent_messages_from_json(&missing_id).is_err());
    let unknown_block = serde_json::json!([{
        "role": "assistant",
        "message_id": "m-1",
        "blocks": [{"type": "future_block"}]
    }]);
    assert!(try_agent_messages_from_json(&unknown_block).is_err());
    let valid = serde_json::json!([{
        "role": "tool",
        "message_id": "m-2",
        "tool_call_id": "call-1",
        "name": "read_file",
        "tool_result_blocks": [{"type": "json", "value": {"ok": true}}]
    }]);
    let decoded = try_agent_messages_from_json(&valid).expect("valid snapshot");
    assert_eq!(decoded.len(), 1);

    let malformed_arguments = serde_json::json!([{
        "role": "assistant",
        "message_id": "m-3",
        "blocks": [{
            "type": "tool_call",
            "tool_call_id": "call-2",
            "name": "read_file",
            "arguments": "{not-json}"
        }]
    }]);
    let error = try_agent_messages_from_json(&malformed_arguments).unwrap_err();
    assert!(error.contains("arguments are invalid JSON"));
}

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
async fn frozen_tool_plan_is_not_discovered_a_second_time() {
    struct CountingTools(AtomicUsize);
    #[async_trait::async_trait]
    impl EngineToolRuntime for CountingTools {
        async fn list_tool_schemas(&self) -> Vec<ToolSchema> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Vec::new()
        }
        async fn execute_tool(
            &self,
            _name: &str,
            _input: Value,
            _cancel: &CancellationToken,
        ) -> ToolExecutionResult {
            unreachable!()
        }
    }

    let tools = CountingTools(AtomicUsize::new(0));
    AgentEngine::new(EventSequencer::new())
        .run_with_tool_schemas(
            EngineRunConfig {
                run_id: "frozen-tools".into(),
                conversation_id: "conversation".into(),
                model: "model".into(),
                system_prompt: None,
                messages: Vec::new(),
                user_content: "hello".into(),
                max_steps: 1,
            },
            &FakeProvider {
                rounds: Mutex::new(Vec::new()),
            },
            &tools,
            vec![ToolSchema {
                name: "frozen".into(),
                description: "frozen".into(),
                input_schema: serde_json::json!({"type":"object"}),
            }],
        )
        .await
        .unwrap();
    assert_eq!(tools.0.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn session_end_hook_fires_after_success() {
    struct RecordingHook(Arc<Mutex<Vec<HookEvent>>>);

    #[async_trait::async_trait]
    impl crate::hooks::HookHandler for RecordingHook {
        async fn handle(&self, request: HookRequest) -> crate::hooks::HookResponse {
            self.0.lock().unwrap().push(request.event);
            crate::hooks::HookResponse {
                decision: HookDecision::Allow,
            }
        }
    }

    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut hooks = HookRegistry::new();
    hooks.register(HookEvent::SessionEnd, Box::new(RecordingHook(seen.clone())));
    let engine = AgentEngine::new(EventSequencer::new()).with_hooks(hooks);
    let provider = FakeProvider {
        rounds: Mutex::new(Vec::new()),
    };
    engine
        .run(
            EngineRunConfig {
                run_id: format!("run-hook-end-{}", uuid::Uuid::new_v4()),
                conversation_id: format!("conversation-hook-end-{}", uuid::Uuid::new_v4()),
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
async fn typed_hook_inject_reaches_provider_turn_request() {
    struct InjectingHook;

    #[async_trait::async_trait]
    impl crate::hooks::HookHandler for InjectingHook {
        async fn handle(&self, _: HookRequest) -> crate::hooks::HookResponse {
            crate::hooks::HookResponse {
                decision: HookDecision::Inject {
                    messages: vec!["SYSTEM-GUARD-INJECTED".into()],
                },
            }
        }
    }

    struct RecordingTurnProvider(Arc<Mutex<Vec<Vec<crate::AgentMessage>>>>);

    #[async_trait::async_trait]
    impl EngineProvider for RecordingTurnProvider {
        async fn stream(
            &self,
            _model: &str,
            _messages: Vec<EngineMessage>,
            _tools: &[ToolSchema],
            _system_prompt: Option<&str>,
            _cancel: CancellationToken,
        ) -> Result<EngineProviderEventStream, EngineError> {
            unreachable!("typed production path must go through stream_turn")
        }

        async fn stream_turn(
            &self,
            request: ProviderTurnRequest,
            _cancel: CancellationToken,
        ) -> Result<EngineProviderEventStream, EngineError> {
            self.0.lock().unwrap().push(request.messages);
            Ok(Box::pin(futures_util::stream::iter(vec![
                EngineProviderEvent::TextDelta("done".into()),
                EngineProviderEvent::Completed,
            ])))
        }
    }

    let mut hooks = HookRegistry::new();
    hooks.register(HookEvent::SessionStart, Box::new(InjectingHook));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let provider = RecordingTurnProvider(seen.clone());
    let run_id = format!("hook-inject-typed-{}", uuid::Uuid::new_v4());
    AgentEngine::new(EventSequencer::new())
        .with_hooks(hooks)
        .run_with_typed_messages(
            EngineRunConfig {
                run_id: run_id.clone(),
                conversation_id: "conversation-hook-inject".into(),
                model: "model".into(),
                system_prompt: None,
                messages: Vec::new(),
                user_content: "hello".into(),
                max_steps: 1,
            },
            &provider,
            &FakeTools,
            Vec::new(),
            vec![crate::AgentMessage::User(crate::UserMessage {
                message_id: crate::MessageId::new(),
                content: vec![crate::ContentBlock::Text {
                    text: "hello".into(),
                }],
            })],
        )
        .await
        .unwrap();

    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1, "one provider turn expected");
    assert!(
        seen[0].iter().any(|message| matches!(
            message,
            crate::AgentMessage::System(system)
                if system.text == "SYSTEM-GUARD-INJECTED"
        )),
        "hook Inject must reach the typed ProviderTurnRequest: {:?}",
        seen[0]
    );
}

#[tokio::test]
async fn tool_name_does_not_select_batch_execution() {
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
                EngineProviderEvent::CompletedWithReason {
                    reason: ProviderStopReason::ToolUse,
                },
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
    assert!(
        matches!(status, crate::EngineOutcome::Completed { .. }),
        "{status:?}"
    );
    assert_eq!(tools.batch_calls.load(AtomicOrdering::SeqCst), 0);
    assert_eq!(tools.single_task_calls.load(AtomicOrdering::SeqCst), 3);
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
async fn parallel_readonly_tools_emit_source_order_results_after_out_of_order_completion() {
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

    struct ParallelRuntime {
        completion_order: Arc<Mutex<Vec<String>>>,
        current: Arc<AtomicUsize>,
        max_seen: Arc<AtomicUsize>,
    }
    #[async_trait::async_trait]
    impl EngineToolRuntime for ParallelRuntime {
        async fn list_tool_schemas(&self) -> Vec<ToolSchema> {
            vec![
                ToolSchema {
                    name: "read_a".into(),
                    description: "a".into(),
                    input_schema: serde_json::json!({"type":"object"}),
                },
                ToolSchema {
                    name: "read_b".into(),
                    description: "b".into(),
                    input_schema: serde_json::json!({"type":"object"}),
                },
            ]
        }
        async fn list_tool_capabilities(&self) -> Vec<ToolCapability> {
            vec![
                ToolCapability {
                    name: "read_a".into(),
                    schema: serde_json::json!({"type":"object"}),
                    execution_mode: ToolExecutionMode::ParallelSafe,
                    side_effect: ToolSideEffect::ReadOnly,
                    conflict_key: None,
                },
                ToolCapability {
                    name: "read_b".into(),
                    schema: serde_json::json!({"type":"object"}),
                    execution_mode: ToolExecutionMode::ParallelSafe,
                    side_effect: ToolSideEffect::ReadOnly,
                    conflict_key: None,
                },
            ]
        }
        async fn execute_tool(
            &self,
            name: &str,
            _input: Value,
            _cancel: &CancellationToken,
        ) -> ToolExecutionResult {
            let running = self.current.fetch_add(1, AtomicOrdering::SeqCst) + 1;
            self.max_seen.fetch_max(running, AtomicOrdering::SeqCst);
            if name == "read_a" {
                tokio::time::sleep(std::time::Duration::from_millis(60)).await;
            }
            self.completion_order.lock().unwrap().push(name.to_string());
            self.current.fetch_sub(1, AtomicOrdering::SeqCst);
            ToolExecutionResult {
                output: serde_json::json!({"tool": name}),
                is_error: false,
                duration_ms: 0,
            }
        }
    }

    let runtime = ParallelRuntime {
        completion_order: Arc::new(Mutex::new(Vec::new())),
        current: Arc::new(AtomicUsize::new(0)),
        max_seen: Arc::new(AtomicUsize::new(0)),
    };
    let engine = AgentEngine::new(EventSequencer::new());
    let run_id = format!("r-parallel-{}", uuid::Uuid::new_v4());
    let provider = FakeProvider {
        rounds: Mutex::new(vec![vec![
            EngineProviderEvent::ToolCallDelta {
                index: 0,
                id: Some("a1".into()),
                name: Some("read_a".into()),
                arguments_delta: r#"{}"#.into(),
            },
            EngineProviderEvent::ToolCallDelta {
                index: 1,
                id: Some("b1".into()),
                name: Some("read_b".into()),
                arguments_delta: r#"{}"#.into(),
            },
            EngineProviderEvent::CompletedWithReason {
                reason: ProviderStopReason::ToolUse,
            },
        ]]),
    };
    engine
        .run(
            EngineRunConfig {
                run_id: run_id.clone(),
                conversation_id: "c1".into(),
                model: "m".into(),
                system_prompt: None,
                messages: Vec::new(),
                user_content: "read both".into(),
                max_steps: 3,
            },
            &provider,
            &runtime,
        )
        .await
        .unwrap();
    // Both read-only tools ran concurrently (max > 1) and the slower one
    // (read_a) finished last, but the emitted ToolCallCompleted events stay
    // in source order.
    assert!(
        runtime.max_seen.load(AtomicOrdering::SeqCst) >= 2,
        "parallel-safe read-only tools must overlap, got max_seen={}",
        runtime.max_seen.load(AtomicOrdering::SeqCst)
    );
    let order = runtime.completion_order.lock().unwrap();
    assert_eq!(order.len(), 2);
    assert_eq!(
        order[0], "read_b",
        "read_b must finish before the delayed read_a"
    );
    assert_eq!(order[1], "read_a");
    let events = engine.events.replay_after(&run_id, 0);
    let completed: Vec<String> = events
        .iter()
        .filter_map(|e| match &e.payload {
            RunEventKind::ToolCallCompleted { id, .. } => Some(id.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(completed, vec!["a1".to_string(), "b1".to_string()]);
}

#[tokio::test]
async fn sequential_tool_serializes_the_batch() {
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

    struct MixedRuntime {
        current: Arc<AtomicUsize>,
        max_seen: Arc<AtomicUsize>,
    }
    #[async_trait::async_trait]
    impl EngineToolRuntime for MixedRuntime {
        async fn list_tool_schemas(&self) -> Vec<ToolSchema> {
            vec![
                ToolSchema {
                    name: "read_a".into(),
                    description: "a".into(),
                    input_schema: serde_json::json!({"type":"object"}),
                },
                ToolSchema {
                    name: "write_b".into(),
                    description: "b".into(),
                    input_schema: serde_json::json!({"type":"object"}),
                },
            ]
        }
        async fn list_tool_capabilities(&self) -> Vec<ToolCapability> {
            vec![
                ToolCapability {
                    name: "read_a".into(),
                    schema: serde_json::json!({"type":"object"}),
                    execution_mode: ToolExecutionMode::ParallelSafe,
                    side_effect: ToolSideEffect::ReadOnly,
                    conflict_key: None,
                },
                ToolCapability {
                    name: "write_b".into(),
                    schema: serde_json::json!({"type":"object"}),
                    execution_mode: ToolExecutionMode::Sequential,
                    side_effect: ToolSideEffect::Write,
                    conflict_key: None,
                },
            ]
        }
        async fn execute_tool(
            &self,
            _name: &str,
            _input: Value,
            _cancel: &CancellationToken,
        ) -> ToolExecutionResult {
            let running = self.current.fetch_add(1, AtomicOrdering::SeqCst) + 1;
            self.max_seen.fetch_max(running, AtomicOrdering::SeqCst);
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            self.current.fetch_sub(1, AtomicOrdering::SeqCst);
            ToolExecutionResult {
                output: serde_json::json!({"ok": true}),
                is_error: false,
                duration_ms: 0,
            }
        }
    }

    let runtime = MixedRuntime {
        current: Arc::new(AtomicUsize::new(0)),
        max_seen: Arc::new(AtomicUsize::new(0)),
    };
    let engine = AgentEngine::new(EventSequencer::new());
    let provider = FakeProvider {
        rounds: Mutex::new(vec![vec![
            EngineProviderEvent::ToolCallDelta {
                index: 0,
                id: Some("a1".into()),
                name: Some("read_a".into()),
                arguments_delta: r#"{}"#.into(),
            },
            EngineProviderEvent::ToolCallDelta {
                index: 1,
                id: Some("b1".into()),
                name: Some("write_b".into()),
                arguments_delta: r#"{}"#.into(),
            },
            EngineProviderEvent::CompletedWithReason {
                reason: ProviderStopReason::ToolUse,
            },
        ]]),
    };
    engine
        .run(
            EngineRunConfig {
                run_id: format!("r-serial-{}", uuid::Uuid::new_v4()),
                conversation_id: "c1".into(),
                model: "m".into(),
                system_prompt: None,
                messages: Vec::new(),
                user_content: "mixed".into(),
                max_steps: 3,
            },
            &provider,
            &runtime,
        )
        .await
        .unwrap();
    // A batch containing a Sequential tool must never run tools in
    // parallel — the read-only tool is demoted to serial execution.
    assert_eq!(
        runtime.max_seen.load(AtomicOrdering::SeqCst),
        1,
        "a Sequential tool in the batch must serialize execution"
    );
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
    let run_id = format!("simple-text-{}", uuid::Uuid::new_v4());
    // A1: text deltas are live-only — subscribe before the run so the
    // ephemeral broadcast retains them for the assertion below.
    let mut live_rx = engine.live.subscribe(&run_id);
    let status = engine
        .run(
            EngineRunConfig {
                run_id: run_id.clone(),
                conversation_id: format!("c-{run_id}"),
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
    assert!(
        matches!(status, crate::EngineOutcome::Completed { .. }),
        "{status:?}"
    );
    let events = engine.events.replay_after(&run_id, 0);
    // TextDelta is a live event and must NOT be in the durable store.
    assert!(
        !events
            .iter()
            .any(|e| matches!(e.payload, RunEventKind::TextDelta { .. })),
        "text delta must not be persisted (live lane only)"
    );
    // The live lane delivered the delta.
    let mut live_text = String::new();
    while let Ok(event) = live_rx.try_recv() {
        if let RunEventKind::TextDelta { text } = event.kind {
            live_text.push_str(&text);
        }
    }
    assert!(
        live_text.contains("hello"),
        "text delta must be delivered on the live bus, got {live_text:?}"
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e.payload, RunEventKind::Completed { .. }))
            || matches!(status, crate::EngineOutcome::Completed { .. })
    );
}

#[tokio::test]
async fn length_stop_never_executes_collected_tool_call() {
    let engine = AgentEngine::new(EventSequencer::new());
    let provider = FakeProvider {
        rounds: Mutex::new(vec![
            vec![
                EngineProviderEvent::ToolCallDelta {
                    index: 0,
                    id: Some("truncated-1".into()),
                    name: Some("echo".into()),
                    arguments_delta: r#"{"path":"ok"}"#.into(),
                },
                EngineProviderEvent::CompletedWithReason {
                    reason: ProviderStopReason::Length,
                },
            ],
            vec![
                EngineProviderEvent::TextDelta("recovered".into()),
                EngineProviderEvent::Completed,
            ],
        ]),
    };
    let calls = AtomicUsize::new(0);
    struct CountingRuntime<'a>(&'a AtomicUsize);
    #[async_trait::async_trait]
    impl EngineToolRuntime for CountingRuntime<'_> {
        async fn list_tool_schemas(&self) -> Vec<ToolSchema> {
            vec![ToolSchema {
                name: "echo".into(),
                description: "echo".into(),
                input_schema: json!({"type":"object"}),
            }]
        }
        async fn execute_tool(
            &self,
            _: &str,
            _: Value,
            _: &CancellationToken,
        ) -> ToolExecutionResult {
            self.0.fetch_add(1, Ordering::SeqCst);
            ToolExecutionResult {
                output: json!({"unexpected": true}),
                is_error: false,
                duration_ms: 0,
            }
        }
    }
    let runtime = CountingRuntime(&calls);
    let run_id = format!("length-run-{}", uuid::Uuid::new_v4());
    engine
        .run(
            EngineRunConfig {
                run_id: run_id.clone(),
                conversation_id: "length-conversation".into(),
                model: "m".into(),
                system_prompt: None,
                messages: Vec::new(),
                user_content: "use echo".into(),
                max_steps: 3,
            },
            &provider,
            &runtime,
        )
        .await
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let completed = engine
        .events
        .replay_after(&run_id, 0)
        .into_iter()
        .filter_map(|event| match event.payload {
            RunEventKind::ToolCallCompleted {
                id,
                output,
                is_error: true,
                ..
            } => Some((id, output)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(completed.len(), 1);
    assert_eq!(completed[0].0, "truncated-1");
    assert_eq!(completed[0].1["error_code"], "TRUNCATED_TOOL_CALL");
}

#[tokio::test]
async fn hook_deny_keeps_tool_call_and_emits_one_error_result() {
    struct DenyHook;
    #[async_trait::async_trait]
    impl crate::hooks::HookHandler for DenyHook {
        async fn handle(&self, _: HookRequest) -> crate::hooks::HookResponse {
            crate::hooks::HookResponse {
                decision: HookDecision::Deny {
                    reason: "policy".into(),
                },
            }
        }
    }
    let mut hooks = HookRegistry::new();
    hooks.register(HookEvent::PreToolUse, Box::new(DenyHook));
    let engine = AgentEngine::new(EventSequencer::new()).with_hooks(hooks);
    let provider = FakeProvider {
        rounds: Mutex::new(vec![
            vec![
                EngineProviderEvent::ToolCallDelta {
                    index: 0,
                    id: Some("deny-1".into()),
                    name: Some("echo".into()),
                    arguments_delta: r#"{"path":"ok"}"#.into(),
                },
                EngineProviderEvent::Completed,
            ],
            vec![
                EngineProviderEvent::TextDelta("after deny".into()),
                EngineProviderEvent::Completed,
            ],
        ]),
    };
    let run_id = format!("deny-run-{}", uuid::Uuid::new_v4());
    engine
        .run(
            EngineRunConfig {
                run_id: run_id.clone(),
                conversation_id: "deny-conversation".into(),
                model: "m".into(),
                system_prompt: None,
                messages: Vec::new(),
                user_content: "use echo".into(),
                max_steps: 3,
            },
            &provider,
            &FakeTools,
        )
        .await
        .unwrap();
    let completed = engine
        .events
        .replay_after(&run_id, 0)
        .into_iter()
        .filter_map(|event| match event.payload {
            RunEventKind::ToolCallCompleted {
                id,
                output,
                is_error: true,
                ..
            } => Some((id, output)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(completed.len(), 1);
    assert_eq!(completed[0].0, "deny-1");
    assert_eq!(completed[0].1["error_code"], "HOOK_DENIED");
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
                        images: Vec::new(),
                    },
                    EngineMessage {
                        role: "assistant".into(),
                        content: "ack".into(),
                        tool_call_id: None,
                        tool_name: None,
                        tool_calls: None,
                        images: Vec::new(),
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
async fn critical_turn_event_persistence_failure_stops_provider_call() {
    struct FailingPersistence;
    impl EventPersistence for FailingPersistence {
        fn append(&self, _: &assistant_protocol::v2::RunEventV2) -> Result<(), String> {
            Err("disk unavailable".into())
        }
        fn replay_after(
            &self,
            _: &str,
            _: u64,
        ) -> Result<Vec<assistant_protocol::v2::RunEventV2>, String> {
            Ok(Vec::new())
        }
        fn last_sequence(&self, _: &str) -> Result<u64, String> {
            Ok(0)
        }
    }
    struct CountingProvider(AtomicUsize);
    #[async_trait::async_trait]
    impl EngineProvider for CountingProvider {
        async fn stream(
            &self,
            _: &str,
            _: Vec<EngineMessage>,
            _: &[ToolSchema],
            _: Option<&str>,
            _: CancellationToken,
        ) -> Result<EngineProviderEventStream, EngineError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(Box::pin(futures_util::stream::iter(vec![
                EngineProviderEvent::Completed,
            ])))
        }
    }
    let provider = CountingProvider(AtomicUsize::new(0));
    let result = AgentEngine::new(EventSequencer::with_persistence(Arc::new(
        FailingPersistence,
    )))
    .run(
        EngineRunConfig {
            run_id: "persist-fail".into(),
            conversation_id: "conversation".into(),
            model: "model".into(),
            system_prompt: None,
            messages: Vec::new(),
            user_content: "hello".into(),
            max_steps: 1,
        },
        &provider,
        &FakeTools,
    )
    .await;
    assert!(
        matches!(result, Err(EngineError::Message(message)) if message == "PERSISTENCE_FAILED")
    );
    assert_eq!(provider.0.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn recovery_blocked_when_completion_and_uncertain_both_fail() {
    // T02: when the ToolCallCompleted fact CANNOT be persisted AND the
    // runtime's uncertain recording ALSO fails, the engine must terminate the
    // run with a distinct `recovery_blocked` code — a later resume must never
    // assume a side effect it cannot prove.
    struct FailsOnToolCallCompleted;
    impl EventPersistence for FailsOnToolCallCompleted {
        fn append(&self, event: &assistant_protocol::v2::RunEventV2) -> Result<(), String> {
            if matches!(
                event.payload,
                assistant_protocol::v2::RunEventKind::ToolCallCompleted { .. }
            ) {
                return Err("disk unavailable".into());
            }
            Ok(())
        }
        fn replay_after(
            &self,
            _: &str,
            _: u64,
        ) -> Result<Vec<assistant_protocol::v2::RunEventV2>, String> {
            Ok(Vec::new())
        }
        fn last_sequence(&self, _: &str) -> Result<u64, String> {
            Ok(0)
        }
    }
    struct FailingUncertainTools;
    #[async_trait::async_trait]
    impl EngineToolRuntime for FailingUncertainTools {
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
        async fn mark_tool_call_uncertain(
            &self,
            _call_id: &str,
            _name: &str,
            _turn_id: Option<&str>,
            _input: &Value,
        ) -> Result<(), String> {
            Err("uncertain recording also failed".into())
        }
    }
    let provider = FakeProvider {
        rounds: Mutex::new(vec![vec![
            EngineProviderEvent::ToolCallDelta {
                index: 0,
                id: Some("t1".into()),
                name: Some("echo".into()),
                arguments_delta: r#"{"x":1}"#.into(),
            },
            EngineProviderEvent::Completed,
        ]]),
    };
    let result = AgentEngine::new(EventSequencer::with_persistence(Arc::new(
        FailsOnToolCallCompleted,
    )))
    .run(
        EngineRunConfig {
            run_id: "recovery-blocked".into(),
            conversation_id: "conversation".into(),
            model: "model".into(),
            system_prompt: None,
            messages: Vec::new(),
            user_content: "hello".into(),
            max_steps: 1,
        },
        &provider,
        &FailingUncertainTools,
    )
    .await;
    assert!(
        matches!(result, Err(EngineError::RecoveryBlocked(_))),
        "double persistence failure must surface recovery_blocked, got {result:?}"
    );
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
    assert!(
        matches!(status, crate::EngineOutcome::Completed { .. }),
        "{status:?}"
    );
    let events = engine.events.replay_after("r2", 0);
    assert!(events
        .iter()
        .any(|e| matches!(e.payload, RunEventKind::ToolCallCompleted { .. })));
}

#[tokio::test]
async fn tool_turn_then_text_turn_are_distinct_turns() {
    let engine = AgentEngine::new(EventSequencer::new());
    let run_id = format!("r-distinct-turns-{}", uuid::Uuid::new_v4());
    let provider = FakeProvider {
        rounds: Mutex::new(vec![
            vec![
                EngineProviderEvent::ToolCallDelta {
                    index: 0,
                    id: Some("t1".into()),
                    name: Some("echo".into()),
                    arguments_delta: r#"{"x":1}"#.into(),
                },
                EngineProviderEvent::CompletedWithReason {
                    reason: ProviderStopReason::ToolUse,
                },
            ],
            vec![
                EngineProviderEvent::TextDelta("after tool".into()),
                EngineProviderEvent::Completed,
            ],
        ]),
    };
    engine
        .run(
            EngineRunConfig {
                run_id: run_id.clone(),
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
    let events = engine.events.replay_after(&run_id, 0);
    let turn_ids: Vec<&String> = events
        .iter()
        .filter_map(|event| match &event.payload {
            RunEventKind::TurnStarted { turn_id } => Some(turn_id),
            _ => None,
        })
        .collect();
    assert_eq!(
        turn_ids.len(),
        2,
        "tool turn + following text turn must each create a Turn"
    );
    assert_ne!(
        turn_ids[0], turn_ids[1],
        "the next provider request is a new Turn, not a retry of the first"
    );
    let completed_turns = events
        .iter()
        .filter(|event| matches!(&event.payload, RunEventKind::TurnCompleted { .. }))
        .count();
    assert_eq!(completed_turns, 2);
    // The committed tool result id rides on ToolCallCompleted so the daemon
    // can reload the same identity instead of inventing a new one.
    assert!(events.iter().any(|event| matches!(
        &event.payload,
        RunEventKind::ToolCallCompleted {
            result_message_id: Some(_),
            ..
        }
    )));
}

#[test]
fn custom_snapshot_round_trips_losslessly() {
    let messages = vec![
        crate::AgentMessage::Custom(crate::CustomMessage {
            message_id: crate::MessageId::from("custom-snap-1"),
            kind: "recipe".into(),
            payload: serde_json::json!({"steps": 3, "tag": "chef"}),
        }),
        crate::AgentMessage::ToolResult(crate::ToolResultMessage {
            message_id: crate::MessageId::from("tool-result-snap-1"),
            tool_call_id: crate::ToolCallId::from("call-snap-1"),
            tool_name: "read_file".into(),
            content: vec![crate::ToolResultBlock::Json {
                value: serde_json::json!({"ok": true}),
            }],
            is_error: false,
            code: None,
        }),
    ];
    let values = agent_messages_to_values(&messages);
    let decoded = try_agent_messages_from_json(&Value::Array(values))
        .expect("strict snapshot decode must accept custom + tool results");
    assert_eq!(
        decoded, messages,
        "snapshot Custom kind/payload and ToolResult identity must be lossless"
    );
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
            Ok(Box::pin(futures_util::stream::unfold(
                0,
                |state| async move {
                    match state {
                        0 => Some((EngineProviderEvent::TextDelta("early".into()), 1)),
                        1 => {
                            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                            Some((EngineProviderEvent::Completed, 2))
                        }
                        _ => None,
                    }
                },
            )))
        }
    }

    let engine = AgentEngine::new(EventSequencer::new());
    let run_id = format!("r-stream-{}", uuid::Uuid::new_v4());
    let run_id_bg = run_id.clone();
    // A1: subscribe the live bus before the run (live deltas are ephemeral).
    let live = engine.live.clone();
    let mut live_rx = live.subscribe(&run_id);
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
        // The provider emits "early" then waits 2s before Completed; the live
        // delta must be observable before the run terminates.
        if let Ok(event) = live_rx.try_recv() {
            if matches!(event.kind, RunEventKind::TextDelta { .. }) {
                saw_text_before_done = true;
                break;
            }
        }
        if handle.is_finished() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(
        saw_text_before_done,
        "text delta must be emitted before stream completion"
    );
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
    let run_id = format!("r-provider-cancel-{}", uuid::Uuid::new_v4());
    let run_id_bg = run_id.clone();
    // A1: subscribe the live bus before the run (live deltas are ephemeral).
    let live = engine.live.clone();
    let mut live_rx = live.subscribe(&run_id);
    let events_after = engine.events.clone();
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
        if let Ok(event) = live_rx.try_recv() {
            if matches!(event.kind, RunEventKind::TextDelta { .. }) {
                saw_text = true;
                break;
            }
        }
        if handle.is_finished() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(
        saw_text,
        "test must observe first text delta before cancelling"
    );
    cancel.cancel();

    let status = tokio::time::timeout(std::time::Duration::from_secs(1), handle)
        .await
        .expect("run should stop promptly after cancel")
        .expect("join")
        .expect("run");
    assert!(
        matches!(
            status,
            crate::EngineOutcome::Cancelled | crate::EngineOutcome::Interrupted { .. }
        ),
        "{status:?}"
    );
    assert!(
        provider_cancel_seen.load(Ordering::SeqCst),
        "provider stream must observe engine cancel flag"
    );
    let current = events_after.replay_after(&run_id, 0);
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
    assert!(
        matches!(status, crate::EngineOutcome::Completed { .. }),
        "{status:?}"
    );
    assert_eq!(
        provider.attempts.load(std::sync::atomic::Ordering::SeqCst),
        3
    );
    let events = engine.events.replay_after(&run_id, 0);
    let attempt_events = events
        .iter()
        .filter(|e| matches!(&e.payload, RunEventKind::GenerationAttemptStarted { .. }))
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
                ..
            } if code == "http_503"
        )
    }));
    assert!(
        events
            .iter()
            .any(|e| matches!(e.payload, RunEventKind::Completed { .. }))
            || matches!(status, crate::EngineOutcome::Completed { .. })
    );
    assert!(events.iter().any(|e| {
        matches!(
            e.payload,
            RunEventKind::GenerationAttemptCommitted { attempt: 3 }
        )
    }));
    let turn_starts = events
        .iter()
        .filter(|e| matches!(&e.payload, RunEventKind::TurnStarted { .. }))
        .count();
    assert_eq!(
        turn_starts, 1,
        "provider retries must stay within one Turn, not start new ones"
    );
    let turn_completed = events
        .iter()
        .filter(|e| matches!(&e.payload, RunEventKind::TurnCompleted { .. }))
        .count();
    assert_eq!(turn_completed, 1);
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
    assert!(
        matches!(status, crate::EngineOutcome::Completed { .. }),
        "{status:?}"
    );
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
    assert!(
        matches!(status, crate::EngineOutcome::Completed { .. }),
        "{status:?}"
    );
    let events = engine.events.replay_after(&run_id, 0);
    assert!(events.iter().any(|e| {
        matches!(
            &e.payload,
            RunEventKind::GenerationAttemptFailed {
                attempt: 1,
                code,
                retryable: true,
                retrying: true,
                ..
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
            self.summary_requests.lock().unwrap().push(
                messages
                    .first()
                    .map(|m| m.content.clone())
                    .unwrap_or_default(),
            );
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
            role: if i % 2 == 0 {
                "user".into()
            } else {
                "assistant".into()
            },
            content: format!("turn {i}: {}", "detail ".repeat(40)),
            tool_call_id: None,
            tool_name: None,
            tool_calls: None,
            images: Vec::new(),
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
        EngineProviderEvent::CompletedWithReason {
            reason: ProviderStopReason::ToolUse,
        },
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
    assert!(
        matches!(status, crate::EngineOutcome::Completed { .. }),
        "{status:?}"
    );

    assert_eq!(
        provider.summary_count(),
        1,
        "exactly one summarization round trip"
    );
    assert!(
        provider.summary_tools_empty.load(Ordering::SeqCst),
        "summarization must be sent without tools so it cannot start a tool loop"
    );
    let asked = provider.summary_requests.lock().unwrap()[0].clone();
    assert!(
        asked.contains("turn 0"),
        "transcript must carry the oldest turn"
    );

    // The next provider turn sees the summary instead of the old prefix.
    let after = provider.last_main_history();
    let summary_msgs: Vec<_> = after
        .iter()
        .filter(|m| m.content.starts_with(crate::compaction::SUMMARY_MARKER))
        .collect();
    assert_eq!(summary_msgs.len(), 1, "history: {after:?}");
    assert!(summary_msgs[0].content.contains("## Next steps"));
    assert!(
        !after.iter().any(|m| m.content.contains("turn 0")),
        "prefix must be gone"
    );
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
    assert!(
        compressed[0].2.contains("## Goal"),
        "event must carry the real summary"
    );
    assert!(
        compressed[0].1 < compressed[0].0,
        "compaction must shrink the history"
    );
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
    assert!(
        matches!(status, crate::EngineOutcome::Completed { .. }),
        "{status:?}"
    );
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
    assert!(
        started,
        "test must observe the summarization request before cancelling"
    );
    cancel.cancel();

    let status = tokio::time::timeout(std::time::Duration::from_secs(2), handle)
        .await
        .expect("a cancelled summary must not hold the run open")
        .expect("join")
        .expect("run");
    assert!(
        matches!(status, crate::EngineOutcome::Cancelled),
        "{status:?}"
    );
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
    assert!(
        matches!(status, crate::EngineOutcome::Completed { .. }),
        "{status:?}"
    );
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

// ---- provider backoff -------------------------------------------------

#[test]
fn backoff_uses_the_local_schedule_without_a_provider_hint() {
    assert_eq!(provider_backoff_ms(1, None), 500);
    assert_eq!(provider_backoff_ms(2, None), 1_000);
    assert_eq!(provider_backoff_ms(3, None), 2_000);
}

#[test]
fn backoff_honours_a_longer_provider_hint_and_ignores_a_shorter_one() {
    assert_eq!(provider_backoff_ms(1, Some(7_500)), 7_500);
    // A hint below the local schedule never shortens the wait.
    assert_eq!(provider_backoff_ms(3, Some(100)), 2_000);
}

#[test]
fn backoff_clamps_an_absurd_provider_hint() {
    assert_eq!(
        provider_backoff_ms(1, Some(6 * 60 * 60 * 1_000)),
        MAX_PROVIDER_BACKOFF_MS
    );
}

/// Fails the first `fail_times` attempts with a rate limit that names a
/// `retry_after_ms`, then answers.
struct RateLimitedProvider {
    attempts: std::sync::atomic::AtomicUsize,
    fail_times: usize,
    retry_after_ms: Option<u64>,
    /// `true` reports the rate limit as a stream event instead of a
    /// connect-time error, exercising the second retry site.
    as_stream_event: bool,
}

#[async_trait::async_trait]
impl EngineProvider for RateLimitedProvider {
    async fn stream(
        &self,
        _model: &str,
        _messages: Vec<EngineMessage>,
        _tools: &[ToolSchema],
        _system_prompt: Option<&str>,
        _cancel: CancellationToken,
    ) -> Result<EngineProviderEventStream, EngineError> {
        let n = self
            .attempts
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if n < self.fail_times {
            if self.as_stream_event {
                return Ok(Box::pin(futures_util::stream::iter(vec![
                    EngineProviderEvent::Error {
                        message: "slow down".into(),
                        code: "http_429".into(),
                        retryable: true,
                        category: "RateLimit".into(),
                        retry_after_ms: self.retry_after_ms,
                    },
                ])));
            }
            return Err(EngineError::Provider {
                message: "slow down".into(),
                code: "http_429".into(),
                retryable: true,
                category: "RateLimit".into(),
                retry_after_ms: self.retry_after_ms,
            });
        }
        Ok(Box::pin(futures_util::stream::iter(vec![
            EngineProviderEvent::TextDelta("ok".into()),
            EngineProviderEvent::Completed,
        ])))
    }
}

fn rate_limit_config(run_id: &str) -> EngineRunConfig {
    EngineRunConfig {
        run_id: run_id.to_string(),
        conversation_id: "c-429".into(),
        model: "m".into(),
        system_prompt: None,
        messages: Vec::new(),
        user_content: "hi".into(),
        max_steps: 5,
    }
}

fn announced_backoffs(engine: &AgentEngine, run_id: &str) -> Vec<u64> {
    engine
        .events
        .replay_after(run_id, 0)
        .iter()
        .filter_map(|e| match &e.payload {
            RunEventKind::GenerationAttemptFailed {
                retrying: true,
                retry_in_ms,
                ..
            } => *retry_in_ms,
            _ => None,
        })
        .collect()
}

/// The regression: a 429 used to skip the backoff entirely and retry inside
/// a millisecond, three times, ignoring the delay the provider asked for.
#[tokio::test]
async fn rate_limited_connect_error_waits_for_the_provider_hint() {
    let engine = AgentEngine::new(EventSequencer::new());
    let run_id = format!("r-429-connect-{}", uuid::Uuid::new_v4());
    let provider = RateLimitedProvider {
        attempts: std::sync::atomic::AtomicUsize::new(0),
        fail_times: 1,
        // Above the 500ms local schedule for attempt 1, so only the hint
        // can explain the wait. Kept small to keep the test fast; the
        // clamping arithmetic is covered by the unit tests above.
        retry_after_ms: Some(700),
        as_stream_event: false,
    };
    let start = std::time::Instant::now();
    let status = engine
        .run(rate_limit_config(&run_id), &provider, &FakeTools)
        .await
        .unwrap();
    let waited = start.elapsed();
    assert!(
        matches!(status, crate::EngineOutcome::Completed { .. }),
        "{status:?}"
    );
    assert!(
        waited >= std::time::Duration::from_millis(700),
        "429 must wait out Retry-After, waited {waited:?}"
    );
    assert_eq!(announced_backoffs(&engine, &run_id), vec![700]);
}

#[tokio::test]
async fn rate_limited_stream_event_waits_for_the_provider_hint() {
    let engine = AgentEngine::new(EventSequencer::new());
    let run_id = format!("r-429-stream-{}", uuid::Uuid::new_v4());
    let provider = RateLimitedProvider {
        attempts: std::sync::atomic::AtomicUsize::new(0),
        fail_times: 1,
        retry_after_ms: Some(900),
        as_stream_event: true,
    };
    let start = std::time::Instant::now();
    let status = engine
        .run(rate_limit_config(&run_id), &provider, &FakeTools)
        .await
        .unwrap();
    let waited = start.elapsed();
    assert!(
        matches!(status, crate::EngineOutcome::Completed { .. }),
        "{status:?}"
    );
    assert!(
        waited >= std::time::Duration::from_millis(900),
        "a stream-side 429 must wait too, waited {waited:?}"
    );
    assert_eq!(announced_backoffs(&engine, &run_id), vec![900]);
}

#[tokio::test]
async fn backoff_is_cancellable() {
    let engine = Arc::new(AgentEngine::new(EventSequencer::new()));
    let run_id = format!("r-429-cancel-{}", uuid::Uuid::new_v4());
    let provider = RateLimitedProvider {
        attempts: std::sync::atomic::AtomicUsize::new(0),
        fail_times: 2,
        retry_after_ms: Some(MAX_PROVIDER_BACKOFF_MS),
        as_stream_event: false,
    };
    let cancel = engine.cancel_token();
    let runner = {
        let engine = engine.clone();
        let run_id = run_id.clone();
        tokio::spawn(async move {
            engine
                .run(rate_limit_config(&run_id), &provider, &FakeTools)
                .await
        })
    };
    // Long enough to be inside the 60s wait, nowhere near finishing it.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    cancel.cancel();
    let status = tokio::time::timeout(std::time::Duration::from_secs(5), runner)
        .await
        .expect("a cancelled backoff must not hold the run open")
        .expect("join")
        .expect("run");
    assert!(
        matches!(status, crate::EngineOutcome::Cancelled),
        "{status:?}"
    );
}

// ---- doom loop diagnostics -------------------------------------------

#[tokio::test]
async fn doom_loop_error_names_the_repeating_pattern() {
    struct RepeatingToolProvider;
    #[async_trait::async_trait]
    impl EngineProvider for RepeatingToolProvider {
        async fn stream(
            &self,
            _model: &str,
            _messages: Vec<EngineMessage>,
            _tools: &[ToolSchema],
            _system_prompt: Option<&str>,
            _cancel: CancellationToken,
        ) -> Result<EngineProviderEventStream, EngineError> {
            Ok(Box::pin(futures_util::stream::iter(vec![
                EngineProviderEvent::ToolCallDelta {
                    index: 0,
                    id: Some(uuid::Uuid::new_v4().to_string()),
                    name: Some("read_file".into()),
                    arguments_delta: r#"{"path":"a.txt"}"#.into(),
                },
                EngineProviderEvent::Completed,
            ])))
        }
    }

    let engine = AgentEngine::new(EventSequencer::new());
    let error = engine
        .run(
            EngineRunConfig {
                run_id: format!("r-doom-{}", uuid::Uuid::new_v4()),
                conversation_id: "c-doom".into(),
                model: "m".into(),
                system_prompt: None,
                messages: Vec::new(),
                user_content: "go".into(),
                max_steps: 20,
            },
            &RepeatingToolProvider,
            &FakeTools,
        )
        .await
        .expect_err("a repeating tool call must abort the run");
    assert_eq!(error.code(), "doom_loop");
    let EngineError::DoomLoop(reason) = &error else {
        panic!("expected a doom-loop error, got {error:?}");
    };
    assert_eq!(reason.signal.as_str(), "tool");
    let rendered = error.to_string();
    assert!(rendered.contains("read_file"), "{rendered}");
    assert!(rendered.contains("cycle"), "{rendered}");
}

#[test]
fn long_tool_args_sharing_a_prefix_stay_distinguishable() {
    let shared = "x".repeat(200);
    let a = format!(r#"{{"path":"{shared}","new":"alpha"}}"#);
    let b = format!(r#"{{"path":"{shared}","new":"beta"}}"#);
    assert_ne!(
        tool_args_fingerprint(&a),
        tool_args_fingerprint(&b),
        "a bare 80-char prefix would call these the same edit"
    );
    // Short arguments keep their readable, hash-free form.
    assert_eq!(tool_args_fingerprint(r#"{"path":"a"}"#), r#"{"path":"a"}"#);
}

// ---- multimodal history ----------------------------------------------

#[test]
fn images_survive_the_compaction_value_round_trip() {
    let original = vec![EngineMessage {
        role: "user".into(),
        content: "what is this".into(),
        images: vec![EngineImage {
            url: "data:image/png;base64,AAAB".into(),
            media_type: Some("image/png".into()),
            detail: Some("high".into()),
        }],
        ..Default::default()
    }];
    let values = engine_messages_to_values(&original);
    assert_eq!(values[0]["images"][0]["url"], "data:image/png;base64,AAAB");
    assert_eq!(values_to_engine_messages(&values), original);
}

#[test]
fn text_only_messages_do_not_grow_an_images_key() {
    let values = engine_messages_to_values(&[EngineMessage::text("user", "hi")]);
    assert!(values[0].get("images").is_none(), "{:?}", values[0]);
}

// ---- AgentMessage ↔ EngineMessage round-trip ---------------------------
//
// These tests verify that the conversion between the typed AgentMessage
// (message.rs) and the legacy EngineMessage preserves every content block
// type. Known limitations are documented per test:
//
// - Thinking blocks: signature field is lost (EngineMessage has no
//   dedicated Thinking slot; text survives in the content field).
// - Custom messages: payload is flattened to a string (EngineMessage has
//   no structured payload field).
// - Multiple text blocks within one AgentMessage are concatenated during
//   the round-trip (EngineMessage stores a single content string).

#[test]
fn thinking_block_survives_agent_message_round_trip() {
    let original = vec![crate::AgentMessage::Assistant(crate::AssistantMessage {
        message_id: crate::MessageId::from("thinking-1"),
        content: vec![
            crate::ContentBlock::Thinking {
                text: "step 1: analyze the problem".into(),
                signature: Some("sig_abc123".into()),
            },
            crate::ContentBlock::Text {
                text: "The answer is 42".into(),
            },
        ],
        stop_reason: Some(crate::StopReason::Stop),
    })];

    let engine_msgs = agent_messages_to_engine_messages(&original);
    assert_eq!(engine_msgs.len(), 1);
    // Text from both Thinking and Text blocks is concatenated
    assert!(engine_msgs[0]
        .content
        .contains("step 1: analyze the problem"));
    assert!(engine_msgs[0].content.contains("The answer is 42"));

    // Round-trip back: signature is lost (EngineMessage has no slot)
    let round_tripped = engine_messages_to_agent_messages(&engine_msgs);
    assert_eq!(round_tripped.len(), 1);
    let assistant = match &round_tripped[0] {
        crate::AgentMessage::Assistant(msg) => msg,
        other => panic!("expected Assistant, got {other:?}"),
    };
    // Text content survives
    assert!(assistant.content.iter().any(|b| matches!(b,
        crate::ContentBlock::Text { text } if text.contains("step 1: analyze the problem")
    )));
    assert!(assistant.content.iter().any(|b| matches!(b,
        crate::ContentBlock::Text { text } if text.contains("The answer is 42")
    )));
    // Thinking signature is lost (documented limitation)
    assert!(!assistant.content.iter().any(|b| matches!(
        b,
        crate::ContentBlock::Thinking {
            signature: Some(_),
            ..
        }
    )));
}

#[test]
fn image_block_survives_agent_message_round_trip() {
    let original = vec![crate::AgentMessage::User(crate::UserMessage {
        message_id: crate::MessageId::from("img-1"),
        content: vec![
            crate::ContentBlock::Text {
                text: "what is this image".into(),
            },
            crate::ContentBlock::Image {
                source: crate::ImageSource {
                    url: "data:image/png;base64,AAAB".into(),
                    media_type: Some("image/png".into()),
                    detail: Some("high".into()),
                },
            },
        ],
    })];

    let engine_msgs = agent_messages_to_engine_messages(&original);
    assert_eq!(engine_msgs.len(), 1);
    assert_eq!(engine_msgs[0].images.len(), 1);
    assert_eq!(engine_msgs[0].images[0].url, "data:image/png;base64,AAAB");
    assert_eq!(
        engine_msgs[0].images[0].media_type,
        Some("image/png".into())
    );

    // Round-trip back
    let round_tripped = engine_messages_to_agent_messages(&engine_msgs);
    assert_eq!(round_tripped.len(), 1);
    let user = match &round_tripped[0] {
        crate::AgentMessage::User(msg) => msg,
        other => panic!("expected User, got {other:?}"),
    };
    let image_blocks: Vec<_> = user
        .content
        .iter()
        .filter_map(|b| {
            if let crate::ContentBlock::Image { source } = b {
                Some(source)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(image_blocks.len(), 1, "image block must survive round-trip");
    assert_eq!(image_blocks[0].url, "data:image/png;base64,AAAB");
    assert_eq!(image_blocks[0].media_type, Some("image/png".into()));
    assert_eq!(image_blocks[0].detail, Some("high".into()));
}

#[test]
fn custom_message_survives_agent_message_round_trip() {
    let original = vec![crate::AgentMessage::Custom(crate::CustomMessage {
        message_id: crate::MessageId::from("custom-1"),
        kind: "recipe".into(),
        payload: serde_json::json!({"steps": 3, "tag": "chef"}),
    })];

    let engine_msgs = agent_messages_to_engine_messages(&original);
    assert_eq!(engine_msgs.len(), 1);
    // Custom message kind becomes role, payload becomes content string
    assert_eq!(engine_msgs[0].role, "recipe");
    assert!(engine_msgs[0].content.contains("chef"));

    // Round-trip back: payload is a string, not structured JSON
    // (documented limitation of the EngineMessage format)
    let round_tripped = engine_messages_to_agent_messages(&engine_msgs);
    assert_eq!(round_tripped.len(), 1);
    let custom = match &round_tripped[0] {
        crate::AgentMessage::Custom(msg) => msg,
        other => panic!("expected Custom, got {other:?}"),
    };
    assert_eq!(custom.kind, "recipe");
    // The structured payload is flattened to a string in the round-trip
    // (EngineMessage has no structured payload field)
    assert!(custom.payload.to_string().contains("chef"));
}

#[test]
fn tool_call_survives_agent_message_round_trip() {
    let original = vec![crate::AgentMessage::Assistant(crate::AssistantMessage {
        message_id: crate::MessageId::from("tool-call-1"),
        content: vec![
            crate::ContentBlock::Text {
                text: "Let me check the file".into(),
            },
            crate::ContentBlock::ToolCall(crate::ToolCall {
                tool_call_id: crate::ToolCallId::from("call_read"),
                name: "read_file".into(),
                arguments_json: r#"{"path":"Cargo.toml"}"#.into(),
            }),
        ],
        stop_reason: Some(crate::StopReason::ToolUse),
    })];

    let engine_msgs = agent_messages_to_engine_messages(&original);
    assert_eq!(engine_msgs.len(), 1);
    assert!(engine_msgs[0].tool_calls.is_some());
    let calls = engine_msgs[0].tool_calls.as_ref().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].name, "read_file");
    assert_eq!(calls[0].id, "call_read");

    // Round-trip back
    let round_tripped = engine_messages_to_agent_messages(&engine_msgs);
    assert_eq!(round_tripped.len(), 1);
    let assistant = match &round_tripped[0] {
        crate::AgentMessage::Assistant(msg) => msg,
        other => panic!("expected Assistant, got {other:?}"),
    };
    let tool_calls: Vec<_> = assistant
        .content
        .iter()
        .filter_map(|b| {
            if let crate::ContentBlock::ToolCall(call) = b {
                Some(call)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(tool_calls.len(), 1, "tool call must survive round-trip");
    assert_eq!(tool_calls[0].name, "read_file");
    assert_eq!(tool_calls[0].tool_call_id.to_string(), "call_read");
}

#[test]
fn tool_result_survives_agent_message_round_trip() {
    let original = vec![crate::AgentMessage::ToolResult(crate::ToolResultMessage {
        message_id: crate::MessageId::from("tr-1"),
        tool_call_id: crate::ToolCallId::from("call_read"),
        tool_name: "read_file".into(),
        content: vec![crate::ToolResultBlock::Json {
            value: serde_json::json!({"content": "fn main() {}", "path": "src/main.rs"}),
        }],
        is_error: false,
        code: None,
    })];

    let engine_msgs = agent_messages_to_engine_messages(&original);
    assert_eq!(engine_msgs.len(), 1);
    assert_eq!(engine_msgs[0].role, "tool");
    assert_eq!(engine_msgs[0].tool_call_id, Some("call_read".into()));
    assert_eq!(engine_msgs[0].tool_name, Some("read_file".into()));

    // Round-trip back
    let round_tripped = engine_messages_to_agent_messages(&engine_msgs);
    assert_eq!(round_tripped.len(), 1);
    let result = match &round_tripped[0] {
        crate::AgentMessage::ToolResult(msg) => msg,
        other => panic!("expected ToolResult, got {other:?}"),
    };
    assert_eq!(result.tool_call_id.to_string(), "call_read");
    assert_eq!(result.tool_name, "read_file");
    assert!(result.content.iter().any(|b| matches!(b,
        crate::ToolResultBlock::Json { value } if value.to_string().contains("fn main()")
    )));
}

#[test]
fn round_trip_preserves_message_order_and_roles() {
    let original = vec![
        crate::AgentMessage::System(crate::SystemMessage {
            message_id: crate::MessageId::from("sys-1"),
            text: "You are a helpful assistant".into(),
        }),
        crate::AgentMessage::User(crate::UserMessage {
            message_id: crate::MessageId::from("user-1"),
            content: vec![crate::ContentBlock::Text {
                text: "Hello".into(),
            }],
        }),
        crate::AgentMessage::Assistant(crate::AssistantMessage {
            message_id: crate::MessageId::from("asst-1"),
            content: vec![crate::ContentBlock::Text {
                text: "Hi! How can I help?".into(),
            }],
            stop_reason: None,
        }),
    ];

    let engine_msgs = agent_messages_to_engine_messages(&original);
    assert_eq!(engine_msgs.len(), 3);
    assert_eq!(engine_msgs[0].role, "system");
    assert_eq!(engine_msgs[1].role, "user");
    assert_eq!(engine_msgs[2].role, "assistant");

    let round_tripped = engine_messages_to_agent_messages(&engine_msgs);
    assert_eq!(round_tripped.len(), 3);
    assert!(matches!(round_tripped[0], crate::AgentMessage::System(_)));
    assert!(matches!(round_tripped[1], crate::AgentMessage::User(_)));
    assert!(matches!(
        round_tripped[2],
        crate::AgentMessage::Assistant(_)
    ));
}

#[tokio::test]
async fn engine_hands_user_images_to_the_provider() {
    let provider = CaptureProvider::default();
    let engine = AgentEngine::new(EventSequencer::new());
    engine
        .run(
            EngineRunConfig {
                run_id: format!("r-image-{}", uuid::Uuid::new_v4()),
                conversation_id: "c-image".into(),
                model: "m".into(),
                system_prompt: None,
                messages: vec![EngineMessage {
                    role: "user".into(),
                    content: "what is this".into(),
                    images: vec![EngineImage::new("data:image/png;base64,AAAB")],
                    ..Default::default()
                }],
                user_content: "what is this".into(),
                max_steps: 3,
            },
            &provider,
            &FakeTools,
        )
        .await
        .unwrap();
    let seen = provider.seen.lock().unwrap();
    let images: Vec<&EngineImage> = seen.iter().flat_map(|m| m.images.iter()).collect();
    assert_eq!(images.len(), 1, "the image must reach the provider seam");
    assert_eq!(images[0].url, "data:image/png;base64,AAAB");
}

#[derive(Default)]
struct CaptureProvider {
    seen: Mutex<Vec<EngineMessage>>,
}

#[async_trait::async_trait]
impl EngineProvider for CaptureProvider {
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
            EngineProviderEvent::TextDelta("a picture".into()),
            EngineProviderEvent::Completed,
        ])))
    }
}

#[tokio::test]
async fn follow_up_closes_previous_turn_before_next_provider_call() {
    struct FollowUpReceiver {
        offered: AtomicBool,
    }

    #[async_trait::async_trait]
    impl crate::EngineInputReceiver for FollowUpReceiver {
        async fn drain(
            &self,
            kind: crate::PendingInputKind,
            _mode: crate::DrainMode,
            _point: crate::InputSafePoint,
        ) -> Result<Vec<crate::PendingInput>, String> {
            if kind == crate::PendingInputKind::FollowUp
                && !self.offered.swap(true, Ordering::SeqCst)
            {
                Ok(vec![crate::PendingInput {
                    id: "follow-up-1".into(),
                    kind,
                    content: "continue with the next step".into(),
                    lease_token: None,
                }])
            } else {
                Ok(Vec::new())
            }
        }

        async fn ack(
            &self,
            _input: &crate::PendingInput,
            _turn_id: Option<&str>,
        ) -> Result<(), String> {
            Ok(())
        }
    }

    let run_id = format!("follow-up-turn-boundary-{}", uuid::Uuid::new_v4());
    let engine =
        AgentEngine::new(EventSequencer::new()).with_input_receiver(Arc::new(FollowUpReceiver {
            offered: AtomicBool::new(false),
        }));
    let provider = FakeProvider {
        rounds: Mutex::new(vec![
            vec![
                EngineProviderEvent::TextDelta("first answer".into()),
                EngineProviderEvent::Completed,
            ],
            vec![
                EngineProviderEvent::TextDelta("second answer".into()),
                EngineProviderEvent::Completed,
            ],
        ]),
    };

    engine
        .run(
            EngineRunConfig {
                run_id: run_id.clone(),
                conversation_id: "follow-up-conversation".into(),
                model: "model".into(),
                system_prompt: None,
                messages: Vec::new(),
                user_content: "start".into(),
                max_steps: 3,
            },
            &provider,
            &FakeTools,
        )
        .await
        .unwrap();

    let events = engine.events.replay_after(&run_id, 0);
    let first_turn_completed = events
        .iter()
        .position(|event| matches!(event.payload, RunEventKind::TurnCompleted { .. }))
        .expect("first turn must be committed");
    let second_turn_started = events
        .iter()
        .skip(first_turn_completed + 1)
        .position(|event| matches!(event.payload, RunEventKind::TurnStarted { .. }))
        .map(|offset| first_turn_completed + 1 + offset)
        .expect("follow-up must begin a new turn");
    assert!(first_turn_completed < second_turn_started);
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event.payload, RunEventKind::MessageCompleted { .. }))
            .count(),
        2
    );
}

/// TASK-010 (C02): steering inputs are drained at safe points and injected
/// as COMPLETE User messages in FIFO order — never a half Assistant
/// message, never reordered, and each input is acked.
#[tokio::test]
async fn steering_injects_complete_user_messages_in_fifo_order() {
    struct SteeringReceiver {
        offered: AtomicUsize,
        acked: Mutex<Vec<String>>,
    }

    #[async_trait::async_trait]
    impl crate::EngineInputReceiver for SteeringReceiver {
        async fn drain(
            &self,
            kind: crate::PendingInputKind,
            _mode: crate::DrainMode,
            _point: crate::InputSafePoint,
        ) -> Result<Vec<crate::PendingInput>, String> {
            if kind == crate::PendingInputKind::Steering {
                let n = self.offered.fetch_add(1, Ordering::SeqCst);
                if n < 2 {
                    return Ok(vec![crate::PendingInput {
                        id: format!("steer-{n}"),
                        kind,
                        content: format!("steer-{n}"),
                        lease_token: None,
                    }]);
                }
            }
            Ok(Vec::new())
        }

        async fn ack(
            &self,
            input: &crate::PendingInput,
            _turn_id: Option<&str>,
        ) -> Result<(), String> {
            self.acked.lock().unwrap().push(input.id.clone());
            Ok(())
        }
    }

    let receiver = Arc::new(SteeringReceiver {
        offered: AtomicUsize::new(0),
        acked: Mutex::new(Vec::new()),
    });
    let engine = AgentEngine::new(EventSequencer::new()).with_input_receiver(receiver.clone());
    let mut messages: Vec<crate::AgentMessage> = Vec::new();
    // Two steering inputs offered on successive drains — drain them FIFO.
    engine
        .drain_inputs(
            crate::PendingInputKind::Steering,
            crate::DrainMode::All,
            crate::InputSafePoint::AfterToolBatch,
            &mut messages,
            None,
        )
        .await
        .unwrap();
    engine
        .drain_inputs(
            crate::PendingInputKind::Steering,
            crate::DrainMode::All,
            crate::InputSafePoint::AfterToolBatch,
            &mut messages,
            None,
        )
        .await
        .unwrap();
    assert_eq!(messages.len(), 2, "both steering inputs are injected");
    let texts: Vec<String> = messages
        .iter()
        .filter_map(|m| match m {
            crate::AgentMessage::User(u) => u.content.iter().find_map(|b| match b {
                crate::ContentBlock::Text { text } => Some(text.clone()),
                _ => None,
            }),
            _ => None,
        })
        .collect();
    assert_eq!(
        texts,
        vec![
            "[steering]\nsteer-0".to_string(),
            "[steering]\nsteer-1".to_string()
        ],
        "steering inputs are complete User messages in FIFO order"
    );
    let acked = receiver.acked.lock().unwrap();
    assert_eq!(
        acked.as_slice(),
        &["steer-0".to_string(), "steer-1".to_string()],
        "each steering input is acked after injection"
    );
}

// ---- Characterization tests for public API --------------------------------
// These verify that the engine.rs module split preserved the public API types,
// traits, and functions. They are compile-time checks that fail if a public
// item was removed or renamed during the refactor.

#[test]
fn engine_public_api_types_are_accessible() {
    // EngineMessage and related types
    let _msg = EngineMessage::text("user", "hello");
    let _img = EngineImage::new("data:image/png;base64,test");
    let _call = EngineToolCall {
        id: "call-1".into(),
        name: "read_file".into(),
        arguments: r#"{}"#.into(),
    };
    let _event = EngineProviderEvent::TextDelta("delta".into());

    // EngineRunConfig
    let _config = EngineRunConfig {
        run_id: "test".into(),
        conversation_id: "conv".into(),
        model: "gpt-4o".into(),
        system_prompt: None,
        messages: vec![EngineMessage::text("user", "hello")],
        user_content: "hello".into(),
        max_steps: 3,
    };

    // EngineProviderContext
    let _ctx = EngineProviderContext {
        run_id: "test".into(),
        attempt: 0,
    };

    // AgentEngine
    let _engine = AgentEngine::new(EventSequencer::new());

    // EngineError
    let _err = EngineError::Message("test".into());
    let _code = _err.code();
    let _retryable = _err.retryable();
    let _rate_limited = _err.is_rate_limited();
    let _overflow = _err.is_context_overflow();
}

#[test]
fn engine_public_traits_are_accessible() {
    // EngineToolRuntime trait
    struct TestTools;
    #[async_trait::async_trait]
    impl EngineToolRuntime for TestTools {
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
            _input: serde_json::Value,
            _cancel: &CancellationToken,
        ) -> ToolExecutionResult {
            ToolExecutionResult {
                output: serde_json::json!({}),
                is_error: false,
                duration_ms: 0,
            }
        }
    }

    // EngineProvider trait
    struct TestProvider;
    #[async_trait::async_trait]
    impl EngineProvider for TestProvider {
        async fn stream(
            &self,
            _model: &str,
            _messages: Vec<EngineMessage>,
            _tools: &[ToolSchema],
            _system_prompt: Option<&str>,
            _cancel: CancellationToken,
        ) -> Result<EngineProviderEventStream, EngineError> {
            Ok(Box::pin(futures_util::stream::iter(vec![
                EngineProviderEvent::TextDelta("ok".into()),
                EngineProviderEvent::Completed,
            ])))
        }
    }

    // Construct the trait impls so dead-code does not fire (their purpose is
    // to prove the public traits are implementable).
    let _tools = TestTools;
    let _provider = TestProvider;

    // ToolProgressSink trait
    struct TestSink;
    #[async_trait::async_trait]
    impl ToolProgressSink for TestSink {
        async fn publish(&self, _update: ToolProgressUpdate) {}
    }

    let _sink = NoopToolProgressSink;
    let _sink_ref: &dyn ToolProgressSink = &_sink;
    let _sink_arc: Arc<dyn ToolProgressSink> = Arc::new(TestSink);
}

#[test]
fn engine_conversion_functions_are_accessible() {
    // engine_messages_to_agent_messages
    let engine_msgs = vec![EngineMessage::text("user", "hello")];
    let agent_msgs = engine_messages_to_agent_messages(&engine_msgs);
    assert_eq!(agent_msgs.len(), 1);

    // agent_messages_to_engine_messages
    let back = agent_messages_to_engine_messages(&agent_msgs);
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].content, "hello");

    // agent_messages_from_json / try_agent_messages_from_json
    // The typed snapshot format requires specific fields.
    let json = serde_json::json!([{
        "role": "user",
        "message_id": "m-1",
        "content": [{"type": "text", "text": "hello"}]
    }]);
    let from_json = agent_messages_from_json(&json);
    assert_eq!(from_json.len(), 1);
    let try_from = try_agent_messages_from_json(&json);
    assert!(
        try_from.is_ok(),
        "try_agent_messages_from_json failed: {:?}",
        try_from.err()
    );

    // ToolCapability and ToolExecutionMode
    let _cap = ToolCapability {
        name: "read_file".into(),
        schema: serde_json::json!({"type":"object"}),
        execution_mode: ToolExecutionMode::Sequential,
        side_effect: ToolSideEffect::Write,
        conflict_key: None,
    };
}

#[test]
fn engine_agentengine_builder_chain_compiles() {
    let engine = AgentEngine::new(EventSequencer::new())
        .with_model_compaction(false)
        .with_cancel_token(CancellationToken::new())
        .with_context_budget(50_000, 4_000)
        .with_provider_context_window(Some(128_000));

    let _token = engine.cancel_token();
    let _cancelled = engine.is_cancelled();
    // Note: request_cancel would cancel the token, so we don't call it here.
}

#[test]
fn engine_public_types_have_expected_sizes() {
    // Smoke-check that the types compile and have reasonable sizes.
    use std::mem::size_of;

    // Core message types
    assert!(size_of::<EngineMessage>() > 0);
    assert!(size_of::<EngineImage>() > 0);
    assert!(size_of::<EngineToolCall>() > 0);

    // Engine configuration
    assert!(size_of::<EngineRunConfig>() > 0);

    // Provider types
    assert!(size_of::<EngineProviderContext>() > 0);
    assert!(size_of::<EngineError>() > 0);

    // Tool types
    assert!(size_of::<ToolSchema>() > 0);
    assert!(size_of::<ToolCapability>() > 0);
    assert!(size_of::<ToolExecutionResult>() > 0);
    assert!(size_of::<ToolProgressUpdate>() > 0);
}

#[tokio::test]
async fn post_tool_use_deny_fails_run_loudly() {
    // T03: a PostToolUse hook that denies after the tool already executed must
    // fail the run with a `hook_refused` error — never be silently ignored.
    struct DenyPostHook;
    #[async_trait::async_trait]
    impl crate::hooks::HookHandler for DenyPostHook {
        async fn handle(&self, _: HookRequest) -> crate::hooks::HookResponse {
            crate::hooks::HookResponse {
                decision: HookDecision::Deny {
                    reason: "too late to block".into(),
                },
            }
        }
    }
    let mut hooks = HookRegistry::new();
    hooks.register(HookEvent::PostToolUse, Box::new(DenyPostHook));
    let engine = AgentEngine::new(EventSequencer::new()).with_hooks(hooks);
    let provider = FakeProvider {
        rounds: Mutex::new(vec![
            vec![
                EngineProviderEvent::ToolCallDelta {
                    index: 0,
                    id: Some("post-deny-1".into()),
                    name: Some("echo".into()),
                    arguments_delta: r#"{"ok":true}"#.into(),
                },
                EngineProviderEvent::CompletedWithReason {
                    reason: ProviderStopReason::ToolUse,
                },
            ],
            vec![
                EngineProviderEvent::TextDelta("done".into()),
                EngineProviderEvent::Completed,
            ],
        ]),
    };
    let run_id = format!("post-deny-{}", uuid::Uuid::new_v4());
    let error = engine
        .run(
            EngineRunConfig {
                run_id: run_id.clone(),
                conversation_id: "post-deny-conversation".into(),
                model: "m".into(),
                system_prompt: None,
                messages: Vec::new(),
                user_content: "use echo".into(),
                max_steps: 3,
            },
            &provider,
            &FakeTools,
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), "hook_refused");
    assert!(
        error.to_string().contains("cannot be honoured"),
        "the refusal must explain the post-event contract: {error}"
    );
    // The refusal aborts BEFORE the completion fact is appended, so the tool's
    // effect stays unsettled (the ledger/resume gate treats it as uncertain —
    // fail-closed, never replay-safe).
    let completed: Vec<_> = engine
        .events
        .replay_after(&run_id, 0)
        .into_iter()
        .filter(|event| matches!(event.payload, RunEventKind::ToolCallCompleted { .. }))
        .collect();
    assert_eq!(
        completed.len(),
        0,
        "a refused post hook must not settle the tool's effect"
    );
}

#[tokio::test]
async fn post_tool_use_observe_continues_run() {
    // T03: an observation-only PostToolUse hook (Allow) must not disturb the
    // run; the follow-up turn still completes.
    struct ObservePostHook;
    #[async_trait::async_trait]
    impl crate::hooks::HookHandler for ObservePostHook {
        async fn handle(&self, _: HookRequest) -> crate::hooks::HookResponse {
            crate::hooks::HookResponse {
                decision: HookDecision::Allow,
            }
        }
    }
    let mut hooks = HookRegistry::new();
    hooks.register(HookEvent::PostToolUse, Box::new(ObservePostHook));
    let engine = AgentEngine::new(EventSequencer::new()).with_hooks(hooks);
    let provider = FakeProvider {
        rounds: Mutex::new(vec![
            vec![
                EngineProviderEvent::ToolCallDelta {
                    index: 0,
                    id: Some("post-obs-1".into()),
                    name: Some("echo".into()),
                    arguments_delta: r#"{"ok":true}"#.into(),
                },
                EngineProviderEvent::CompletedWithReason {
                    reason: ProviderStopReason::ToolUse,
                },
            ],
            vec![
                EngineProviderEvent::TextDelta("after observe".into()),
                EngineProviderEvent::Completed,
            ],
        ]),
    };
    let run_id = format!("post-obs-{}", uuid::Uuid::new_v4());
    // A1: text deltas are live-only — subscribe before the run.
    let mut live_rx = engine.live.subscribe(&run_id);
    engine
        .run(
            EngineRunConfig {
                run_id: run_id.clone(),
                conversation_id: "post-obs-conversation".into(),
                model: "m".into(),
                system_prompt: None,
                messages: Vec::new(),
                user_content: "use echo".into(),
                max_steps: 3,
            },
            &provider,
            &FakeTools,
        )
        .await
        .unwrap();
    let mut text: Vec<String> = Vec::new();
    while let Ok(event) = live_rx.try_recv() {
        if let RunEventKind::TextDelta { text: t } = event.kind {
            text.push(t);
        }
    }
    assert!(
        text.iter().any(|t| t.contains("after observe")),
        "the run must continue past an observing post hook"
    );
}

#[tokio::test]
async fn live_text_delta_never_calls_durable_persistence() {
    // A1 regression: a 1000-delta text stream must not grow the durable
    // persistence append count (live lane only). Lifecycle facts still persist.
    struct CountingPersistence(Arc<AtomicUsize>);
    impl EventPersistence for CountingPersistence {
        fn append(&self, _: &assistant_protocol::v2::RunEventV2) -> Result<(), String> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        fn replay_after(
            &self,
            _: &str,
            _: u64,
        ) -> Result<Vec<assistant_protocol::v2::RunEventV2>, String> {
            Ok(Vec::new())
        }
        fn last_sequence(&self, _: &str) -> Result<u64, String> {
            Ok(0)
        }
    }
    struct ChunkProvider(usize);
    #[async_trait::async_trait]
    impl EngineProvider for ChunkProvider {
        async fn stream(
            &self,
            _: &str,
            _: Vec<EngineMessage>,
            _: &[ToolSchema],
            _: Option<&str>,
            _: CancellationToken,
        ) -> Result<EngineProviderEventStream, EngineError> {
            let mut events = Vec::with_capacity(self.0 + 1);
            for i in 0..self.0 {
                events.push(EngineProviderEvent::TextDelta(format!("chunk-{i}")));
            }
            events.push(EngineProviderEvent::Completed);
            Ok(Box::pin(futures_util::stream::iter(events)))
        }
    }
    let count = Arc::new(AtomicUsize::new(0));
    let engine = AgentEngine::new(EventSequencer::with_persistence(Arc::new(
        CountingPersistence(count.clone()),
    )));
    let run_id = format!("live-delta-{}", uuid::Uuid::new_v4());
    engine
        .run(
            EngineRunConfig {
                run_id: run_id.clone(),
                conversation_id: "conversation".into(),
                model: "model".into(),
                system_prompt: None,
                messages: Vec::new(),
                user_content: "hello".into(),
                max_steps: 1,
            },
            &ChunkProvider(1000),
            &FakeTools,
        )
        .await
        .unwrap();
    let durable_calls = count.load(Ordering::SeqCst);
    // Durable facts only (turn started/message started/generation
    // attempt/message completed/turn completed…) — must be far below 1000 and
    // independent of delta count; text deltas themselves are live-only.
    assert!(
        durable_calls < 10,
        "durable persistence called {durable_calls} times for a 1000-delta stream"
    );
    // Live lane sequenced every delta (sequence counter does not depend on
    // the broadcast window, so a 1000-delta stream is asserted exactly).
    assert_eq!(
        engine.live.last_sequence(&run_id),
        1000,
        "all text deltas must be sequenced on the live bus"
    );
}
