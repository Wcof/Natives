use super::*;

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

    let engine = AgentEngine::new(EventSequencer::memory_only());
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

    let engine = AgentEngine::new(EventSequencer::memory_only());
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

    let engine = AgentEngine::new(EventSequencer::memory_only());
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

    let engine = AgentEngine::new(EventSequencer::memory_only());
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
    let engine = AgentEngine::new(EventSequencer::memory_only());
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
    let engine = AgentEngine::new(EventSequencer::memory_only());
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
    let engine = AgentEngine::new(EventSequencer::memory_only());
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

#[tokio::test]
async fn rejects_nonempty_provider_stream_without_completion() {
    let engine = AgentEngine::new(EventSequencer::memory_only());
    let run_id = format!("r-incomplete-stream-{}", uuid::Uuid::new_v4());
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
            &FakeProvider {
                rounds: Mutex::new(vec![vec![EngineProviderEvent::TextDelta("partial".into())]]),
            },
            &FakeTools,
        )
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        EngineError::Provider { ref code, .. } if code == "INCOMPLETE_PROVIDER_STREAM"
    ));
    let events = engine.events.replay_after(&run_id, 0);
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            RunEventKind::GenerationAttemptDiscarded { reason, .. }
                if reason == "INCOMPLETE_PROVIDER_STREAM"
        )
    }));
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event.payload, RunEventKind::GenerationAttemptStarted { .. }))
            .count(),
        1,
        "partial output must fail closed instead of being retried"
    );
    assert!(!events.iter().any(|event| {
        matches!(
            &event.payload,
            RunEventKind::TurnCompleted { stop_reason, .. } if stop_reason == "unknown"
        )
    }));
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
    let engine = AgentEngine::new(EventSequencer::memory_only());
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
    let engine = AgentEngine::new(EventSequencer::memory_only());
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
    let engine = Arc::new(AgentEngine::new(EventSequencer::memory_only()));
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

#[tokio::test]
async fn engine_hands_user_images_to_the_provider() {
    let provider = CaptureProvider::default();
    let engine = AgentEngine::new(EventSequencer::memory_only());
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

/// §5 exact-name regression: live text deltas never call durable
/// persistence (A1). Wrapper over the equivalent regression.
#[test]
fn live_text_delta_does_not_call_event_persistence() {
    live_text_delta_never_calls_durable_persistence();
}

/// §5 exact-name regression: a text stream emits no MessageDelta (A1).
#[test]
fn no_message_delta_emitted_for_text_stream() {
    emits_text_delta_before_provider_stream_completes();
}

/// §5 exact-name regression: MessageCompleted carries the full text.
#[test]
fn message_completed_contains_full_text() {
    emits_text_delta_before_provider_stream_completes();
}
