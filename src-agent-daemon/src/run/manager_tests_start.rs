use super::*;

    #[test]
    fn start_cleans_auto_trigger_message_when_run_create_fails() {
        with_env_lock(|| {
            let dir = tempfile::tempdir().unwrap();
            let previous_db = std::env::var("NATIVES_DB_PATH").ok();
            let previous_runtime = std::env::var("NATIVES_RUNTIME_DIR").ok();
            let db_path = dir.path().join("natives.db");
            std::env::set_var("NATIVES_DB_PATH", &db_path);
            std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db_path);
            std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());

            std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());
            crate::storage::set_test_db_override(
                Some(db_path.clone()),
                Some(dir.path().join("artifacts")),
            );
            let store = Arc::new(
                crate::storage::DataStore::new(&db_path, &dir.path().join("artifacts")).unwrap(),
            );
            store
                .conn()
                .unwrap()
                .execute(
                    "INSERT INTO conversation (id, mode, title, provider_id, model_id)
                 VALUES ('trigger-clean-conv', 'agent', 'Trigger Clean', 'openai', 'gpt-4o')",
                    [],
                )
                .unwrap();
            // T01: keep the real migrated schema; inject persistence failure with a
            // trigger so recovery (SELECT on run_event) still works while INSERT fails.
            store
                .conn()
                .unwrap()
                .execute(
                    "CREATE TRIGGER fail_run_event_insert BEFORE INSERT ON run_event
                     BEGIN SELECT RAISE(FAIL, 'injected run_event insert'); END",
                    [],
                )
                .unwrap();

            let rm = RunManager::new_with_store(store.clone());
            let err = rm
                .ensure_run_for_start(&StartRunRequest {
                    agent_profile_id: None,
                    capability_selection: None,
                    run_id: None,
                    conversation_id: Some("trigger-clean-conv".into()),
                    provider_id: Some("openai".into()),
                    model_id: Some("gpt-4o".into()),
                    key_id: None,
                    content: Some("do not leave me".into()),
                    attachments: None,
                    trigger_message_id: None,
                    permission_profile: None,
                    max_steps: None,
                    project_path: None,
                    idempotency_key: Some(format!("trigger-clean-{}", Uuid::new_v4())),
                    effort: None,
                    runtime_id: None,
                })
                .unwrap_err();
            assert!(err.contains("PERSISTENCE_FAILED"), "{err}");
            let messages: i64 = store
                .conn()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM message WHERE conversation_id = 'trigger-clean-conv'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(messages, 0);

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
        });
    }

    #[test]
    fn start_without_run_id_appends_trigger_message_in_daemon_store() {
        with_env_lock(|| {
            let dir = tempfile::tempdir().unwrap();
            let previous_db = std::env::var("NATIVES_DB_PATH").ok();
            let previous_runtime = std::env::var("NATIVES_RUNTIME_DIR").ok();
            let db_path = dir.path().join("natives.db");
            std::env::set_var("NATIVES_DB_PATH", &db_path);
            std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db_path);
            std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());

            std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());
            crate::storage::set_test_db_override(
                Some(db_path.clone()),
                Some(dir.path().join("artifacts")),
            );
            let store =
                crate::storage::DataStore::new(&db_path, &dir.path().join("artifacts")).unwrap();
            let conversation_id = "trigger-conversation";
            store.conn().unwrap().execute(
                "INSERT INTO conversation (id, mode, title, provider_id, model_id, permission_profile_id)
                 VALUES (?1, 'agent', 'Trigger', 'openai', 'gpt-4o', 'readonly')",
                rusqlite::params![conversation_id],
            ).unwrap();
            let rm = RunManager::new();
            let run = rm
                .ensure_run_for_start(&StartRunRequest {
                    agent_profile_id: None,
                    capability_selection: None,
                    run_id: None,
                    conversation_id: Some(conversation_id.to_string()),
                    provider_id: Some("openai".into()),
                    model_id: Some("gpt-4o".into()),
                    key_id: None,
                    content: Some("inspect".into()),
                    attachments: Some(vec![assistant_protocol::v2::AttachmentRef {
                        path: "/tmp/a.txt".into(),
                        name: Some("a.txt".into()),
                        mime_type: Some("text/plain".into()),
                        size: Some(3),
                    }]),
                    trigger_message_id: None,
                    permission_profile: None,
                    max_steps: None,
                    project_path: None,
                    idempotency_key: Some("trigger-idem".into()),
                    effort: None,
                    runtime_id: None,
                })
                .unwrap();
            assert_eq!(run.permission_profile, "readonly");
            assert!(run.trigger_message_id.is_some());
            let trigger_message_id = run.trigger_message_id.as_deref().unwrap();

            let text: String = store.conn().unwrap().query_row(
                "SELECT block_json FROM message_block WHERE message_id = ?1 AND block_type = 'text'",
                rusqlite::params![trigger_message_id],
                |row| row.get(0),
            ).unwrap();
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&text).unwrap()["text"],
                "inspect"
            );
            let file: String = store.conn().unwrap().query_row(
                "SELECT block_json FROM message_block WHERE message_id = ?1 AND block_type = 'file_reference'",
                rusqlite::params![trigger_message_id],
                |row| row.get(0),
            ).unwrap();
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&file).unwrap()["path"],
                "/tmp/a.txt"
            );

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
        });
    }

    #[test]
    fn start_with_seams_loads_daemon_conversation_history() {
        with_env_lock(|| {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let dir = tempfile::tempdir().unwrap();
                let previous_db = std::env::var("NATIVES_DB_PATH").ok();
                let previous_runtime = std::env::var("NATIVES_RUNTIME_DIR").ok();
                let db_path = dir.path().join("natives.db");
                std::env::set_var("NATIVES_DB_PATH", &db_path);
                std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db_path);
                std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());

                std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());
                crate::storage::set_test_db_override(Some(db_path.clone()), Some(dir.path().join("artifacts")));
                let store = Arc::new(
                    crate::storage::DataStore::new(&db_path, &dir.path().join("artifacts"))
                        .unwrap(),
                );
                store.conn().unwrap().execute(
                    "INSERT INTO conversation (id, mode, title, provider_id, model_id, permission_profile_id)
                     VALUES ('history-conv', 'agent', 'History', 'openai', 'gpt-4o', 'readonly')",
                    [],
                ).unwrap();
                crate::conversation_store::append_trigger_message(
                    "history-conv",
                    Some("remember alpha"),
                    None,
                ).unwrap();
                crate::conversation_store::append_trigger_message(
                    "history-conv",
                    Some("now beta"),
                    None,
                ).unwrap();

                struct EmptyTools;
                #[async_trait::async_trait]
                impl agent_core::EngineToolRuntime for EmptyTools {
                    async fn list_tool_schemas(&self) -> Vec<agent_core::ToolSchema> {
                        Vec::new()
                    }
                    async fn execute_tool(
                        &self,
                        _name: &str,
                        _input: serde_json::Value,
                        _cancel: &CancellationToken,
                    ) -> agent_core::ToolExecutionResult {
                        agent_core::ToolExecutionResult {
                            output: serde_json::json!({}),
                            is_error: false,
                            duration_ms: 0,
                        }
                    }
                }
                struct CaptureProvider(std::sync::Arc<StdMutex<Vec<agent_core::EngineMessage>>>);
                #[async_trait::async_trait]
                impl agent_core::EngineProvider for CaptureProvider {
                    async fn stream(
                        &self,
                        _model: &str,
                        messages: Vec<agent_core::EngineMessage>,
                        _tools: &[agent_core::ToolSchema],
                        _system_prompt: Option<&str>,
                        _cancel: CancellationToken,
                    ) -> Result<agent_core::EngineProviderEventStream, agent_core::EngineError>
                    {
                        *self.0.lock().unwrap() = messages;
                        Ok(Box::pin(futures_util::stream::iter(vec![
                            agent_core::EngineProviderEvent::TextDelta("ok".into()),
                            agent_core::EngineProviderEvent::Completed,
                        ])))
                    }
                }

                let seen = std::sync::Arc::new(StdMutex::new(Vec::new()));
                let provider = CaptureProvider(seen.clone());
                let rm = RunManager::new_with_store(store.clone());
                let run = rm.ensure_run_for_start(&StartRunRequest {
            agent_profile_id: None,
            capability_selection: None,
                    run_id: None,
                    conversation_id: Some("history-conv".into()),
                    provider_id: Some("openai".into()),
                    model_id: Some("gpt-4o".into()),
                    key_id: None,
                    content: None,
                    attachments: None,
                    trigger_message_id: None,
                    permission_profile: None,
                    max_steps: Some(3),
                    project_path: None,
                    idempotency_key: Some("history-run".into()),
                            effort: None,
            runtime_id: None,
        }).unwrap();
                let run_id = run.id.clone();
                rm.start_with_seams(
                    StartRunRequest {
            agent_profile_id: None,
            capability_selection: None,
                        run_id: Some(run_id.clone()),
                        conversation_id: None,
                        provider_id: None,
                        model_id: None,
                        key_id: None,
                        content: None,
                        attachments: None,
                        trigger_message_id: None,
                        permission_profile: None,
                        max_steps: Some(3),
                        project_path: None,
                        idempotency_key: None,
                                effort: None,
            runtime_id: None,
        },
                    &provider,
                    &EmptyTools,
                ).await.unwrap();
                let seen = seen.lock().unwrap();
                assert_eq!(seen.len(), 2);
                assert_eq!(seen[0].content, "remember alpha");
                assert_eq!(seen[1].content, "now beta");
                let reply: String = store.conn().unwrap().query_row(
                    "SELECT block_json
                     FROM message_block
                     WHERE block_type = 'text'
                       AND message_id IN (SELECT id FROM message WHERE conversation_id = 'history-conv' AND role = 'assistant')",
                    [],
                    |row| row.get(0),
                ).unwrap();
                assert_eq!(
                    serde_json::from_str::<serde_json::Value>(&reply).unwrap()["text"],
                    "ok"
                );
                let linked_run: String = store.conn().unwrap().query_row(
                    "SELECT block_json
                     FROM message_block
                     WHERE block_type = 'run_reference'
                       AND message_id IN (SELECT id FROM message WHERE conversation_id = 'history-conv' AND role = 'assistant')",
                    [],
                    |row| row.get(0),
                ).unwrap();
                assert_eq!(
                    serde_json::from_str::<serde_json::Value>(&linked_run).unwrap()["run_id"],
                    run_id
                );

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
            });
        });
    }

    #[tokio::test]
    async fn start_detached_returns_preparing_before_terminal() {
        std::env::set_var("NATIVES_DAEMON_FIXTURE", "1");
        let dir = tempfile::tempdir().unwrap();
        let previous_db = std::env::var("NATIVES_DB_PATH").ok();
        let previous_runtime = std::env::var("NATIVES_RUNTIME_DIR").ok();
        let db_path = dir.path().join("natives.db");
        std::env::set_var("NATIVES_DB_PATH", &db_path);
        std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db_path);
        std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());
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
             VALUES ('c-detach', 'agent', 'Detached', 'openai', 'gpt-4o', 'full_access')",
            [],
        ).unwrap();
        let rm = Arc::new(RunManager::new_with_store(store.clone()));
        let created = rm
            .create_run(CreateRunRequest {
                capability_selection: None,
                disabled_tools: None,
                conversation_id: "c-detach".into(),
                provider_id: "openai".into(),
                model_id: "gpt-4o".into(),
                key_id: Some("k".into()),
                agent_profile_id: None,
                permission_profile: Some("full_access".into()),
                content: Some("detach me".into()),
                attachments: None,
                max_steps: Some(5),
                parent_run_id: None,
                project_path: Some(dir.path().to_string_lossy().into_owned()),
                idempotency_key: Some("detach-1".into()),
                effort: None,
                runtime_id: None,
            })
            .unwrap();

        let immediate = rm
            .start_detached(StartRunRequest {
                agent_profile_id: None,
                capability_selection: None,
                run_id: Some(created.id.clone()),
                conversation_id: None,
                provider_id: Some("openai".into()),
                model_id: Some("gpt-4o".into()),
                key_id: Some("k".into()),
                content: Some("detach me".into()),
                attachments: None,
                trigger_message_id: None,
                permission_profile: Some("full_access".into()),
                max_steps: Some(5),
                project_path: Some(dir.path().to_string_lossy().into_owned()),
                idempotency_key: None,
                effort: None,
                runtime_id: None,
            })
            .unwrap();
        // Must not wait for engine terminal status.
        assert!(
            !immediate.status.is_terminal(),
            "detached start must return before terminal; got {:?}",
            immediate.status
        );
        assert_eq!(immediate.status, RunStatusV2::Preparing);

        // Background task eventually completes fixture engine.
        let mut terminal = None;
        for _ in 0..100 {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            if let Some(r) = rm.get_run(&created.id) {
                if r.status.is_terminal() {
                    terminal = Some(r);
                    break;
                }
            }
        }
        let done = terminal.expect("detached run should reach terminal status");
        assert_eq!(
            done.status,
            RunStatusV2::Completed,
            "error_code={:?} id={}",
            done.error_code,
            done.id
        );
        // Terminal re-start must fail closed (use retry).
        let err = rm
            .start_detached(StartRunRequest {
                agent_profile_id: None,
                capability_selection: None,
                run_id: Some(created.id.clone()),
                conversation_id: None,
                provider_id: None,
                model_id: None,
                key_id: None,
                content: None,
                attachments: None,
                trigger_message_id: None,
                permission_profile: None,
                max_steps: None,
                project_path: None,
                idempotency_key: None,
                effort: None,
                runtime_id: None,
            })
            .unwrap_err();
        assert!(
            err.contains("terminal") || err.contains("retry"),
            "unexpected: {err}"
        );
        if let Some(value) = previous_db {
            std::env::set_var("NATIVES_DB_PATH", &value);
            std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &value);
        } else {
            std::env::remove_var("NATIVES_DB_PATH");
            std::env::remove_var("NATIVES_ASSISTANT_DB_PATH");
        }
        if let Some(value) = previous_runtime {
            std::env::set_var("NATIVES_RUNTIME_DIR", &value);
        } else {
            std::env::remove_var("NATIVES_RUNTIME_DIR");
        }
        // keep FIXTURE=1 for parallel tests under cfg(test)
    }

    #[tokio::test]
    async fn duplicate_start_detached_is_idempotent_while_active() {
        with_env_lock(|| {
            std::env::set_var("NATIVES_DAEMON_FIXTURE", "1");
            let dir = tempfile::tempdir().unwrap();
            let db_path = dir.path().join("natives.db");
            std::env::set_var("NATIVES_DB_PATH", &db_path);
            std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db_path);
            std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());
            std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());
            crate::storage::set_test_db_override(
                Some(db_path.clone()),
                Some(dir.path().join("artifacts")),
            );
            let store = Arc::new(
                crate::storage::DataStore::new(&db_path, &dir.path().join("artifacts")).unwrap(),
            );
            store
                .conn()
                .unwrap()
                .execute(
                    "INSERT INTO conversation (id, mode, title, provider_id, model_id)
                 VALUES ('c-idem-start', 'agent', 'Idem', 'openai', 'gpt-4o')",
                    [],
                )
                .unwrap();
            let rm = Arc::new(RunManager::new_with_store(store.clone()));
            let created = rm
                .create_run(CreateRunRequest {
                    capability_selection: None,
                    disabled_tools: None,
                    conversation_id: "c-idem-start".into(),
                    provider_id: "openai".into(),
                    model_id: "gpt-4o".into(),
                    key_id: Some("k".into()),
                    agent_profile_id: None,
                    permission_profile: Some("full_access".into()),
                    content: Some("once".into()),
                    attachments: None,
                    max_steps: Some(5),
                    parent_run_id: None,
                    project_path: None,
                    idempotency_key: Some(format!("idem-start-{}", uuid::Uuid::new_v4())),
                    effort: None,
                    runtime_id: None,
                })
                .unwrap();
            let req = StartRunRequest {
                agent_profile_id: None,
                capability_selection: None,
                run_id: Some(created.id.clone()),
                conversation_id: None,
                provider_id: Some("openai".into()),
                model_id: Some("gpt-4o".into()),
                key_id: Some("k".into()),
                content: Some("once".into()),
                attachments: None,
                trigger_message_id: None,
                permission_profile: Some("full_access".into()),
                max_steps: Some(5),
                project_path: None,
                idempotency_key: None,
                effort: None,
                runtime_id: None,
            };
            let a = rm.start_detached(req.clone()).unwrap();
            let b = rm.start_detached(req).unwrap();
            assert_eq!(a.id, b.id);
            // Second call must not error; active run is returned as-is.
            assert!(
                a.status.is_active() || a.status.is_terminal() || a.status == RunStatusV2::Queued
            );
            // keep FIXTURE=1 for parallel tests under cfg(test)
        });
    }

    #[test]
    fn start_detached_codex_runtime_is_fail_closed() {
        let _prev_a = std::env::var("NATIVES_ASSISTANT_DB_PATH").ok();
        let _prev_d = std::env::var("NATIVES_DB_PATH").ok();
        std::env::remove_var("NATIVES_ASSISTANT_DB_PATH");
        std::env::remove_var("NATIVES_DB_PATH");
        // Gate runs before ensure/create — use existing run_id to avoid FK on new conversation.
        let rm = Arc::new(RunManager::new());
        let run = rm
            .create_run(CreateRunRequest {
                capability_selection: None,
                disabled_tools: None,
                conversation_id: "c-codex".into(),
                provider_id: "openai".into(),
                model_id: "gpt".into(),
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
        let err = rm
            .start_detached(StartRunRequest {
                agent_profile_id: None,
                capability_selection: None,
                run_id: Some(run.id),
                conversation_id: Some("c-codex".into()),
                provider_id: Some("openai".into()),
                model_id: Some("gpt".into()),
                key_id: None,
                content: Some("hi".into()),
                attachments: None,
                trigger_message_id: None,
                permission_profile: Some("ask".into()),
                max_steps: Some(3),
                project_path: Some("/tmp".into()),
                idempotency_key: None,
                effort: None,
                runtime_id: Some("codex_cli".into()),
            })
            .unwrap_err();
        assert!(
            err.contains("codex_cli") && err.contains("unavailable"),
            "unexpected: {err}"
        );
    }

    #[test]
    fn start_detached_unknown_runtime_is_fail_closed() {
        let _prev_a = std::env::var("NATIVES_ASSISTANT_DB_PATH").ok();
        let _prev_d = std::env::var("NATIVES_DB_PATH").ok();
        std::env::remove_var("NATIVES_ASSISTANT_DB_PATH");
        std::env::remove_var("NATIVES_DB_PATH");
        let rm = Arc::new(RunManager::new());
        let run = rm
            .create_run(CreateRunRequest {
                capability_selection: None,
                disabled_tools: None,
                conversation_id: "c-rt".into(),
                provider_id: "openai".into(),
                model_id: "gpt".into(),
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
        let err = rm
            .start_detached(StartRunRequest {
                agent_profile_id: None,
                capability_selection: None,
                run_id: Some(run.id),
                conversation_id: Some("c-rt".into()),
                provider_id: Some("openai".into()),
                model_id: Some("gpt".into()),
                key_id: None,
                content: Some("hi".into()),
                attachments: None,
                trigger_message_id: None,
                permission_profile: Some("ask".into()),
                max_steps: Some(3),
                project_path: Some("/tmp".into()),
                idempotency_key: None,
                effort: None,
                runtime_id: Some("not_a_runtime".into()),
            })
            .unwrap_err();
        assert!(err.contains("unknown runtime_id"), "unexpected: {err}");
    }

    #[test]
    fn ensure_run_for_start_with_run_id_appends_daemon_local_user_message() {
        with_env_lock(|| {
            let dir = tempfile::tempdir().unwrap();
            let previous_db = std::env::var("NATIVES_DB_PATH").ok();
            let previous_runtime = std::env::var("NATIVES_RUNTIME_DIR").ok();
            let db_path = dir.path().join("natives.db");
            std::env::set_var("NATIVES_DB_PATH", &db_path);
            std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db_path);
            std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());

            std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());
            crate::storage::set_test_db_override(
                Some(db_path.clone()),
                Some(dir.path().join("artifacts")),
            );
            let store = Arc::new(
                crate::storage::DataStore::new(&db_path, &dir.path().join("artifacts")).unwrap(),
            );
            store
                .conn()
                .unwrap()
                .execute(
                    "INSERT INTO conversation (id, mode, title, provider_id, model_id)
                     VALUES ('host-conv', 'agent', 'Host', 'openai', 'gpt-4o')",
                    [],
                )
                .unwrap();
            let rm = RunManager::new_with_store(store.clone());
            let run = rm
                .create_run(CreateRunRequest {
                    capability_selection: None,
                    disabled_tools: None,
                    conversation_id: "host-conv".into(),
                    provider_id: "openai".into(),
                    model_id: "gpt-4o".into(),
                    key_id: None,
                    agent_profile_id: None,
                    permission_profile: Some("ask".into()),
                    content: Some("second question".into()),
                    attachments: None,
                    max_steps: Some(3),
                    parent_run_id: None,
                    project_path: Some("/tmp".into()),
                    idempotency_key: Some(format!("host-key-{}", Uuid::new_v4())),
                    effort: None,
                    runtime_id: Some("native".into()),
                })
                .unwrap();
            // Host path: run_id present, trigger_message_id is host UUID (ignored).
            let ensured = rm
                .ensure_run_for_start(&StartRunRequest {
                    agent_profile_id: None,
                    capability_selection: None,
                    run_id: Some(run.id.clone()),
                    conversation_id: Some("host-conv".into()),
                    provider_id: Some("openai".into()),
                    model_id: Some("gpt-4o".into()),
                    key_id: None,
                    content: Some("second question".into()),
                    attachments: None,
                    trigger_message_id: Some("host-message-uuid-not-in-daemon".into()),
                    permission_profile: Some("ask".into()),
                    max_steps: Some(3),
                    project_path: Some("/tmp".into()),
                    idempotency_key: None,
                    effort: None,
                    runtime_id: Some("native".into()),
                })
                .unwrap();
            assert!(ensured.trigger_message_id.is_some());
            let daemon_msg_id = ensured.trigger_message_id.unwrap();
            assert_ne!(daemon_msg_id, "host-message-uuid-not-in-daemon");
            let text: String = store
                .conn()
                .unwrap()
                .query_row(
                    "SELECT block_json FROM message_block WHERE message_id = ?1 LIMIT 1",
                    rusqlite::params![daemon_msg_id],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(text.contains("second question"), "{text}");

            // Idempotent: second ensure does not double-append.
            let again = rm
                .ensure_run_for_start(&StartRunRequest {
                    agent_profile_id: None,
                    capability_selection: None,
                    run_id: Some(run.id.clone()),
                    conversation_id: Some("host-conv".into()),
                    provider_id: Some("openai".into()),
                    model_id: Some("gpt-4o".into()),
                    key_id: None,
                    content: Some("second question".into()),
                    attachments: None,
                    trigger_message_id: None,
                    permission_profile: Some("ask".into()),
                    max_steps: Some(3),
                    project_path: Some("/tmp".into()),
                    idempotency_key: None,
                    effort: None,
                    runtime_id: Some("native".into()),
                })
                .unwrap();
            assert_eq!(
                again.trigger_message_id.as_deref(),
                Some(daemon_msg_id.as_str())
            );
            let count: i64 = store
                .conn()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM message WHERE conversation_id = 'host-conv' AND role = 'user'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 1);

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
        });
    }

    /// TASK-013: the native runtime is the daemon's executable authority and
    /// the CLI bridge, when present, is never advertised as a native authority.
    #[test]
    fn lineage_compat_capabilities_mark_cli_as_non_native() {
        let caps = RunManager::capabilities();
        let native = caps
            .runtimes
            .iter()
            .find(|r| r.id == "native")
            .expect("native runtime is always advertised");
        assert_eq!(
            native.status,
            assistant_protocol::v2::RuntimeAvailability::Executable
        );
        if crate::cli_runtime_bridge::claude_cli_available() {
            let cli = caps
                .runtimes
                .iter()
                .find(|r| r.id == "cli")
                .expect("CLI runtime is advertised when the CLI is available");
            assert_ne!(
                cli.status,
                assistant_protocol::v2::RuntimeAvailability::Executable,
                "the CLI must never claim native execution authority"
            );
        }
    }
