use super::*;

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
    let engine = AgentEngine::new(EventSequencer::memory_only()).with_hooks(hooks);
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
    let engine = AgentEngine::new(EventSequencer::memory_only()).with_hooks(hooks);
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
