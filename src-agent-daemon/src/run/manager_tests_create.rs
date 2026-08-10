use super::*;

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
