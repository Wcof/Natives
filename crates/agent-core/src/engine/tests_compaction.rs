use super::*;

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
    let engine = AgentEngine::new(EventSequencer::memory_only()).with_context_budget(1_000, 512);
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
async fn context_stats_tracked_and_reset_by_engine_loop() {
    // P1-05: the engine loop must be a real caller of ContextStats — it
    // appends transcript chars incrementally and, after a real compaction,
    // bumps the revision and re-seeds the counters from the kept size.
    let engine = AgentEngine::new(EventSequencer::memory_only()).with_context_budget(1_000, 512);
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
    let run_id = format!("r-stats-{}", uuid::Uuid::new_v4());
    let status = engine
        .run(
            EngineRunConfig {
                run_id: run_id.clone(),
                conversation_id: "c-stats".into(),
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

    let stats = engine.context_stats.lock().unwrap().clone();
    assert!(
        stats.last_compaction_revision >= 1,
        "engine loop must note the real compaction (revision={})",
        stats.last_compaction_revision
    );
    assert!(
        stats.estimated_chars < 100_000,
        "stats must reset to the kept transcript size, not accumulate stale bytes (chars={})",
        stats.estimated_chars
    );
}

#[tokio::test]
async fn context_stats_estimate_tracks_transcript_not_cumulative_sum() {
    // PERF-001: the engine loop must observe the transcript (append only the
    // per-round growth), not re-accumulate the full history every round. After
    // a tool round below the compaction threshold, the estimate must sit
    // exactly at the cheap char count of the transcript the engine observed —
    // a cumulative implementation would land far above it after repeated
    // rounds.
    let engine =
        AgentEngine::new(EventSequencer::memory_only()).with_context_budget(10_000_000, 512);
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
    let status = engine
        .run(
            EngineRunConfig {
                run_id: format!("r-observe-{}", uuid::Uuid::new_v4()),
                conversation_id: "c-observe".into(),
                model: "m".into(),
                system_prompt: None,
                messages: Vec::new(),
                user_content: "hello".into(),
                max_steps: 2,
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

    let stats = engine.context_stats.lock().unwrap().clone();
    // Reconstruct the transcript the engine observed after the tool round
    // (turn 1's assistant message + its tool result; the final text turn
    // returns before a second observation). `values_chars` only sums string
    // lengths, so random UUID message ids match by construction.
    let mut final_messages = engine_messages_to_agent_messages(&[EngineMessage {
        role: "user".into(),
        content: "hello".into(),
        tool_call_id: None,
        tool_name: None,
        tool_calls: None,
        images: Vec::new(),
    }]);
    final_messages.push(crate::AgentMessage::Assistant(crate::AssistantMessage {
        message_id: crate::MessageId::new(),
        content: vec![crate::ContentBlock::ToolCall(crate::ToolCall {
            tool_call_id: crate::ToolCallId::from("t1"),
            name: "echo".into(),
            arguments_json: r#"{"x":1}"#.into(),
        })],
        stop_reason: Some(crate::StopReason::ToolUse),
    }));
    final_messages.push(crate::AgentMessage::ToolResult(crate::ToolResultMessage {
        message_id: crate::MessageId::new(),
        tool_call_id: crate::ToolCallId::from("t1"),
        tool_name: "echo".into(),
        content: vec![crate::ToolResultBlock::Json {
            value: serde_json::json!({ "body": "y".repeat(5_000) }),
        }],
        is_error: false,
        code: None,
    }));
    let expected = values_chars(&agent_messages_to_values(&final_messages));
    assert_eq!(
        stats.estimated_chars as usize,
        expected,
        "estimate must equal the final transcript size, not a cumulative sum (chars={}, expected={})",
        stats.estimated_chars,
        expected
    );
}

#[tokio::test]
async fn compaction_falls_back_to_mechanical_when_summary_fails() {
    let engine = AgentEngine::new(EventSequencer::memory_only()).with_context_budget(1_000, 512);
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
    let engine = AgentEngine::new(EventSequencer::memory_only()).with_context_budget(1_000, 512);
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
    let engine = AgentEngine::new(EventSequencer::memory_only()).with_context_budget(1_000, 512);
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
    let engine = AgentEngine::new(EventSequencer::memory_only());
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
    let engine = AgentEngine::new(EventSequencer::memory_only())
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
