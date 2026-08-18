use super::*;

#[tokio::test]
async fn session_end_hook_reports_domain_outcome() {
    struct RecordingHook(Arc<Mutex<Vec<bool>>>);

    #[async_trait::async_trait]
    impl crate::hooks::HookHandler for RecordingHook {
        async fn handle(&self, request: HookRequest) -> crate::hooks::HookResponse {
            self.0
                .lock()
                .unwrap()
                .push(request.input["success"].as_bool().unwrap());
            crate::hooks::HookResponse {
                decision: HookDecision::Allow,
            }
        }
    }

    for (rounds, expected_status, expected_success) in [
        (
            Vec::new(),
            assistant_protocol::v2::RunStatusV2::Completed,
            true,
        ),
        (
            vec![vec![
                EngineProviderEvent::TextDelta("failed".into()),
                EngineProviderEvent::CompletedWithReason {
                    reason: ProviderStopReason::Error,
                },
            ]],
            assistant_protocol::v2::RunStatusV2::Failed,
            false,
        ),
        (
            vec![vec![
                EngineProviderEvent::TextDelta("cancelled".into()),
                EngineProviderEvent::CompletedWithReason {
                    reason: ProviderStopReason::Cancelled,
                },
            ]],
            assistant_protocol::v2::RunStatusV2::Cancelled,
            false,
        ),
    ] {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let mut hooks = HookRegistry::new();
        hooks.register(HookEvent::SessionEnd, Box::new(RecordingHook(seen.clone())));
        let engine = AgentEngine::new(EventSequencer::memory_only()).with_hooks(hooks);
        let provider = FakeProvider {
            rounds: Mutex::new(rounds),
        };

        let outcome = engine
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

        assert_eq!(outcome.target_status(), expected_status);
        assert_eq!(*seen.lock().unwrap(), vec![expected_success]);
    }
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
    // memory_only: this test asserts hook Inject reaches the provider; the
    // shared event log dir is never touched so parallel env mutation from
    // other sequencer tests cannot fail hook telemetry persistence.
    AgentEngine::new(EventSequencer::memory_only())
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
    let engine = AgentEngine::new(EventSequencer::memory_only()).with_hooks(hooks);
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
async fn pre_tool_modify_is_the_single_argument_authority() {
    struct ModifyHook;
    #[async_trait::async_trait]
    impl crate::hooks::HookHandler for ModifyHook {
        async fn handle(&self, _: HookRequest) -> crate::hooks::HookResponse {
            crate::hooks::HookResponse {
                decision: HookDecision::Modify {
                    payload: json!({"enabled": true, "path": "modified"}),
                },
            }
        }
    }
    struct RecordingTools(Arc<Mutex<Vec<Value>>>);
    #[async_trait::async_trait]
    impl EngineToolRuntime for RecordingTools {
        async fn list_tool_schemas(&self) -> Vec<ToolSchema> {
            vec![ToolSchema {
                name: "echo".into(),
                description: "echo".into(),
                input_schema: json!({"type": "object"}),
            }]
        }

        async fn execute_tool(
            &self,
            _name: &str,
            input: Value,
            _cancel: &CancellationToken,
        ) -> ToolExecutionResult {
            self.0.lock().unwrap().push(input.clone());
            ToolExecutionResult {
                output: json!({"input": input}),
                is_error: false,
                duration_ms: 1,
            }
        }
    }
    struct CapturingProvider {
        rounds: Mutex<Vec<Vec<EngineProviderEvent>>>,
        messages: Mutex<Vec<Vec<EngineMessage>>>,
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
            self.messages.lock().unwrap().push(messages);
            Ok(Box::pin(futures_util::stream::iter(
                self.rounds.lock().unwrap().remove(0),
            )))
        }
    }

    let expected = json!({"enabled": true, "path": "modified"});
    let expected_args = serde_json::to_string(&expected).unwrap();
    let mut hooks = HookRegistry::new();
    hooks.register(HookEvent::PreToolUse, Box::new(ModifyHook));
    let engine = AgentEngine::new(EventSequencer::memory_only()).with_hooks(hooks);
    let tools = RecordingTools(Arc::new(Mutex::new(Vec::new())));
    let provider = CapturingProvider {
        rounds: Mutex::new(vec![
            vec![
                EngineProviderEvent::ToolCallDelta {
                    index: 0,
                    id: Some("modify-1".into()),
                    name: Some("echo".into()),
                    arguments_delta: r#"{"path":"original"}"#.into(),
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
        messages: Mutex::new(Vec::new()),
    };
    let run_id = format!("modify-run-{}", uuid::Uuid::new_v4());
    engine
        .run(
            EngineRunConfig {
                run_id: run_id.clone(),
                conversation_id: "modify-conversation".into(),
                model: "m".into(),
                system_prompt: None,
                messages: Vec::new(),
                user_content: "use echo".into(),
                max_steps: 3,
            },
            &provider,
            &tools,
        )
        .await
        .unwrap();

    assert_eq!(*tools.0.lock().unwrap(), vec![expected.clone()]);
    let events = engine.events.replay_after(&run_id, 0);
    let prepared_inputs = events
        .iter()
        .filter_map(|event| match &event.payload {
            RunEventKind::ToolCallPrepared { input, .. } => Some(input.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(prepared_inputs, vec![expected.clone()]);
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            RunEventKind::ToolCallCompleted { output, .. } if output["input"] == expected
        )
    }));
    let transcript_args = events.iter().find_map(|event| match &event.payload {
        RunEventKind::MessageCompleted {
            role,
            content: Some(content),
            ..
        } if role == "assistant" => content["content"].as_array().and_then(|blocks| {
            blocks.iter().find_map(|block| {
                block["ToolCall"]["arguments_json"]
                    .as_str()
                    .map(str::to_string)
            })
        }),
        _ => None,
    });
    assert_eq!(transcript_args.as_deref(), Some(expected_args.as_str()));
    let provider_messages = provider.messages.lock().unwrap();
    assert_eq!(provider_messages.len(), 2);
    assert_eq!(
        provider_messages[1]
            .iter()
            .find_map(|message| message.tool_calls.as_ref())
            .and_then(|calls| calls.first())
            .map(|call| call.arguments.as_str()),
        Some(expected_args.as_str())
    );
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
    struct SideEffectRuntime(AtomicUsize);
    #[async_trait::async_trait]
    impl EngineToolRuntime for SideEffectRuntime {
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
            self.0.fetch_add(1, Ordering::SeqCst);
            ToolExecutionResult {
                output: serde_json::json!({"tool": name, "input": input}),
                is_error: false,
                duration_ms: 1,
            }
        }
    }
    let mut hooks = HookRegistry::new();
    hooks.register(HookEvent::PostToolUse, Box::new(DenyPostHook));
    let engine = AgentEngine::new(EventSequencer::memory_only()).with_hooks(hooks);
    let tools = SideEffectRuntime(AtomicUsize::new(0));
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
            &tools,
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), "hook_refused");
    assert!(
        error.to_string().contains("cannot be honoured"),
        "the refusal must explain the post-event contract: {error}"
    );
    assert_eq!(
        tools.0.load(Ordering::SeqCst),
        1,
        "the tool side effect ran once"
    );
    let completed: Vec<_> = engine
        .events
        .replay_after(&run_id, 0)
        .into_iter()
        .filter(|event| matches!(event.payload, RunEventKind::ToolCallCompleted { .. }))
        .collect();
    assert_eq!(
        completed.len(),
        1,
        "a refused post hook must not erase the executed tool outcome"
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
    let engine = AgentEngine::new(EventSequencer::memory_only()).with_hooks(hooks);
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
