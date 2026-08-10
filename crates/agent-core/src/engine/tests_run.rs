use super::*;

#[tokio::test]
async fn completes_simple_text_turn() {
    let engine = AgentEngine::new(EventSequencer::memory_only());
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
    let engine = AgentEngine::new(EventSequencer::memory_only());
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
    let engine = AgentEngine::new(EventSequencer::memory_only());
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
    let engine = AgentEngine::new(EventSequencer::memory_only()).with_input_receiver(Arc::new(
        FollowUpReceiver {
            offered: AtomicBool::new(false),
        },
    ));
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
    let engine =
        AgentEngine::new(EventSequencer::memory_only()).with_input_receiver(receiver.clone());
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
