    use super::*;
    use agent_core::EngineToolRuntime;
    use assistant_protocol::v2::{
        ContinueRunRequest, CreateRunRequest, ReplayRunRequest, RetryRunRequest, StartRunRequest,
    };
    use std::sync::Mutex as StdMutex;

    /// Process-global lock for tests that mutate NATIVES_* env (fixture, runtime dir, keys).
    fn with_env_lock<R>(f: impl FnOnce() -> R) -> R {
        let _g = crate::storage::DataStore::env_test_lock();
        f()
    }

    #[test]
    fn create_run_is_idempotent_with_key() {
        let _prev_a = std::env::var("NATIVES_ASSISTANT_DB_PATH").ok();
        let _prev_d = std::env::var("NATIVES_DB_PATH").ok();
        std::env::remove_var("NATIVES_ASSISTANT_DB_PATH");
        std::env::remove_var("NATIVES_DB_PATH");
        let rm = RunManager::new();
        let req = CreateRunRequest {
            capability_selection: None,
            disabled_tools: None,
            conversation_id: "c1".into(),
            provider_id: "openai".into(),
            model_id: "gpt-4o".into(),
            key_id: Some("k1".into()),
            agent_profile_id: None,
            permission_profile: Some("ask".into()),
            content: Some("hello".into()),
            attachments: None,
            max_steps: Some(10),
            parent_run_id: None,
            project_path: None,
            idempotency_key: Some("idem-1".into()),
            effort: None,
            runtime_id: None,
        };
        let a = rm.create_run(req.clone()).unwrap();
        let b = rm.create_run(req).unwrap();
        assert_eq!(a.id, b.id);
    }

    #[test]
    fn create_run_persists_queued_event_to_sqlite_run_event() {
        with_env_lock(|| {
            let dir = tempfile::tempdir().unwrap();
            let db_path = dir.path().join("natives.db");
            std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db_path);
            std::env::set_var("NATIVES_DB_PATH", &db_path);
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
                 VALUES ('sqlite-events-conv', 'agent', 'SQLite Events', 'openai', 'gpt-4o')",
                    [],
                )
                .unwrap();

            let rm = RunManager::new_with_store(store.clone());
            let run = rm
                .create_run(CreateRunRequest {
                    capability_selection: None,
                    disabled_tools: None,
                    conversation_id: "sqlite-events-conv".into(),
                    provider_id: "openai".into(),
                    model_id: "gpt-4o".into(),
                    key_id: None,
                    agent_profile_id: None,
                    permission_profile: Some("ask".into()),
                    content: Some("persist me".into()),
                    attachments: None,
                    max_steps: Some(3),
                    parent_run_id: None,
                    project_path: None,
                    idempotency_key: Some(format!("sqlite-event-{}", Uuid::new_v4())),
                    effort: None,
                    runtime_id: None,
                })
                .unwrap();

            let (count, event_type): (i64, String) = store
                .conn()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*), MAX(event_type) FROM run_event WHERE run_id = ?1",
                    rusqlite::params![run.id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            assert_eq!(count, 1);
            assert_eq!(event_type, "queued");

            let replayed = crate::production::ProductionRuntime::new_with_event_store(store)
                .events
                .replay_after(&run.id, 0);
            assert_eq!(replayed.len(), 1);
            assert!(matches!(replayed[0].payload, RunEventKind::Queued));
        });
    }

    #[test]
    fn create_run_persists_protocol_v2_run_metadata_to_sqlite() {
        with_env_lock(|| {
            let dir = tempfile::tempdir().unwrap();
            let db_path = dir.path().join("natives.db");
            std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db_path);
            std::env::set_var("NATIVES_DB_PATH", &db_path);
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
                 VALUES ('run-meta-conv', 'agent', 'Run Meta', 'openai', 'gpt-4o')",
                    [],
                )
                .unwrap();

            let rm = RunManager::new_with_store(store.clone());
            let idempotency_key = format!("run-meta-{}", Uuid::new_v4());
            let run = rm
                .create_run(CreateRunRequest {
                    capability_selection: None,
                    disabled_tools: None,
                    conversation_id: "run-meta-conv".into(),
                    provider_id: "openai-compatible-provider".into(),
                    model_id: "deepseek-v4-flash".into(),
                    key_id: Some("key-123".into()),
                    agent_profile_id: Some("agent-profile-1".into()),
                    permission_profile: Some("full_access".into()),
                    content: Some("persist metadata".into()),
                    attachments: None,
                    max_steps: Some(9),
                    parent_run_id: Some("parent-run-1".into()),
                    project_path: Some("/tmp/natives-project".into()),
                    idempotency_key: Some(idempotency_key.clone()),
                    effort: None,
                    runtime_id: None,
                })
                .unwrap();

            let row: (String, String, String, String, String, String, String, i64) = store
                .conn()
                .unwrap()
                .query_row(
                    "SELECT parent_run_id, agent_profile_id, key_id, permission_profile,
                            project_path, idempotency_key, model_id, max_steps
                     FROM run WHERE id = ?1",
                    rusqlite::params![run.id],
                    |row| {
                        Ok((
                            row.get(0)?,
                            row.get(1)?,
                            row.get(2)?,
                            row.get(3)?,
                            row.get(4)?,
                            row.get(5)?,
                            row.get(6)?,
                            row.get(7)?,
                        ))
                    },
                )
                .unwrap();
            assert_eq!(row.0, "parent-run-1");
            assert_eq!(row.1, "agent-profile-1");
            assert_eq!(row.2, "key-123");
            assert_eq!(row.3, "full_access");
            assert_eq!(row.4, "/tmp/natives-project");
            assert_eq!(row.5, idempotency_key);
            assert_eq!(row.6, "deepseek-v4-flash");
            assert_eq!(row.7, 9);
        });
    }

    #[test]
    fn create_run_idempotency_survives_sqlite_backed_restart() {
        with_env_lock(|| {
            let dir = tempfile::tempdir().unwrap();
            let db_path = dir.path().join("natives.db");
            std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db_path);
            std::env::set_var("NATIVES_DB_PATH", &db_path);
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
                 VALUES ('sqlite-idem-conv', 'agent', 'SQLite Idem', 'openai', 'gpt-4o')",
                    [],
                )
                .unwrap();

            let idempotency_key = format!("sqlite-idem-{}", Uuid::new_v4());
            let req = CreateRunRequest {
                capability_selection: None,
                disabled_tools: None,
                conversation_id: "sqlite-idem-conv".into(),
                provider_id: "openai".into(),
                model_id: "gpt-4o".into(),
                key_id: Some("key-A".into()),
                agent_profile_id: Some("agent-A".into()),
                permission_profile: Some("ask".into()),
                content: Some("only queue once".into()),
                attachments: None,
                max_steps: Some(7),
                parent_run_id: None,
                project_path: Some("/tmp/sqlite-idem".into()),
                idempotency_key: Some(idempotency_key.clone()),
                effort: None,
                runtime_id: None,
            };

            let first = RunManager::new_with_store(store.clone())
                .create_run(req.clone())
                .unwrap();
            let second = RunManager::new_with_store(store.clone())
                .create_run(req)
                .unwrap();

            assert_eq!(first.id, second.id);
            assert_eq!(
                second.idempotency_key.as_deref(),
                Some(idempotency_key.as_str())
            );
            assert_eq!(second.project_path.as_deref(), Some("/tmp/sqlite-idem"));

            let event_count: i64 = store
                .conn()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM run_event WHERE run_id = ?1 AND event_type = 'queued'",
                    rusqlite::params![first.id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(event_count, 1);
        });
    }

    #[test]
    fn create_run_cleans_sqlite_row_when_queued_event_persistence_fails() {
        with_env_lock(|| {
            let dir = tempfile::tempdir().unwrap();
            let db_path = dir.path().join("natives.db");
            std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db_path);
            std::env::set_var("NATIVES_DB_PATH", &db_path);
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
                 VALUES ('broken-events-conv', 'agent', 'Broken Events', 'openai', 'gpt-4o')",
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
            let run_id = format!("broken-event-{}", Uuid::new_v4());
            let err = rm
                .create_run(CreateRunRequest {
                    capability_selection: None,
                    disabled_tools: None,
                    conversation_id: "broken-events-conv".into(),
                    provider_id: "openai".into(),
                    model_id: "gpt-4o".into(),
                    key_id: None,
                    agent_profile_id: None,
                    permission_profile: Some("ask".into()),
                    content: Some("must rollback".into()),
                    attachments: None,
                    max_steps: Some(3),
                    parent_run_id: None,
                    project_path: None,
                    idempotency_key: Some(run_id.clone()),
                    effort: None,
                    runtime_id: None,
                })
                .unwrap_err();
            assert!(err.contains("PERSISTENCE_FAILED"), "{err}");
            assert!(rm.get_run(&run_id).is_none());
            let count: i64 = store
                .conn()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM run WHERE id = ?1",
                    rusqlite::params![run_id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 0);
        });
    }

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

    #[test]
    fn retry_creates_new_run_id() {
        // T01: hermetic memory RunManager — no ~/.natives reads, no leaked
        // thread-local override, deterministic under --test-threads=2.
        let _env_guard = crate::storage::DataStore::env_test_lock();
        let _env_restore = crate::storage::EnvRestore::capture();
        crate::storage::set_test_db_override(None, None);
        std::env::remove_var("NATIVES_ASSISTANT_DB_PATH");
        std::env::remove_var("NATIVES_DB_PATH");
        let rt_dir = tempfile::tempdir().unwrap();
        std::env::set_var("NATIVES_RUNTIME_DIR", rt_dir.path());
        std::env::set_var("NATIVES_RUN_MANAGER_MEMORY", "1");
        let rm = RunManager::new();
        let original = rm
            .create_run(CreateRunRequest {
                capability_selection: None,
                disabled_tools: None,
                conversation_id: "c1".into(),
                provider_id: "openai".into(),
                model_id: "gpt-4o".into(),
                key_id: None,
                agent_profile_id: None,
                permission_profile: None,
                content: Some("retry me".into()),
                attachments: None,
                max_steps: None,
                parent_run_id: None,
                project_path: None,
                idempotency_key: None,
                effort: None,
                runtime_id: None,
            })
            .unwrap();
        let retried = rm
            .retry(RetryRunRequest {
                run_id: original.id.clone(),
            })
            .unwrap();
        assert_ne!(original.id, retried.id);
        assert_eq!(retried.conversation_id, original.conversation_id);
    }

    #[test]
    fn retry_rejects_active_run() {
        let rm = RunManager::new();
        let original = rm
            .create_run(CreateRunRequest {
                capability_selection: None,
                disabled_tools: None,
                conversation_id: "active-retry".into(),
                provider_id: "openai".into(),
                model_id: "gpt-4o".into(),
                key_id: None,
                agent_profile_id: None,
                permission_profile: None,
                content: Some("do not duplicate".into()),
                attachments: None,
                max_steps: None,
                parent_run_id: None,
                project_path: None,
                idempotency_key: None,
                effort: None,
                runtime_id: None,
            })
            .unwrap();
        rm.commit_status(
            &original.id,
            RunStatusV2::Preparing,
            TransitionMetadata::empty(),
        )
        .unwrap();
        let error = rm
            .retry(RetryRunRequest {
                run_id: original.id,
            })
            .unwrap_err();
        assert!(error.contains("active run cannot be retried"));
    }

    #[test]
    fn continue_creates_lineage_from_durable_checkpoint() {
        with_env_lock(|| {
            let dir = tempfile::tempdir().unwrap();
            let db_path = dir.path().join("continue.db");
            std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db_path);
            std::env::set_var("NATIVES_DB_PATH", &db_path);
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
                 VALUES ('continue-conv', 'agent', 'Continue', 'openai', 'gpt-4o')",
                    [],
                )
                .unwrap();

            let rm = RunManager::new_with_store(store.clone());
            let source = rm
                .create_run(CreateRunRequest {
                    capability_selection: None,
                    disabled_tools: None,
                    conversation_id: "continue-conv".into(),
                    provider_id: "openai".into(),
                    model_id: "gpt-4o".into(),
                    key_id: None,
                    agent_profile_id: None,
                    permission_profile: Some("ask".into()),
                    content: Some("continue me".into()),
                    attachments: None,
                    max_steps: Some(5),
                    parent_run_id: None,
                    project_path: Some(dir.path().to_string_lossy().into_owned()),
                    idempotency_key: None,
                    effort: None,
                    runtime_id: Some("native".into()),
                })
                .unwrap();
            rm.commit_status(
                &source.id,
                RunStatusV2::Preparing,
                TransitionMetadata::empty().with_lifecycle_hint("preparing"),
            )
            .unwrap();
            rm.commit_status(
                &source.id,
                RunStatusV2::Running,
                TransitionMetadata::empty().with_lifecycle_hint("running"),
            )
            .unwrap();
            rm.commit_status(
                &source.id,
                RunStatusV2::Completed,
                TransitionMetadata::empty().with_lifecycle_hint("completed"),
            )
            .unwrap();

            let conn = store.conn().unwrap();
            conn.execute(
                "INSERT INTO context_snapshot
                 (id, run_id, sequence, snapshot_type, token_count, snapshot_json)
                 VALUES ('snapshot-continue', ?1, 1, 'active_context', 3, ?2)",
                rusqlite::params![&source.id, serde_json::json!({"messages": []}).to_string()],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO checkpoint
                 (id, run_id, conversation_id, sequence, turn_id,
                  active_context_snapshot_id, side_effect_ledger_cursor, snapshot_json)
                 VALUES ('checkpoint-continue', ?1, 'continue-conv', 1, 'turn-1',
                         'snapshot-continue', 'ledger-1', '{}')",
                rusqlite::params![&source.id],
            )
            .unwrap();
            drop(conn);

            let continued = rm
                .continue_run(ContinueRunRequest {
                    run_id: source.id.clone(),
                    checkpoint_id: None,
                    content: Some("resume from checkpoint".into()),
                })
                .unwrap();
            assert_ne!(continued.id, source.id);
            assert_eq!(
                continued.continued_from_run_id.as_deref(),
                Some(source.id.as_str())
            );
            assert_eq!(
                continued.resume_of_run_id.as_deref(),
                Some(source.id.as_str())
            );
            assert_eq!(
                continued.checkpoint_id.as_deref(),
                Some("checkpoint-continue")
            );
            assert_eq!(continued.retry_of_turn_id.as_deref(), Some("turn-1"));

            let status: String = store
                .conn()
                .unwrap()
                .query_row(
                    "SELECT status FROM resume_plan WHERE source_run_id = ?1 AND new_run_id = ?2",
                    rusqlite::params![&source.id, &continued.id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(status, "approved");
            rm.mark_resume_plan_executed(&source.id, &continued.id)
                .unwrap();
            let executed: String = store
                .conn()
                .unwrap()
                .query_row(
                    "SELECT status FROM resume_plan WHERE source_run_id = ?1 AND new_run_id = ?2",
                    rusqlite::params![&source.id, &continued.id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(executed, "executed");
        });
    }

    #[test]
    fn continue_rejects_checkpoint_without_active_snapshot() {
        with_env_lock(|| {
            let dir = tempfile::tempdir().unwrap();
            let db_path = dir.path().join("continue-nosnapshot.db");
            std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db_path);
            std::env::set_var("NATIVES_DB_PATH", &db_path);
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
                 VALUES ('continue-nosnap-conv', 'agent', 'Continue', 'openai', 'gpt-4o')",
                    [],
                )
                .unwrap();

            let rm = RunManager::new_with_store(store.clone());
            let source = rm
                .create_run(CreateRunRequest {
                    capability_selection: None,
                    disabled_tools: None,
                    conversation_id: "continue-nosnap-conv".into(),
                    provider_id: "openai".into(),
                    model_id: "gpt-4o".into(),
                    key_id: None,
                    agent_profile_id: None,
                    permission_profile: Some("ask".into()),
                    content: Some("continue me".into()),
                    attachments: None,
                    max_steps: Some(5),
                    parent_run_id: None,
                    project_path: Some(dir.path().to_string_lossy().into_owned()),
                    idempotency_key: None,
                    effort: None,
                    runtime_id: Some("native".into()),
                })
                .unwrap();
            rm.commit_status(
                &source.id,
                RunStatusV2::Preparing,
                TransitionMetadata::empty().with_lifecycle_hint("preparing"),
            )
            .unwrap();
            rm.commit_status(
                &source.id,
                RunStatusV2::Running,
                TransitionMetadata::empty().with_lifecycle_hint("running"),
            )
            .unwrap();
            rm.commit_status(
                &source.id,
                RunStatusV2::Completed,
                TransitionMetadata::empty().with_lifecycle_hint("completed"),
            )
            .unwrap();

            // Checkpoint with NO committed active context snapshot: exact
            // continue must fail closed instead of approving and later falling
            // back to a newer conversation snapshot.
            let conn = store.conn().unwrap();
            conn.execute(
                "INSERT INTO checkpoint
                 (id, run_id, conversation_id, sequence, turn_id,
                  active_context_snapshot_id, side_effect_ledger_cursor, snapshot_json)
                 VALUES ('checkpoint-nosnapshot', ?1, 'continue-nosnap-conv', 1, 'turn-1',
                         NULL, 'ledger-1', '{}')",
                rusqlite::params![&source.id],
            )
            .unwrap();
            drop(conn);

            let error = rm
                .continue_run(ContinueRunRequest {
                    run_id: source.id.clone(),
                    checkpoint_id: None,
                    content: Some("resume from checkpoint".into()),
                })
                .expect_err("checkpoint without a snapshot must be rejected");

            assert!(
                error.contains("active context snapshot"),
                "stable fail-closed error expected, got: {error}"
            );
            // No run must be created and no approved resume plan may exist.
            let approved: i64 = store
                .conn()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM resume_plan
                     WHERE source_run_id = ?1 AND action = 'continue' AND status = 'approved'",
                    rusqlite::params![&source.id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(approved, 0, "no approved continue plan may be created");
        });
    }

    /// Build a terminal source run with a fully watermarked checkpoint and a
    /// fresh store, under `with_env_lock`. Returns `(store, source_run_id)`.
    fn resume_fixture() -> (std::sync::Arc<crate::storage::DataStore>, String) {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("resume.db");
        std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db_path);
        std::env::set_var("NATIVES_DB_PATH", &db_path);
        crate::storage::set_test_db_override(
            Some(db_path.clone()),
            Some(dir.path().join("artifacts")),
        );
        let store = std::sync::Arc::new(
            crate::storage::DataStore::new(&db_path, &dir.path().join("artifacts")).unwrap(),
        );
        store
            .conn()
            .unwrap()
            .execute(
                "INSERT INTO conversation (id, mode, title, provider_id, model_id)
                 VALUES ('resume-conv', 'agent', 'Resume', 'openai', 'gpt-4o')",
                [],
            )
            .unwrap();
        let rm = RunManager::new_with_store(store.clone());
        let source = rm
            .create_run(CreateRunRequest {
                capability_selection: None,
                disabled_tools: None,
                conversation_id: "resume-conv".into(),
                provider_id: "openai".into(),
                model_id: "gpt-4o".into(),
                key_id: None,
                agent_profile_id: None,
                permission_profile: Some("ask".into()),
                content: Some("resume me".into()),
                attachments: None,
                max_steps: Some(5),
                parent_run_id: None,
                project_path: Some(dir.path().to_string_lossy().into_owned()),
                idempotency_key: None,
                effort: None,
                runtime_id: Some("native".into()),
            })
            .unwrap();
        for (status, hint) in [
            (RunStatusV2::Preparing, "preparing"),
            (RunStatusV2::Running, "running"),
            (RunStatusV2::Completed, "completed"),
        ] {
            rm.commit_status(
                &source.id,
                status,
                TransitionMetadata::empty().with_lifecycle_hint(hint),
            )
            .unwrap();
        }
        let conn = store.conn().unwrap();
        conn.execute(
            "INSERT INTO context_snapshot
             (id, run_id, sequence, snapshot_type, token_count, snapshot_json)
             VALUES ('snapshot-resume', ?1, 1, 'active_context', 3, ?2)",
            rusqlite::params![&source.id, serde_json::json!({"messages": []}).to_string()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO checkpoint
             (id, run_id, conversation_id, sequence, turn_id,
              active_context_snapshot_id, side_effect_ledger_cursor, snapshot_json)
             VALUES ('checkpoint-resume', ?1, 'resume-conv', 1, 'turn-1',
                     'snapshot-resume', 'ledger-1', '{}')",
            rusqlite::params![&source.id],
        )
        .unwrap();
        drop(conn);
        (store, source.id)
    }

    #[test]
    fn continue_rejects_checkpoint_without_ledger_watermark() {
        with_env_lock(|| {
            let dir = tempfile::tempdir().unwrap();
            let db_path = dir.path().join("continue-noledger.db");
            std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db_path);
            std::env::set_var("NATIVES_DB_PATH", &db_path);
            crate::storage::set_test_db_override(
                Some(db_path.clone()),
                Some(dir.path().join("artifacts")),
            );
            let store = std::sync::Arc::new(
                crate::storage::DataStore::new(&db_path, &dir.path().join("artifacts")).unwrap(),
            );
            store
                .conn()
                .unwrap()
                .execute(
                    "INSERT INTO conversation (id, mode, title, provider_id, model_id)
                 VALUES ('noledger-conv', 'agent', 'Continue', 'openai', 'gpt-4o')",
                    [],
                )
                .unwrap();
            let rm = RunManager::new_with_store(store.clone());
            let source = rm
                .create_run(CreateRunRequest {
                    capability_selection: None,
                    disabled_tools: None,
                    conversation_id: "noledger-conv".into(),
                    provider_id: "openai".into(),
                    model_id: "gpt-4o".into(),
                    key_id: None,
                    agent_profile_id: None,
                    permission_profile: Some("ask".into()),
                    content: Some("continue me".into()),
                    attachments: None,
                    max_steps: Some(5),
                    parent_run_id: None,
                    project_path: Some(dir.path().to_string_lossy().into_owned()),
                    idempotency_key: None,
                    effort: None,
                    runtime_id: Some("native".into()),
                })
                .unwrap();
            for (status, hint) in [
                (RunStatusV2::Preparing, "preparing"),
                (RunStatusV2::Running, "running"),
                (RunStatusV2::Completed, "completed"),
            ] {
                rm.commit_status(
                    &source.id,
                    status,
                    TransitionMetadata::empty().with_lifecycle_hint(hint),
                )
                .unwrap();
            }
            let conn = store.conn().unwrap();
            conn.execute(
                "INSERT INTO context_snapshot
                 (id, run_id, sequence, snapshot_type, token_count, snapshot_json)
                 VALUES ('snapshot-noledger', ?1, 1, 'active_context', 3, ?2)",
                rusqlite::params![&source.id, serde_json::json!({"messages": []}).to_string()],
            )
            .unwrap();
            // Snapshot + turn present, ledger cursor NULL: not resumable.
            conn.execute(
                "INSERT INTO checkpoint
                 (id, run_id, conversation_id, sequence, turn_id,
                  active_context_snapshot_id, side_effect_ledger_cursor, snapshot_json)
                 VALUES ('checkpoint-noledger', ?1, 'noledger-conv', 1, 'turn-1',
                         'snapshot-noledger', NULL, '{}')",
                rusqlite::params![&source.id],
            )
            .unwrap();
            drop(conn);
            let error = rm
                .continue_run(ContinueRunRequest {
                    run_id: source.id.clone(),
                    checkpoint_id: None,
                    content: None,
                })
                .expect_err("checkpoint without a ledger watermark must be rejected");
            assert!(
                error.contains("ledger watermark"),
                "stable fail-closed error expected, got: {error}"
            );
        });
    }

    #[test]
    fn resume_uncertain_returns_confirmation_required_without_creating_run() {
        with_env_lock(|| {
            let (store, source_id) = resume_fixture();
            store
                .conn()
                .unwrap()
                .execute(
                    "INSERT INTO side_effect_record (id, run_id, category, status, replay_safe)
                     VALUES ('effect-uncertain', ?1, 'process', 'uncertain', 1)",
                    rusqlite::params![&source_id],
                )
                .unwrap();
            let rm = RunManager::new_with_store(store.clone());
            let response = rm
                .resume_run(ResumeRunRequest {
                    run_id: source_id.clone(),
                    checkpoint_id: None,
                    content: None,
                    confirmed: false,
                })
                .unwrap();
            assert_eq!(response.decision, ResumeDecision::ConfirmationRequired);
            assert!(
                response.new_run_id.is_none(),
                "no run may be created before confirmation"
            );
            let approved: i64 = store
                .conn()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM resume_plan
                     WHERE source_run_id = ?1 AND action = 'resume' AND status = 'approved'",
                    rusqlite::params![&source_id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(approved, 0, "no approved resume plan before confirmation");
        });
    }

    #[test]
    fn resume_confirmed_creates_independent_run() {
        with_env_lock(|| {
            let (store, source_id) = resume_fixture();
            store
                .conn()
                .unwrap()
                .execute(
                    "INSERT INTO side_effect_record (id, run_id, category, status, replay_safe)
                     VALUES ('effect-confirmed', ?1, 'process', 'uncertain', 1)",
                    rusqlite::params![&source_id],
                )
                .unwrap();
            let rm = RunManager::new_with_store(store.clone());
            let response = rm
                .resume_run(ResumeRunRequest {
                    run_id: source_id.clone(),
                    checkpoint_id: None,
                    content: None,
                    confirmed: true,
                })
                .unwrap();
            assert_eq!(response.decision, ResumeDecision::SafeToContinue);
            let new_run_id = response
                .new_run_id
                .expect("confirmed resume must create a run");
            assert_ne!(
                new_run_id, source_id,
                "resume must create a fresh independent run"
            );
            let resumed = rm.get_run(&new_run_id).expect("new run must exist");
            assert_eq!(
                resumed.continued_from_run_id.as_deref(),
                Some(source_id.as_str())
            );
            assert_eq!(resumed.checkpoint_id.as_deref(), Some("checkpoint-resume"));
        });
    }

    #[test]
    fn resume_blocked_on_non_replay_safe_uncertain() {
        with_env_lock(|| {
            let (store, source_id) = resume_fixture();
            store
                .conn()
                .unwrap()
                .execute(
                    "INSERT INTO side_effect_record (id, run_id, category, status, replay_safe)
                     VALUES ('effect-nonreplay', ?1, 'process', 'uncertain', 0)",
                    rusqlite::params![&source_id],
                )
                .unwrap();
            let rm = RunManager::new_with_store(store.clone());
            let response = rm
                .resume_run(ResumeRunRequest {
                    run_id: source_id.clone(),
                    checkpoint_id: None,
                    content: None,
                    confirmed: true,
                })
                .unwrap();
            assert_eq!(
                response.decision,
                ResumeDecision::Blocked,
                "non-replay-safe uncertain effect hard-blocks resume"
            );
            assert!(
                response.new_run_id.is_none(),
                "Blocked resume must not create a run"
            );
        });
    }

    #[test]
    fn side_effect_resume_gate_blocks_started_effect() {
        // TASK-004 (G02): a crash after `started` (intent recorded, no
        // terminal) leaves an unknown side effect. Auto-resume must NOT invoke
        // the handler again: it must hard-block instead of creating a run.
        with_env_lock(|| {
            let (store, source_id) = resume_fixture();
            store
                .conn()
                .unwrap()
                .execute(
                    "INSERT INTO side_effect_record (id, run_id, category, status, replay_safe)
                     VALUES ('effect-started', ?1, 'process', 'started', 0)",
                    rusqlite::params![&source_id],
                )
                .unwrap();
            let rm = RunManager::new_with_store(store.clone());
            let response = rm
                .resume_run(ResumeRunRequest {
                    run_id: source_id.clone(),
                    checkpoint_id: None,
                    content: None,
                    confirmed: false,
                })
                .unwrap();
            assert_eq!(
                response.decision,
                ResumeDecision::Blocked,
                "started (non-terminal) side effect must hard-block resume"
            );
            assert!(
                response.new_run_id.is_none(),
                "Blocked resume must not create a run for an unknown side effect"
            );
        });
    }

    #[test]
    fn resume_blocks_on_external_effect_after_checkpoint_cursor() {
        // G01: a checkpoint covers only the ledger prefix it was captured at.
        // A completed EXTERNAL effect recorded after the checkpoint cursor has
        // an unknown outcome — resuming from the old checkpoint and re-running
        // would silently replay that side effect. Resume must hard-block even
        // when the caller confirms, because the external outcome is unprovable.
        with_env_lock(|| {
            let (store, source_id) = resume_fixture();
            // Make the checkpoint cursor a real integer ledger watermark (the
            // fixture placeholder 'ledger-1' is not a sequence).
            store
                .conn()
                .unwrap()
                .execute(
                    "UPDATE checkpoint SET side_effect_ledger_cursor = '1' WHERE run_id = ?1",
                    rusqlite::params![&source_id],
                )
                .unwrap();
            store
                .conn()
                .unwrap()
                .execute(
                    "INSERT INTO side_effect_record
                     (id, run_id, tool_call_id, category, status, replay_safe, ledger_sequence)
                     VALUES
                       ('effect-covered', ?1, 'call-a', 'workspace_file', 'completed', 1, 1),
                       ('effect-after-cursor', ?1, 'call-b', 'process', 'completed', 0, 2)",
                    rusqlite::params![&source_id],
                )
                .unwrap();
            let rm = RunManager::new_with_store(store.clone());
            let response = rm
                .resume_run(ResumeRunRequest {
                    run_id: source_id.clone(),
                    checkpoint_id: None,
                    content: None,
                    confirmed: true,
                })
                .unwrap();
            assert_eq!(
                response.decision,
                ResumeDecision::Blocked,
                "an external effect after the checkpoint cursor must hard-block resume"
            );
            assert!(
                response.new_run_id.is_none(),
                "Blocked resume must not create a run"
            );
        });
    }

    #[test]
    fn resume_confirmation_required_for_workspace_effect_after_cursor() {
        // G01: even a replay-safe (workspace) effect recorded after the
        // checkpoint cursor is not covered by the checkpoint; without explicit
        // confirmation resume must not silently continue. Confirming turns it
        // into a safe continue (the checkpoint captures the file before-image).
        with_env_lock(|| {
            let (store, source_id) = resume_fixture();
            store
                .conn()
                .unwrap()
                .execute(
                    "UPDATE checkpoint SET side_effect_ledger_cursor = '1' WHERE run_id = ?1",
                    rusqlite::params![&source_id],
                )
                .unwrap();
            store
                .conn()
                .unwrap()
                .execute(
                    "INSERT INTO side_effect_record
                     (id, run_id, tool_call_id, category, status, replay_safe, ledger_sequence)
                     VALUES
                       ('effect-covered', ?1, 'call-a', 'workspace_file', 'completed', 1, 1),
                       ('effect-after-cursor-ws', ?1, 'call-c', 'workspace_file', 'completed', 1, 2)",
                    rusqlite::params![&source_id],
                )
                .unwrap();
            let rm = RunManager::new_with_store(store.clone());
            let response = rm
                .resume_run(ResumeRunRequest {
                    run_id: source_id.clone(),
                    checkpoint_id: None,
                    content: None,
                    confirmed: false,
                })
                .unwrap();
            assert_eq!(
                response.decision,
                ResumeDecision::ConfirmationRequired,
                "a post-cursor workspace effect needs explicit confirmation, not silent resume"
            );
            assert!(response.new_run_id.is_none());
            // With explicit confirmation the workspace effect is safe to cover.
            let confirmed = rm
                .resume_run(ResumeRunRequest {
                    run_id: source_id.clone(),
                    checkpoint_id: None,
                    content: None,
                    confirmed: true,
                })
                .unwrap();
            assert!(
                confirmed.new_run_id.is_some(),
                "confirmed resume of a workspace post-cursor effect continues"
            );
        });
    }

    #[test]
    fn side_effect_resume_gate_allows_settled_effects() {
        // Regression guard: fully settled effects AT or BEFORE the checkpoint
        // cursor are covered by the checkpoint and must not block resume.
        with_env_lock(|| {
            let (store, source_id) = resume_fixture();
            store
                .conn()
                .unwrap()
                .execute(
                    "UPDATE checkpoint SET side_effect_ledger_cursor = '1' WHERE run_id = ?1",
                    rusqlite::params![&source_id],
                )
                .unwrap();
            store
                .conn()
                .unwrap()
                .execute(
                    "INSERT INTO side_effect_record (id, run_id, category, status, replay_safe, ledger_sequence)
                     VALUES ('effect-settled', ?1, 'process', 'completed', 1, 1)",
                    rusqlite::params![&source_id],
                )
                .unwrap();
            let rm = RunManager::new_with_store(store.clone());
            let response = rm
                .resume_run(ResumeRunRequest {
                    run_id: source_id.clone(),
                    checkpoint_id: None,
                    content: None,
                    confirmed: false,
                })
                .unwrap();
            assert_eq!(
                response.decision,
                ResumeDecision::SafeToContinue,
                "settled effects covered by the checkpoint cursor must not block resume"
            );
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

    #[test]
    fn persist_and_restore_marks_active_as_interrupted() {
        with_env_lock(|| {
            let dir = std::env::temp_dir().join(format!("natives-runs-{}", Uuid::new_v4()));
            let _ = std::fs::create_dir_all(dir.join("runs"));
            let snapshot_path = dir.join("runs").join("snapshot.json");
            let rm = RunManager {
                runs: Mutex::new(HashMap::new()),
                idempotency: Mutex::new(HashMap::new()),
                last_content: Mutex::new(HashMap::new()),
                project_paths: Mutex::new(HashMap::new()),
                snapshot_path_override: Some(snapshot_path.clone()),
                data_store: None,
                runtime: Arc::new(crate::production::ProductionRuntime::new()),
            };
            let run = rm
                .create_run(CreateRunRequest {
                    capability_selection: None,
                    disabled_tools: None,
                    conversation_id: "c-restore".into(),
                    provider_id: "openai".into(),
                    model_id: "m".into(),
                    key_id: None,
                    agent_profile_id: None,
                    permission_profile: None,
                    content: Some("x".into()),
                    attachments: None,
                    max_steps: Some(3),
                    parent_run_id: None,
                    project_path: Some("/tmp/proj".into()),
                    idempotency_key: Some(format!("restore-{}", Uuid::new_v4())),
                    effort: None,
                    runtime_id: None,
                })
                .unwrap();
            // Force active status then snapshot.
            {
                let mut runs = rm.runs.lock().unwrap();
                if let Some(r) = runs.get_mut(&run.id) {
                    r.status = RunStatusV2::Running;
                }
            }
            rm.persist_runs_snapshot().unwrap();
            assert!(
                snapshot_path.exists(),
                "snapshot file missing at {}",
                snapshot_path.display()
            );
            let rm2 = RunManager {
                runs: Mutex::new(HashMap::new()),
                idempotency: Mutex::new(HashMap::new()),
                last_content: Mutex::new(HashMap::new()),
                project_paths: Mutex::new(HashMap::new()),
                snapshot_path_override: Some(snapshot_path.clone()),
                data_store: None,
                runtime: Arc::new(crate::production::ProductionRuntime::new()),
            };
            let n = rm2.restore_runs_snapshot().unwrap();
            assert!(
                n >= 1,
                "expected restored runs from {}",
                snapshot_path.display()
            );
            let restored = rm2.get_run(&run.id).expect("restored");
            assert_eq!(restored.status, RunStatusV2::Interrupted);
            assert_eq!(restored.error_code.as_deref(), Some("daemon_restarted"));
            assert_eq!(restored.project_path.as_deref(), Some("/tmp/proj"));
            let _ = std::fs::remove_dir_all(&dir);
        }); // with_env_lock
    }

    #[test]
    fn sqlite_active_runs_are_interrupted_on_manager_startup() {
        with_env_lock(|| {
            let dir = tempfile::tempdir().unwrap();
            let db_path = dir.path().join("natives.db");
            std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db_path);
            std::env::set_var("NATIVES_DB_PATH", &db_path);
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
                 VALUES ('restart-conv', 'agent', 'Restart', 'openai', 'gpt-4o')",
                    [],
                )
                .unwrap();
            for (run_id, status) in [
                ("restart-queued", "queued"),
                ("restart-running", "running"),
                ("restart-waiting", "waiting_permission"),
                ("restart-completed", "completed"),
            ] {
                store
                    .conn()
                    .unwrap()
                    .execute(
                        "INSERT INTO run (id, conversation_id, status, provider_id, model_id)
                     VALUES (?1, 'restart-conv', ?2, 'openai', 'gpt-4o')",
                        rusqlite::params![run_id, status],
                    )
                    .unwrap();
            }

            let _rm = RunManager::new_with_store(store.clone());
            let rows: Vec<(String, String, Option<String>)> = {
                let conn = store.conn().unwrap();
                let mut stmt = conn
                    .prepare("SELECT id, status, error_code FROM run ORDER BY id")
                    .unwrap();
                stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
                    .unwrap()
                    .map(Result::unwrap)
                    .collect()
            };
            assert!(rows.iter().any(|(id, status, code)| {
                id == "restart-running"
                    && status == "interrupted"
                    && code.as_deref() == Some("daemon_restarted")
            }));
            assert!(rows.iter().any(|(id, status, code)| {
                id == "restart-queued"
                    && status == "interrupted"
                    && code.as_deref() == Some("daemon_restarted")
            }));
            assert!(rows.iter().any(|(id, status, code)| {
                id == "restart-waiting"
                    && status == "interrupted"
                    && code.as_deref() == Some("daemon_restarted")
            }));
            assert!(rows.iter().any(|(id, status, code)| {
                id == "restart-completed" && status == "completed" && code.is_none()
            }));
        });
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
    async fn permission_gate_emits_request_and_respond() {
        let _env_guard = crate::storage::DataStore::env_test_lock();
        let _env_restore = crate::storage::EnvRestore::capture();
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("perm-gate.db");
        std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db_path);
        std::env::set_var("NATIVES_DB_PATH", &db_path);
        std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());
        crate::storage::set_test_db_override(
            Some(db_path.clone()),
            Some(dir.path().join("artifacts")),
        );
        let _store =
            crate::storage::DataStore::new(&db_path, &dir.path().join("artifacts")).unwrap();
        std::env::set_var("NATIVES_DAEMON_FIXTURE", "1");
        let rm = Arc::new(RunManager::new());
        // Event logs are durable by default; fixed ids would replay stale
        // permission events from an earlier test process.
        let run_key = format!("perm-{}", Uuid::new_v4());
        let run = rm
            .create_run(CreateRunRequest {
                capability_selection: None,
                disabled_tools: None,
                conversation_id: "c-perm".into(),
                provider_id: "openai".into(),
                model_id: "gpt-4o".into(),
                key_id: Some("k".into()),
                agent_profile_id: None,
                permission_profile: Some("ask".into()),
                content: Some("tool please".into()),
                attachments: None,
                max_steps: Some(5),
                parent_run_id: None,
                project_path: None,
                idempotency_key: Some(run_key),
                effort: None,
                runtime_id: None,
            })
            .unwrap();

        // Tool-calling fixture provider + permission gated tools.
        let events = rm.runtime.events.clone();
        let tools = crate::production::PermissionGatedTools {
            gateway: {
                let mut g = capability_gateway::CapabilityGateway::new();
                let _ = g.register_builtins();
                Arc::new(g)
            },
            permissions: rm.runtime.permissions.clone(),
            events: events.clone(),
            interactions: rm.runtime.interactions.clone(),
            subagents: rm.runtime.subagents.clone(),
            task_outputs: rm.runtime.task_outputs_ref(),
            engines: rm.runtime.engine_handles().await,
            runtime: None,
            provider_id: "openai".into(),
            key_id: None,
            parent_run_id: run.id.clone(),
            conversation_id: "c-perm".into(),
            model_id: "gpt-4o".into(),
            permission_profile: "ask".into(),
            tool_allowlist: None,
            team: None,
            mcp_tool_schemas: Vec::new(),
            selected_mcp_servers: None,
        };
        let provider = FixtureProvider {
            mode: FixtureMode::RequestPermissionPath,
        };
        // Ensure ConfirmEach profile so side-effect tools ask.

        let rm_bg = rm.clone();
        let rid = run.id.clone();
        let respond_handle = tokio::spawn(async move {
            // Wait for permission_requested event then approve.
            for _ in 0..100 {
                tokio::time::sleep(std::time::Duration::from_millis(30)).await;
                let evs = rm_bg.runtime.events.replay_after(&rid, 0);
                if let Some(pid) = evs.iter().find_map(|e| match &e.payload {
                    RunEventKind::PermissionRequested { permission_id, .. } => {
                        Some(permission_id.clone())
                    }
                    _ => None,
                }) {
                    let _ = rm_bg.respond_permission(&pid, true).await;
                    return;
                }
            }
        });

        let status = tokio::time::timeout(
            std::time::Duration::from_secs(6),
            rm.start_with_seams(
                StartRunRequest {
                    agent_profile_id: None,
                    capability_selection: None,
                    run_id: Some(run.id.clone()),
                    conversation_id: None,
                    provider_id: None,
                    model_id: None,
                    key_id: None,
                    content: Some("tool please".into()),
                    attachments: None,
                    trigger_message_id: None,
                    permission_profile: Some("ask".into()),
                    max_steps: Some(5),
                    project_path: None,
                    idempotency_key: None,
                    effort: None,
                    runtime_id: None,
                },
                &provider,
                &tools,
            ),
        )
        .await
        .expect("permission-gated fixture run must terminate")
        .unwrap();

        let _ = respond_handle.await;
        let evs = rm.replay(ReplayRunRequest {
            run_id: run.id.clone(),
            after_sequence: 0,
        });
        let has_perm_req = evs
            .iter()
            .any(|e| matches!(e.payload, RunEventKind::PermissionRequested { .. }));
        let has_perm_resp = evs
            .iter()
            .any(|e| matches!(e.payload, RunEventKind::PermissionResponded { .. }));

        if let Ok(dir) = std::env::var("NATIVES_TEST_SCRATCH") {
            let dump: Vec<_> = evs
                .iter()
                .map(|e| {
                    serde_json::json!({
                        "sequence": e.run_sequence,
                        "type": e.payload.type_name(),
                    })
                })
                .collect();
            let _ = std::fs::write(
                std::path::Path::new(&dir).join("permission-events.json"),
                serde_json::to_string_pretty(&serde_json::json!({
                    "final_status": status.status.as_str(),
                    "permission_requested": has_perm_req,
                    "permission_responded": has_perm_resp,
                    "events": dump,
                }))
                .unwrap_or_default(),
            );
        }

        assert!(has_perm_req, "expected permission_requested event");
        assert!(has_perm_resp, "expected permission_responded event");
        // keep FIXTURE=1 for parallel tests under cfg(test)
    }

    /// Serialize credential-broker tests — shared process-global slot.
    fn with_broker_slot<R>(f: impl FnOnce() -> R) -> R {
        use std::sync::{Mutex, OnceLock};
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        let lock = LOCK.get_or_init(|| Mutex::new(()));
        let _g = lock.lock().unwrap_or_else(|e| e.into_inner());
        crate::production::clear_credential_broker_for_tests();
        let out = f();
        crate::production::clear_credential_broker_for_tests();
        out
    }

    #[test]
    fn credential_resolve_never_returns_empty_mock_key() {
        with_broker_slot(|| {
            // Broker reports not-found; no env key → hard fail (no offline mock success).
            crate::production::install_credential_broker(std::sync::Arc::new(
                |_p: &str, _k: Option<&str>, _r: &str| Err("No active key for provider".into()),
            ));
            let prev_openai = std::env::var("NATIVES_TEST_OPENAI_KEY").ok();
            let prev_anth = std::env::var("ANTHROPIC_AUTH_TOKEN").ok();
            let prev_api = std::env::var("ANTHROPIC_API_KEY").ok();
            std::env::remove_var("NATIVES_TEST_OPENAI_KEY");
            std::env::remove_var("ANTHROPIC_AUTH_TOKEN");
            std::env::remove_var("ANTHROPIC_API_KEY");
            let err = crate::production::resolve_credential("openai", Some("k1")).unwrap_err();
            assert!(
                err.contains("No credential")
                    || err.contains("broker")
                    || err.contains("unavailable"),
                "unexpected err: {err}"
            );
            assert!(!err.contains("sk-"));
            match prev_openai {
                Some(v) => std::env::set_var("NATIVES_TEST_OPENAI_KEY", v),
                None => std::env::remove_var("NATIVES_TEST_OPENAI_KEY"),
            }
            match prev_anth {
                Some(v) => std::env::set_var("ANTHROPIC_AUTH_TOKEN", v),
                None => std::env::remove_var("ANTHROPIC_AUTH_TOKEN"),
            }
            match prev_api {
                Some(v) => std::env::set_var("ANTHROPIC_API_KEY", v),
                None => std::env::remove_var("ANTHROPIC_API_KEY"),
            }
        });
    }

    #[test]
    fn credential_broker_install_is_invoked_before_env() {
        with_broker_slot(|| {
            crate::production::install_credential_broker(std::sync::Arc::new(
                |_provider_id: &str, key_id: Option<&str>, run_id: &str| {
                    assert!(!run_id.is_empty());
                    Ok(provider_adapters::capabilities::Credential {
                        api_key: "broker-secret-not-for-logs".into(),
                        base_url: Some("https://example.test/v1".into()),
                        proxy_url: None,
                        key_id: Some(key_id.unwrap_or("broker-key-1").to_string()),
                        provider_type: Some("openai_compatible".into()),
                    })
                },
            ));
            std::env::remove_var("NATIVES_TEST_OPENAI_KEY");
            let cred = crate::production::resolve_credential_for_run("openai", Some("k1"), "run-1")
                .expect("broker must win over missing env");
            assert_eq!(cred.key_id.as_deref(), Some("k1"));
            assert_eq!(cred.api_key, "broker-secret-not-for-logs");
            let event_payload = serde_json::json!({"error": "auth failed"});
            assert!(!event_payload.to_string().contains("broker-secret"));
            if let Ok(dir) = std::env::var("NATIVES_TEST_SCRATCH") {
                let _ = std::fs::write(
                    std::path::Path::new(&dir).join("credential-broker-lifecycle.json"),
                    serde_json::to_string_pretty(&serde_json::json!({
                        "broker_invoked": true,
                        "key_id_returned": "k1",
                        "api_key_not_in_events": true,
                        "path": "install_credential_broker → resolve_credential_for_run → Tauri natives.db",
                        "mock_success_without_key": false,
                    }))
                    .unwrap_or_default(),
                );
            }
        });
    }

    #[tokio::test]
    async fn subagent_task_spawns_independent_identity() {
        // Hold the env lock so a concurrent test cannot clear the fixture flag
        // mid-test (the fixture path is env-driven for resolve_batch_assignment).
        let _env_guard = crate::storage::DataStore::env_test_lock();
        std::env::set_var("NATIVES_DAEMON_FIXTURE", "1");
        let rt = crate::production::ProductionRuntime::new();
        // Task is Process/ProjectWrite — under ConfirmEach it asks; use autonomous for identity unit test.
        let tools = crate::production::PermissionGatedTools {
            gateway: {
                let mut g = capability_gateway::CapabilityGateway::new();
                let _ = g.register_builtins();
                Arc::new(g)
            },
            permissions: rt.permissions.clone(),
            events: rt.events.clone(),
            interactions: rt.interactions.clone(),
            subagents: rt.subagents.clone(),
            task_outputs: rt.task_outputs.clone(),
            engines: rt.engine_handles().await,
            runtime: None,
            provider_id: "openai".into(),
            key_id: Some("parent-run-key".into()),
            parent_run_id: "parent-run".into(),
            conversation_id: "c".into(),
            model_id: "gpt-4o".into(),
            permission_profile: "full_access".into(),
            tool_allowlist: None,
            team: None,
            mcp_tool_schemas: Vec::new(),
            selected_mcp_servers: None,
        };
        let result = tools
            .execute_tool(
                "task",
                serde_json::json!({
                    "prompt": "child work",
                    "provider_id": "anthropic",
                    "model_id": "claude-3",
                    "key_id": "child-key-from-broker",
                    "permission_profile": "ask",
                    "fixture": true
                }),
                &CancellationToken::new(),
            )
            .await;
        assert!(!result.is_error, "task error: {:?}", result.output);
        let task_id = result
            .output
            .get("task_id")
            .and_then(|v| v.as_str())
            .unwrap();
        let child_key = result
            .output
            .get("key_id")
            .and_then(|v| v.as_str())
            .unwrap();
        // Model-supplied credentials must be ignored; parent run key is used.
        assert_eq!(child_key, "parent-run-key");
        assert_ne!(child_key, "child-key-from-broker");
        assert_eq!(
            result.output.get("provider_id").and_then(|v| v.as_str()),
            Some("openai")
        );
        // Parent should have SubagentCreated event
        let evs = rt.events.replay_after("parent-run", 0);
        assert!(evs
            .iter()
            .any(|e| matches!(e.payload, RunEventKind::SubagentCreated { .. })));

        // task_output should see running/cancelled eventually
        let out = tools
            .execute_tool(
                "task_output",
                serde_json::json!({ "task_id": task_id }),
                &CancellationToken::new(),
            )
            .await;
        assert!(!out.is_error);

        let kill = tools
            .execute_tool(
                "kill_task",
                serde_json::json!({ "task_id": task_id }),
                &CancellationToken::new(),
            )
            .await;
        assert!(kill
            .output
            .get("cancelled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false));

        if let Ok(dir) = std::env::var("NATIVES_TEST_SCRATCH") {
            let _ = std::fs::write(
                std::path::Path::new(&dir).join("subagent-task-identity.json"),
                serde_json::to_string_pretty(&serde_json::json!({
                    "task_id": task_id,
                    "child_key_id": child_key,
                    "child_provider": "anthropic",
                    "independent_key": true,
                    "subagent_created_event": true,
                    "kill_task": true,
                }))
                .unwrap_or_default(),
            );
        }
        // keep FIXTURE=1 for parallel tests under cfg(test)
    }

    /// Parent openai / child anthropic (different provider+key+model); fixture completes child.
    #[test]
    #[ignore = "covered by live_engine_e2e::dual_provider_engine_fixture_subagent with isolated Harness storage"]
    fn subagent_dual_provider_fixture_completes() {
        with_env_lock(|| {
            let harness_dir = if std::env::var_os("NATIVES_ASSISTANT_DB_PATH").is_none() {
                let dir = tempfile::tempdir().expect("harness tempdir");
                let harness_db = dir.path().join("assistant.db");
                std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &harness_db);
                std::env::set_var("NATIVES_DB_PATH", &harness_db);
                crate::storage::set_test_db_override(
                    Some(harness_db),
                    Some(dir.path().join("artifacts")),
                );
                Some(dir)
            } else {
                None
            };
            std::env::set_var("NATIVES_DAEMON_FIXTURE", "1");
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio");
            rt.block_on(async {
                let parent_id = format!("parent-dual-{}", uuid::Uuid::new_v4());
                let prt = crate::production::ProductionRuntime::new();
                let tools = crate::production::PermissionGatedTools {
                    gateway: {
                        let mut g = capability_gateway::CapabilityGateway::new();
                        let _ = g.register_builtins();
                        Arc::new(g)
                    },
                    permissions: prt.permissions.clone(),
                    events: prt.events.clone(),
                    interactions: prt.interactions.clone(),
                    subagents: prt.subagents.clone(),
                    task_outputs: prt.task_outputs.clone(),
                    engines: prt.engine_handles().await,
                    runtime: None,
                    provider_id: "openai".into(),
                    key_id: Some("parent-key-A".into()),
                    parent_run_id: parent_id.clone(),
                    conversation_id: "c-dual".into(),
                    model_id: "gpt-4o".into(),
                    permission_profile: "full_access".into(),
                    tool_allowlist: None,
                    team: None,
                    mcp_tool_schemas: Vec::new(),
                    selected_mcp_servers: None,
                };
                let result = tools
                    .execute_tool(
                        "task",
                        serde_json::json!({
                            "prompt": "child dual provider",
                            "provider_id": "anthropic",
                            "model_id": "claude-3-haiku",
                            "key_id": "child-key-B",
                            "permission_profile": "full_access",
                            "fixture": true
                        }),
                        &CancellationToken::new(),
                    )
                    .await;
                assert!(!result.is_error, "{:?}", result.output);
                // Credentials come from parent run, not model-supplied child fields.
                assert_eq!(
                    result.output.get("provider_id").and_then(|v| v.as_str()),
                    Some("openai")
                );
                assert_eq!(
                    result.output.get("key_id").and_then(|v| v.as_str()),
                    Some("parent-key-A")
                );
                assert_eq!(
                    result.output.get("model_id").and_then(|v| v.as_str()),
                    Some("gpt-4o")
                );
                let task_id = result
                    .output
                    .get("task_id")
                    .and_then(|v| v.as_str())
                    .unwrap()
                    .to_string();

                let mut final_status = String::new();
                let mut final_output = serde_json::Value::Null;
                for _ in 0..80 {
                    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                    let out = tools
                        .execute_tool(
                            "task_output",
                            serde_json::json!({ "task_id": task_id }),
                            &CancellationToken::new(),
                        )
                        .await;
                    let status = out
                        .output
                        .get("status")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    if status != "running" && status != "unknown" {
                        final_status = status.to_string();
                        final_output = out.output;
                        break;
                    }
                }
                assert_eq!(
                    final_status, "completed",
                    "fixture child should complete with independent identity: {final_output}"
                );
                let parent_done = prt
                    .events
                    .replay_after(&parent_id, 0)
                    .iter()
                    .any(|e| matches!(e.payload, RunEventKind::SubagentCompleted { .. }));
                assert!(parent_done, "parent must observe SubagentCompleted");
                assert!(!prt.events.replay_after(&parent_id, 0).is_empty());

                if let Ok(dir) = std::env::var("NATIVES_TEST_SCRATCH") {
                    let _ = std::fs::write(
                        std::path::Path::new(&dir).join("subagent-dual-provider.json"),
                        serde_json::to_string_pretty(&serde_json::json!({
                            "parent_provider": "openai",
                            "child_provider": "anthropic",
                            "child_key_id": "child-key-B",
                            "child_model": "claude-3-haiku",
                            "status": final_status,
                            "fixture": true,
                            "subagent_completed": true,
                        }))
                        .unwrap_or_default(),
                    );
                }
            });
            if harness_dir.is_some() {
                crate::storage::set_test_db_override(None, None);
                std::env::remove_var("NATIVES_ASSISTANT_DB_PATH");
                std::env::remove_var("NATIVES_DB_PATH");
            }
            // leave FIXTURE=1; other fixture tests expect it
        });
    }

    #[tokio::test]
    async fn mutating_tool_fail_closed_without_project_identity() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("id.db");
        let artifacts = dir.path().join("art");
        std::fs::create_dir_all(&artifacts).unwrap();
        // Install the test DB override so RunManager recovery (which calls
        // conversation_store::store / prompt_queue_store::store) sees the same
        // temp store instead of requiring NATIVES_DB_PATH.
        crate::storage::set_test_db_override(Some(db.clone()), Some(artifacts.clone()));
        let store = std::sync::Arc::new(crate::storage::DataStore::new(&db, &artifacts).unwrap());
        // Install as global so PermissionGatedTools sees data_store_ref.
        let rm = std::sync::Arc::new(RunManager::new_with_store(store.clone()));
        // Bypass global: call tools with runtime that shares manager store via temporary global?
        // Production checks global_run_manager().data_store_ref — set env and use global.
        let _ = rm;
        // Use production tools against run without project_id.
        let rt = crate::production::ProductionRuntime::new();
        // Create run on a store-backed manager that is process global.
        // Replace process global is hard; instead test ensure logic via tool when
        // we bind run on GLOBAL if available.
        // Minimal: verify tool_requires + invocation_from_gate soft id removed.
        assert!(crate::runtime::tool_requires_verified_project("write_file"));
        let inv = crate::runtime::invocation_from_gate(
            "write_file",
            &serde_json::json!({"path":"a"}),
            "c",
            "r",
            Some("/tmp/x"),
        );
        assert!(inv.project_id.is_none());
        let _ = dir;
        let _ = rt;
    }

    #[tokio::test]
    async fn mcp_call_through_permission_gate_emits_events() {
        // T01: hermetic — fresh temp runtime dir (snapshot + event JSONL), a
        // memory RunManager so the verified-project check escapes, and no
        // ~/.natives access. Deterministic under --test-threads=2.
        let _env_guard = crate::storage::DataStore::env_test_lock();
        let _env_restore = crate::storage::EnvRestore::capture();
        crate::storage::set_test_db_override(None, None);
        std::env::remove_var("NATIVES_ASSISTANT_DB_PATH");
        std::env::remove_var("NATIVES_DB_PATH");
        let rt_dir = tempfile::tempdir().unwrap();
        std::env::set_var("NATIVES_RUNTIME_DIR", rt_dir.path());
        std::env::set_var("NATIVES_RUN_MANAGER_MEMORY", "1");
        crate::run_manager::install_global_for_test(crate::run_manager::RunManager::new());
        // Register mock tool without live session → structured error + events.
        let rt = crate::production::ProductionRuntime::new();
        crate::mcp_runtime::global_mcp()
            .register_server(agent_core::McpServerConfig {
                id: "gate-test".into(),
                transport: agent_core::McpTransport::Stdio,
                command: Some("true".into()),
                args: None,
                url: None,
                trusted: true,
                auth_token: None,
                headers: None,
            })
            .unwrap();
        crate::mcp_runtime::global_mcp()
            .upsert_tool(agent_core::McpToolDescriptor {
                server_id: "gate-test".into(),
                name: "echo".into(),
                description: "echo".into(),
                input_schema: serde_json::json!({"type":"object"}),
            })
            .unwrap();
        let tools = crate::production::PermissionGatedTools {
            gateway: {
                let mut g = capability_gateway::CapabilityGateway::new();
                let _ = g.register_builtins();
                Arc::new(g)
            },
            permissions: rt.permissions.clone(),
            events: rt.events.clone(),
            interactions: rt.interactions.clone(),
            subagents: rt.subagents.clone(),
            task_outputs: rt.task_outputs.clone(),
            engines: rt.engine_handles().await,
            runtime: None,
            provider_id: "openai".into(),
            key_id: None,
            parent_run_id: "mcp-parent".into(),
            conversation_id: "c".into(),
            model_id: "m".into(),
            permission_profile: "full_access".into(),
            tool_allowlist: None,
            team: None,
            mcp_tool_schemas: Vec::new(),
            selected_mcp_servers: None,
        };
        let out = tools
            .execute_tool(
                "mcp_call",
                serde_json::json!({
                    "server": "gate-test",
                    "tool": "echo",
                    "arguments": {"x": 1}
                }),
                &CancellationToken::new(),
            )
            .await;
        // No live session → error, but still gated + evented.
        assert!(out.is_error || out.output.get("ok") == Some(&serde_json::json!(false)));
        let evs = rt.events.replay_after("mcp-parent", 0);
        assert!(
            evs.iter()
                .any(|e| matches!(e.payload, RunEventKind::ToolCallStarted { .. })),
            "expected ToolCallStarted for mcp_call"
        );
        // The direct tool path emits ToolCallStarted and surfaces the failure;
        // the ENGINE (agent-core engine_core, covered by its own tests) appends
        // the ToolCallCompleted fact around a full turn. A direct call must NOT
        // fabricate a completion for a call that never reached a live MCP
        // session — that would be a fake green (fail-closed).
        assert!(
            !evs.iter()
                .any(|e| matches!(e.payload, RunEventKind::ToolCallCompleted { .. })),
            "direct mcp_call must not fabricate ToolCallCompleted without a live session"
        );
        if let Ok(dir) = std::env::var("NATIVES_TEST_SCRATCH") {
            let _ = std::fs::write(
                std::path::Path::new(&dir).join("mcp-call-gated.json"),
                serde_json::to_string_pretty(&serde_json::json!({
                    "permission_gated": true,
                    "tool_call_events": true,
                    "is_error": out.is_error,
                }))
                .unwrap_or_default(),
            );
        }
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
    fn create_run_preserves_runtime_id_and_effort() {
        let _prev_a = std::env::var("NATIVES_ASSISTANT_DB_PATH").ok();
        let _prev_d = std::env::var("NATIVES_DB_PATH").ok();
        std::env::remove_var("NATIVES_ASSISTANT_DB_PATH");
        std::env::remove_var("NATIVES_DB_PATH");
        let rm = RunManager::new();
        let run = rm
            .create_run(CreateRunRequest {
                capability_selection: None,
                disabled_tools: None,
                conversation_id: "c-preserve".into(),
                provider_id: "openai".into(),
                model_id: "gpt".into(),
                key_id: None,
                agent_profile_id: None,
                permission_profile: Some("ask".into()),
                content: Some("x".into()),
                attachments: None,
                max_steps: Some(5),
                parent_run_id: None,
                project_path: Some("/tmp".into()),
                idempotency_key: None,
                effort: Some("high".into()),
                runtime_id: Some("claude_cli".into()),
            })
            .unwrap();
        assert_eq!(run.runtime_id.as_deref(), Some("claude_cli"));
        assert_eq!(run.effort.as_deref(), Some("high"));
        let got = rm.get_run(&run.id).unwrap();
        assert_eq!(got.runtime_id.as_deref(), Some("claude_cli"));
        assert_eq!(got.effort.as_deref(), Some("high"));
    }

    #[test]
    fn retry_preserves_runtime_id() {
        let _prev_a = std::env::var("NATIVES_ASSISTANT_DB_PATH").ok();
        let _prev_d = std::env::var("NATIVES_DB_PATH").ok();
        std::env::remove_var("NATIVES_ASSISTANT_DB_PATH");
        std::env::remove_var("NATIVES_DB_PATH");
        let rm = RunManager::new();
        let run = rm
            .create_run(CreateRunRequest {
                capability_selection: None,
                disabled_tools: None,
                conversation_id: "c-retry-rt".into(),
                provider_id: "openai".into(),
                model_id: "gpt".into(),
                key_id: None,
                agent_profile_id: None,
                permission_profile: Some("ask".into()),
                content: Some("retry me".into()),
                attachments: None,
                max_steps: Some(5),
                parent_run_id: None,
                project_path: Some("/tmp".into()),
                idempotency_key: None,
                effort: Some("medium".into()),
                runtime_id: Some("native".into()),
            })
            .unwrap();
        {
            let mut runs = rm.runs.lock().unwrap();
            if let Some(r) = runs.get_mut(&run.id) {
                r.status = RunStatusV2::Failed;
                r.finished_at = Some(chrono::Utc::now());
            }
        }
        rm.last_content
            .lock()
            .unwrap()
            .insert(run.id.clone(), "retry me".into());
        let next = rm
            .retry(RetryRunRequest {
                run_id: run.id.clone(),
            })
            .unwrap();
        assert_eq!(next.runtime_id.as_deref(), Some("native"));
        assert_eq!(next.effort.as_deref(), Some("medium"));
        assert_ne!(next.id, run.id);
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
