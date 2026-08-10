use super::*;

    #[tokio::test]
    async fn start_cancel_retry_lifecycle_with_fixture() {
        let _env = crate::storage::DataStore::env_test_lock();
        std::env::set_var("NATIVES_DAEMON_FIXTURE", "1");
        let dir = tempfile::tempdir().unwrap();
        let previous_db = std::env::var("NATIVES_DB_PATH").ok();
        let previous_runtime = std::env::var("NATIVES_RUNTIME_DIR").ok();
        let db_path = dir.path().join(format!("natives-{}.db", Uuid::new_v4()));
        std::env::set_var("NATIVES_DB_PATH", &db_path);
        std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db_path);
        std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());
        crate::storage::set_test_db_override(
            Some(db_path.clone()),
            Some(dir.path().join("artifacts")),
        );
        let store = Arc::new(
            crate::storage::DataStore::new(&db_path, &dir.path().join("artifacts")).unwrap(),
        );
        store.conn().unwrap().execute(
            "INSERT INTO conversation (id, mode, title, provider_id, model_id, permission_profile_id)
             VALUES ('c1', 'agent', 'Fixture', 'openai', 'gpt-4o', 'full_access')",
            [],
        ).unwrap();
        let rm = RunManager::new_with_store(store.clone());
        let run = rm
            .start(StartRunRequest {
                agent_profile_id: None,
                capability_selection: None,
                run_id: None,
                conversation_id: Some("c1".into()),
                provider_id: Some("openai".into()),
                model_id: Some("gpt-4o".into()),
                key_id: Some("k-test".into()),
                content: Some("ping".into()),
                attachments: None,
                trigger_message_id: None,
                permission_profile: Some("full_access".into()),
                max_steps: Some(5),
                project_path: Some(dir.path().to_string_lossy().into_owned()),
                idempotency_key: None,
                effort: None,
                runtime_id: None,
            })
            .await
            .unwrap();
        assert_eq!(run.status, RunStatusV2::Completed);
        let events = rm.replay(ReplayRunRequest {
            run_id: run.id.clone(),
            after_sequence: 0,
        });
        assert!(events
            .iter()
            .any(|e| matches!(e.payload, RunEventKind::Started)));
        // A1 contract: TextDelta is live-only and must NOT be in the durable
        // store; the committed content arrives via MessageCompleted.
        assert!(
            !events
                .iter()
                .any(|e| matches!(e.payload, RunEventKind::TextDelta { .. })),
            "text deltas are live-only after the A1 split"
        );
        assert!(events
            .iter()
            .any(|e| matches!(e.payload, RunEventKind::MessageCompleted { .. })));

        // Dump evidence for verifier
        if let Ok(dir) = std::env::var("NATIVES_TEST_SCRATCH") {
            let dump: Vec<_> = events
                .iter()
                .map(|e| {
                    serde_json::json!({
                        "run_id": e.run_id,
                        "sequence": e.run_sequence,
                        "type": e.payload.type_name(),
                    })
                })
                .collect();
            let _ = std::fs::write(
                std::path::Path::new(&dir).join("run-events.json"),
                serde_json::to_string_pretty(&dump).unwrap_or_default(),
            );
        }

        // Cancel on already-completed is terminal-safe
        let cancelled = rm
            .cancel(CancelRunRequest {
                run_id: run.id.clone(),
            })
            .await
            .unwrap();
        assert!(cancelled.status.is_terminal());

        // Retry produces new run id
        let retried = rm
            .retry(RetryRunRequest {
                run_id: run.id.clone(),
            })
            .unwrap();
        assert_ne!(retried.id, run.id);

        // Start the retried run
        let retried_done = rm
            .start(StartRunRequest {
                agent_profile_id: None,
                capability_selection: None,
                run_id: Some(retried.id.clone()),
                conversation_id: None,
                provider_id: None,
                model_id: None,
                key_id: None,
                content: None,
                attachments: None,
                trigger_message_id: None,
                permission_profile: Some("full_access".into()),
                max_steps: Some(5),
                project_path: None,
                idempotency_key: None,
                effort: None,
                runtime_id: None,
            })
            .await
            .unwrap();
        assert_eq!(retried_done.status, RunStatusV2::Completed);

        if let Ok(dir) = std::env::var("NATIVES_TEST_SCRATCH") {
            let evidence = serde_json::json!({
                "original_run_id": run.id,
                "retry_run_id": retried.id,
                "ids_differ": run.id != retried.id,
                "original_event_count": events.len(),
                "retry_completed": retried_done.status == RunStatusV2::Completed,
            });
            let _ = std::fs::write(
                std::path::Path::new(&dir).join("daemon-cancel-retry.json"),
                serde_json::to_string_pretty(&evidence).unwrap_or_default(),
            );
        }
        if let Some(value) = previous_db {
            std::env::set_var("NATIVES_DB_PATH", &value);
        } else {
            std::env::remove_var("NATIVES_DB_PATH");
        }
        if let Some(value) = previous_runtime {
            std::env::set_var("NATIVES_RUNTIME_DIR", &value);
        } else {
            std::env::remove_var("NATIVES_RUNTIME_DIR");
        }
        // keep FIXTURE=1 for parallel tests under cfg(test)
    }

    /// Criterion 4: parent cancel_run_tree must request_cancel child engines
    /// registered under child run_id (not metadata-only cascade).
    #[tokio::test]
    async fn parent_cancel_tree_cancels_child_engine() {
        // Isolate from concurrent env-DB tests (store_from_env must stay None).
        let _guard = crate::storage::DataStore::env_test_lock();
        std::env::remove_var("NATIVES_ASSISTANT_DB_PATH");
        std::env::remove_var("NATIVES_DB_PATH");
        std::env::set_var("NATIVES_DAEMON_FIXTURE", "1");
        let rm = Arc::new(RunManager::new());
        let parent = rm
            .create_run(CreateRunRequest {
                capability_selection: None,
                disabled_tools: None,
                conversation_id: "c-tree".into(),
                provider_id: "openai".into(),
                model_id: "gpt-4o".into(),
                key_id: Some("parent-key".into()),
                agent_profile_id: None,
                permission_profile: Some("full_access".into()),
                content: Some("parent".into()),
                attachments: None,
                max_steps: Some(5),
                parent_run_id: None,
                project_path: None,
                // EventSequencer may persist across test processes; keep this run's
                // replay cursor isolated while preserving the one-terminal assertion.
                idempotency_key: Some(format!("tree-parent-{}", Uuid::new_v4())),
                effort: None,
                runtime_id: None,
            })
            .unwrap();

        // Spawn real subagent identity under parent run_id.
        let child = rm
            .runtime
            .subagents
            .spawn(
                &parent.id,
                "child work".into(),
                1,
                "anthropic".into(),
                "child-key-from-broker".into(),
                "claude-3".into(),
                "ask".into(),
                vec!["read_file".into()],
                None,
                Some("none".into()),
                None,
            )
            .await
            .unwrap();
        let _ = rm
            .runtime
            .subagents
            .update_status(&child.id, agent_core::SubAgentStatus::Running)
            .await;

        // Register a live child AgentEngine on child.run_id (production path).
        let child_engine = Arc::new(AgentEngine::with_live(
            rm.runtime.events.clone(),
            rm.runtime.live.clone(),
        ));
        rm.runtime
            .register_engine(&child.run_id, child_engine.clone())
            .await;
        rm.runtime
            .insert_task_output(
                &child.id,
                crate::production::TaskRecord {
                    run_id: child.run_id.clone(),
                    status: "running".into(),
                    output: None,
                },
            )
            .await;

        // Child tool observes cancel flag (same bar as cancel_mid).
        let seen = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let seen_bg = seen.clone();
        let child_flag = child_engine.cancel_token();
        let child_run = child.run_id.clone();
        let watch = tokio::spawn(async move {
            for _ in 0..200 {
                if child_flag.is_cancelled() {
                    seen_bg.store(true, std::sync::atomic::Ordering::SeqCst);
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
            let _ = child_run;
        });

        // Domain tree signal via cancel_run_tree; authoritative Cancelled via RunManager::cancel.
        rm.runtime.cancel_run_tree(&parent.id).await;

        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), watch).await;
        assert!(
            seen.load(std::sync::atomic::Ordering::SeqCst),
            "child engine cancel flag must be set by cancel_run_tree(parent)"
        );

        let child_status = rm
            .runtime
            .subagents
            .get(&child.id)
            .await
            .map(|s| s.status)
            .unwrap();
        assert_eq!(child_status, agent_core::SubAgentStatus::Cancelled);

        let task_status = rm
            .runtime
            .task_outputs
            .lock()
            .await
            .get(&child.id)
            .map(|t| t.status.clone());
        assert_eq!(task_status.as_deref(), Some("cancelled"));

        // Lifecycle Cancelled is committed only by RunManager::cancel (journal), not domain cleanup.
        let cancelled = rm
            .cancel(CancelRunRequest {
                run_id: parent.id.clone(),
            })
            .await
            .expect("cancel parent via journal");
        assert!(
            cancelled.status.is_terminal(),
            "parent must be terminal after RunManager::cancel"
        );
        let parent_lifecycle = rm.runtime.events.replay_after(&parent.id, 0);
        let terminal_count = parent_lifecycle
            .iter()
            .filter(|e| {
                matches!(
                    e.payload,
                    RunEventKind::Cancelled { .. }
                        | RunEventKind::Failed { .. }
                        | RunEventKind::Completed { .. }
                        | RunEventKind::Interrupted { .. }
                )
            })
            .count();
        assert_eq!(
            terminal_count, 1,
            "parent must have exactly one terminal lifecycle event after cancel"
        );

        if let Ok(dir) = std::env::var("NATIVES_TEST_SCRATCH") {
            let evidence = serde_json::json!({
                "parent_run_id": parent.id,
                "child_run_id": child.run_id,
                "child_task_id": child.id,
                "child_engine_cancel_flag": true,
                "child_status": "cancelled",
                "parent_terminal_lifecycle_events": terminal_count,
                "api": "cancel_run_tree+RunManager::cancel",
            });
            let _ = std::fs::write(
                std::path::Path::new(&dir).join("cascade-cancel-tree.json"),
                serde_json::to_string_pretty(&evidence).unwrap_or_default(),
            );
        }
        // keep FIXTURE=1 for parallel tests under cfg(test)
    }

    #[tokio::test]
    async fn cancel_mid_run_marks_interrupted() {
        // Hermetic store: temp DB + thread-local override + env restore so the
        // test never reads a leaked parallel-test env var or the real ~/.natives.
        let _env_guard = crate::storage::DataStore::env_test_lock();
        let _env_restore = crate::storage::EnvRestore::capture();
        let store_dir = tempfile::tempdir().unwrap();
        let db_path = store_dir.path().join("cancel-mid.db");
        std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db_path);
        std::env::set_var("NATIVES_DB_PATH", &db_path);
        crate::storage::set_test_db_override(
            Some(db_path),
            Some(store_dir.path().join("artifacts")),
        );
        std::env::set_var("NATIVES_DAEMON_FIXTURE", "1");
        let runtime_dir = std::env::temp_dir().join(format!("natives-cancel-{}", Uuid::new_v4()));
        std::fs::create_dir_all(runtime_dir.join("runs")).unwrap();
        let prev_runtime_dir = std::env::var("NATIVES_RUNTIME_DIR").ok();
        std::env::set_var("NATIVES_RUNTIME_DIR", &runtime_dir);
        let rm = Arc::new(RunManager::new());
        let run = rm
            .create_run(CreateRunRequest {
                capability_selection: None,
                disabled_tools: None,
                conversation_id: "c-cancel".into(),
                provider_id: "openai".into(),
                model_id: "gpt-4o".into(),
                key_id: Some("k".into()),
                agent_profile_id: None,
                permission_profile: Some("ask".into()),
                content: Some("slow".into()),
                attachments: None,
                max_steps: Some(50),
                parent_run_id: None,
                project_path: None,
                idempotency_key: Some("cancel-mid".into()),
                effort: None,
                runtime_id: None,
            })
            .unwrap();

        let cancel_flag_seen = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let cancel_flag_seen_bg = cancel_flag_seen.clone();
        let cancel_started = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let cancel_started_bg = cancel_started.clone();

        let rm_start = rm.clone();
        let rid = run.id.clone();
        let start_handle = tokio::spawn(async move {
            #[allow(dead_code)]
            struct SlowProvider {
                seen: Arc<std::sync::atomic::AtomicBool>,
            }
            #[async_trait::async_trait]
            impl agent_core::EngineProvider for SlowProvider {
                async fn stream(
                    &self,
                    _model: &str,
                    _messages: Vec<agent_core::EngineMessage>,
                    _tools: &[agent_core::ToolSchema],
                    _system_prompt: Option<&str>,
                    _cancel: CancellationToken,
                ) -> Result<agent_core::EngineProviderEventStream, agent_core::EngineError>
                {
                    // Stay in stream long enough for cancel to register.
                    for _ in 0..100 {
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    }
                    let _ = &self.seen;
                    Ok(Box::pin(futures_util::stream::iter(vec![
                        agent_core::EngineProviderEvent::TextDelta("late".into()),
                        agent_core::EngineProviderEvent::Completed,
                    ])))
                }
            }
            struct CancelAwareTools {
                run_id: String,
                seen: Arc<std::sync::atomic::AtomicBool>,
                started: Arc<std::sync::atomic::AtomicBool>,
            }
            #[async_trait::async_trait]
            impl agent_core::EngineToolRuntime for CancelAwareTools {
                async fn list_tool_schemas(&self) -> Vec<agent_core::ToolSchema> {
                    vec![]
                }
                async fn execute_tool(
                    &self,
                    _name: &str,
                    _input: serde_json::Value,
                    cancel: &CancellationToken,
                ) -> agent_core::ToolExecutionResult {
                    // Signal that tool execution has begun so the test cancels
                    // mid-tool instead of racing the engine's startup loop.
                    self.started
                        .store(true, std::sync::atomic::Ordering::SeqCst);
                    // Poll cancel token while "working".
                    for _ in 0..40 {
                        if cancel.is_cancelled() {
                            self.seen.store(true, std::sync::atomic::Ordering::SeqCst);
                            return agent_core::ToolExecutionResult {
                                output: serde_json::json!({"error": "cancelled"}),
                                is_error: true,
                                duration_ms: 0,
                            };
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                    }
                    let _ = &self.run_id;
                    agent_core::ToolExecutionResult {
                        output: serde_json::json!({}),
                        is_error: false,
                        duration_ms: 0,
                    }
                }
            }
            // Provider that emits a tool call so tools path can observe cancel.
            struct ToolThenSlow;
            #[async_trait::async_trait]
            impl agent_core::EngineProvider for ToolThenSlow {
                async fn stream(
                    &self,
                    _model: &str,
                    messages: Vec<agent_core::EngineMessage>,
                    _tools: &[agent_core::ToolSchema],
                    _system_prompt: Option<&str>,
                    _cancel: CancellationToken,
                ) -> Result<agent_core::EngineProviderEventStream, agent_core::EngineError>
                {
                    if messages.last().map(|m| m.role == "tool").unwrap_or(false) {
                        return Ok(Box::pin(futures_util::stream::iter(vec![
                            agent_core::EngineProviderEvent::TextDelta("done".into()),
                            agent_core::EngineProviderEvent::Completed,
                        ])));
                    }
                    Ok(Box::pin(futures_util::stream::iter(vec![
                        agent_core::EngineProviderEvent::ToolCallDelta {
                            index: 0,
                            id: Some("c1".into()),
                            name: Some("read_file".into()),
                            arguments_delta: r#"{"path":"x"}"#.into(),
                        },
                        agent_core::EngineProviderEvent::CompletedWithReason {
                            reason: agent_core::ProviderStopReason::ToolUse,
                        },
                    ])))
                }
            }
            let tools = CancelAwareTools {
                run_id: rid.clone(),
                seen: cancel_flag_seen_bg,
                started: cancel_started_bg,
            };
            rm_start
                .start_with_seams(
                    StartRunRequest {
                        agent_profile_id: None,
                        capability_selection: None,
                        run_id: Some(rid),
                        conversation_id: None,
                        provider_id: None,
                        model_id: None,
                        key_id: None,
                        content: Some("slow".into()),
                        attachments: None,
                        trigger_message_id: None,
                        permission_profile: Some("full_access".into()),
                        max_steps: Some(5),
                        project_path: None,
                        idempotency_key: None,
                        effort: None,
                        runtime_id: None,
                    },
                    &ToolThenSlow,
                    &tools,
                )
                .await
        });

        // Wait until engine is registered, then cancel → request_cancel.
        for _ in 0..50 {
            if rm.runtime.has_engine(&run.id).await {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert!(
            rm.runtime.has_engine(&run.id).await,
            "engine must be registered before cancel"
        );
        // Wait until the tool is actually executing so cancel lands mid-tool,
        // not in the engine-startup window where a cancelled token aborts the
        // run before any tool runs (would make the flag assertion vacuous).
        for _ in 0..200 {
            if cancel_started.load(std::sync::atomic::Ordering::SeqCst) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert!(
            cancel_started.load(std::sync::atomic::Ordering::SeqCst),
            "tool must start before cancel"
        );
        let cancelled = rm
            .cancel(CancelRunRequest {
                run_id: run.id.clone(),
            })
            .await
            .unwrap();
        assert_eq!(cancelled.status, RunStatusV2::Cancelled);

        let start_result = tokio::time::timeout(std::time::Duration::from_secs(6), start_handle)
            .await
            .expect("start should finish after cancel")
            .expect("join ok");
        let _ = start_result;

        let engine_cancelled = cancel_flag_seen.load(std::sync::atomic::Ordering::SeqCst);
        let has_cancel_lifecycle = rm.runtime.events.replay_after(&run.id, 0).iter().any(|e| {
            matches!(
                e.payload,
                RunEventKind::Cancelled { .. } | RunEventKind::Interrupted { .. }
            )
        });

        if let Ok(dir) = std::env::var("NATIVES_TEST_SCRATCH") {
            let evidence = serde_json::json!({
                "run_id": run.id,
                "status_after_cancel": cancelled.status.as_str(),
                "cancel_mid_run": true,
                "engine_was_registered": true,
                "engine_cancel_flag_observed_by_tool": engine_cancelled,
                "cancel_lifecycle_event": has_cancel_lifecycle,
            });
            let _ = std::fs::write(
                std::path::Path::new(&dir).join("daemon-cancel-mid.json"),
                serde_json::to_string_pretty(&evidence).unwrap_or_default(),
            );
        }
        assert!(
            has_cancel_lifecycle,
            "cancel must append Cancelled/Interrupted lifecycle event via commit_transition"
        );
        // Tool path should observe cancel flag from request_cancel.
        assert!(
            engine_cancelled,
            "production cancel must set engine cancel flag observed by tool execution"
        );
        // keep FIXTURE=1 for parallel tests under cfg(test)
        if let Some(v) = prev_runtime_dir {
            std::env::set_var("NATIVES_RUNTIME_DIR", v);
        } else {
            std::env::remove_var("NATIVES_RUNTIME_DIR");
        }
        crate::storage::set_test_db_override(None, None);
        let _ = std::fs::remove_dir_all(runtime_dir);
    }

    #[tokio::test]
    async fn cancel_vs_complete_race_single_terminal_and_consistent() {
        // 100 concurrent cancel-vs-complete races: each run ends with exactly one
        // terminal lifecycle event, and memory status matches that event.
        // T01: hermetic memory RunManagers — no ~/.natives, no leaked override.
        let _env_guard = crate::storage::DataStore::env_test_lock();
        let _env_restore = crate::storage::EnvRestore::capture();
        crate::storage::set_test_db_override(None, None);
        std::env::remove_var("NATIVES_ASSISTANT_DB_PATH");
        std::env::remove_var("NATIVES_DB_PATH");
        let rt_dir = tempfile::tempdir().unwrap();
        std::env::set_var("NATIVES_RUNTIME_DIR", rt_dir.path());
        std::env::set_var("NATIVES_RUN_MANAGER_MEMORY", "1");
        std::env::set_var("NATIVES_DAEMON_FIXTURE", "1");
        let mut terminal_mismatch = 0u32;
        let mut multi_terminal = 0u32;
        for i in 0..100 {
            let rm = Arc::new(RunManager::new());
            let run = rm
                .create_run(CreateRunRequest {
                    capability_selection: None,
                    disabled_tools: None,
                    conversation_id: format!("c-race-{i}"),
                    provider_id: "openai".into(),
                    model_id: "gpt-4o".into(),
                    key_id: Some("k".into()),
                    agent_profile_id: None,
                    permission_profile: Some("ask".into()),
                    content: Some("race".into()),
                    attachments: None,
                    max_steps: Some(3),
                    parent_run_id: None,
                    project_path: None,
                    idempotency_key: Some(format!("race-key-{i}")),
                    effort: None,
                    runtime_id: None,
                })
                .unwrap();
            let run_id = run.id.clone();

            let rm_start = rm.clone();
            let rid = run_id.clone();
            let start_h = tokio::spawn(async move {
                rm_start
                    .start(StartRunRequest {
                        agent_profile_id: None,
                        capability_selection: None,
                        run_id: Some(rid),
                        conversation_id: None,
                        provider_id: None,
                        model_id: None,
                        key_id: None,
                        content: Some("race".into()),
                        attachments: None,
                        trigger_message_id: None,
                        permission_profile: Some("full_access".into()),
                        max_steps: Some(3),
                        project_path: None,
                        idempotency_key: None,
                        effort: None,
                        runtime_id: None,
                    })
                    .await
            });
            // Small jitter so cancel sometimes wins, sometimes loses.
            if i % 2 == 0 {
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            }
            let rm_cancel = rm.clone();
            let rid2 = run_id.clone();
            let cancel_h =
                tokio::spawn(
                    async move { rm_cancel.cancel(CancelRunRequest { run_id: rid2 }).await },
                );

            let start_res = start_h.await.unwrap();
            let cancel_res = cancel_h.await.unwrap();
            // Both may succeed or one may see already-terminal — both ok.
            let _ = (start_res, cancel_res);

            let final_run = rm.get_run(&run_id).expect("run present");
            assert!(
                final_run.status.is_terminal(),
                "run {i} must be terminal, got {:?}",
                final_run.status
            );

            let events = rm.runtime.events.replay_after(&run_id, 0);
            let terminals: Vec<_> = events
                .iter()
                .filter(|e| {
                    matches!(
                        e.payload,
                        RunEventKind::Completed { .. }
                            | RunEventKind::Failed { .. }
                            | RunEventKind::Cancelled { .. }
                            | RunEventKind::Interrupted { .. }
                    )
                })
                .collect();
            if terminals.len() != 1 {
                multi_terminal += 1;
            }
            if let Some(ev) = terminals.first() {
                let status_from_event = match &ev.payload {
                    RunEventKind::Completed { .. } => RunStatusV2::Completed,
                    RunEventKind::Failed { .. } => RunStatusV2::Failed,
                    RunEventKind::Cancelled { .. } => RunStatusV2::Cancelled,
                    RunEventKind::Interrupted { .. } => RunStatusV2::Interrupted,
                    _ => unreachable!(),
                };
                if status_from_event != final_run.status {
                    terminal_mismatch += 1;
                }
            } else {
                terminal_mismatch += 1;
            }

            // Late outcome / cancel must be idempotent (no second terminal).
            let _ = rm.commit_outcome(&run_id, &EngineOutcome::completed("late"));
            let _ = rm
                .cancel(CancelRunRequest {
                    run_id: run_id.clone(),
                })
                .await;
            let after = rm.runtime.events.replay_after(&run_id, 0);
            let after_term = after
                .iter()
                .filter(|e| {
                    matches!(
                        e.payload,
                        RunEventKind::Completed { .. }
                            | RunEventKind::Failed { .. }
                            | RunEventKind::Cancelled { .. }
                            | RunEventKind::Interrupted { .. }
                    )
                })
                .count();
            assert_eq!(
                after_term,
                terminals.len().max(1),
                "late commit/cancel must not add a second terminal (run {i})"
            );
            let reopened = rm.get_run(&run_id).unwrap();
            assert_eq!(
                reopened.status, final_run.status,
                "reopen status must match final (run {i})"
            );
        }
        assert_eq!(
            multi_terminal, 0,
            "some runs had != 1 terminal lifecycle event"
        );
        assert_eq!(terminal_mismatch, 0, "some runs had event/status mismatch");
    }

    #[test]
    fn fail_run_if_active_is_idempotent_and_emits_failed_event() {
        let _prev_a = std::env::var("NATIVES_ASSISTANT_DB_PATH").ok();
        let _prev_d = std::env::var("NATIVES_DB_PATH").ok();
        std::env::remove_var("NATIVES_ASSISTANT_DB_PATH");
        std::env::remove_var("NATIVES_DB_PATH");
        // Hermetic: force in-memory RunManager so a leaked parallel-test
        // thread-local test_db_override cannot route us into a real store.
        let _prev_mem = std::env::var("NATIVES_RUN_MANAGER_MEMORY").ok();
        std::env::set_var("NATIVES_RUN_MANAGER_MEMORY", "1");
        let rm = Arc::new(RunManager::new());
        let run = rm
            .create_run(CreateRunRequest {
                capability_selection: None,
                disabled_tools: None,
                conversation_id: "c-fail".into(),
                provider_id: "missing-provider".into(),
                model_id: "m".into(),
                key_id: None,
                agent_profile_id: None,
                permission_profile: Some("ask".into()),
                content: Some("hi".into()),
                attachments: None,
                max_steps: Some(3),
                parent_run_id: None,
                project_path: Some("/tmp".into()),
                idempotency_key: None,
                effort: None,
                runtime_id: Some("native".into()),
            })
            .unwrap();
        {
            let mut runs = rm.runs.lock().unwrap();
            if let Some(r) = runs.get_mut(&run.id) {
                r.status = RunStatusV2::Preparing;
            }
        }
        rm.fail_run_if_active(&run.id, "No credentials for provider", "NO_CREDENTIALS");
        let after = rm.get_run(&run.id).unwrap();
        assert_eq!(after.status, RunStatusV2::Failed);
        assert_eq!(after.error_code.as_deref(), Some("NO_CREDENTIALS"));
        assert!(after.finished_at.is_some());
        let events = rm.runtime.events.replay_after(&run.id, 0);
        assert!(
            events
                .iter()
                .any(|e| matches!(&e.payload, RunEventKind::Failed { code, .. } if code == "NO_CREDENTIALS")),
            "expected failed event, got {:?}",
            events.iter().map(|e| format!("{:?}", e.payload)).collect::<Vec<_>>()
        );
        // Second call is a no-op (already terminal).
        rm.fail_run_if_active(&run.id, "again", "NO_CREDENTIALS");
        let events2 = rm.runtime.events.replay_after(&run.id, 0);
        let failed_count = events2
            .iter()
            .filter(|e| matches!(&e.payload, RunEventKind::Failed { .. }))
            .count();
        assert_eq!(failed_count, 1);
        if let Some(v) = _prev_mem {
            std::env::set_var("NATIVES_RUN_MANAGER_MEMORY", v);
        } else {
            std::env::remove_var("NATIVES_RUN_MANAGER_MEMORY");
        }
    }
