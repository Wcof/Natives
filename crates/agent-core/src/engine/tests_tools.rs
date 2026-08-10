use super::*;

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
    AgentEngine::new(EventSequencer::memory_only())
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
    let engine = AgentEngine::new(EventSequencer::memory_only());
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
    let engine = AgentEngine::new(EventSequencer::memory_only());
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
    let engine = AgentEngine::new(EventSequencer::memory_only());
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
async fn length_stop_never_executes_collected_tool_call() {
    let engine = AgentEngine::new(EventSequencer::memory_only());
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

    let engine = AgentEngine::new(EventSequencer::memory_only());
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
