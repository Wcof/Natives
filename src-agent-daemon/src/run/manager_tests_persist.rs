use super::*;

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
            if status != "completed" {
                let event =
                    assistant_protocol::v2::RunEventV2::new(run_id, 1, RunEventKind::Queued);
                store
                    .conn()
                    .unwrap()
                    .execute(
                        "INSERT INTO run_event (run_id, sequence, event_type, payload, timestamp, event_id)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                        rusqlite::params![
                            run_id,
                            event.run_sequence as i64,
                            event.payload.type_name(),
                            serde_json::to_string(&event).unwrap(),
                            event.timestamp.to_rfc3339(),
                            event.event_id,
                        ],
                    )
                    .unwrap();
            }
        }

        store
            .conn()
            .unwrap()
            .execute(
                "INSERT INTO run (
                    id, conversation_id, status, provider_id, model_id, project_path,
                    project_id, project_identity_version, effort, runtime_id
                 ) VALUES (
                    'restart-identity', 'restart-conv', 'completed', 'openai', 'gpt-4o',
                    '/tmp/restart-project', 'project-restart', 9, 'high', 'native'
                 )",
                [],
            )
            .unwrap();

        let rm = RunManager::new_with_store(store.clone());
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
        for run_id in ["restart-queued", "restart-running", "restart-waiting"] {
            let events = rm
                .replay_checked(ReplayRunRequest {
                    run_id: run_id.into(),
                    after_sequence: 0,
                })
                .unwrap();
            assert_eq!(
                events.len(),
                2,
                "{run_id} must have one terminal recovery event"
            );
            assert_eq!(events[0].run_sequence, 1);
            assert_eq!(events[1].run_sequence, 2);
            assert!(matches!(
                &events[1].payload,
                RunEventKind::Interrupted { reason } if reason == "daemon_restarted"
            ));
        }
        let restarted = RunManager::new_with_store(store.clone());
        for run_id in ["restart-queued", "restart-running", "restart-waiting"] {
            let events = restarted
                .replay_checked(ReplayRunRequest {
                    run_id: run_id.into(),
                    after_sequence: 0,
                })
                .unwrap();
            assert_eq!(events.len(), 2, "{run_id} recovery must be idempotent");
        }
        let hydrated = rm
            .get_run("restart-identity")
            .expect("hydrated identity run");
        assert_eq!(
            hydrated.project_path.as_deref(),
            Some("/tmp/restart-project")
        );
        assert_eq!(hydrated.project_id.as_deref(), Some("project-restart"));
        assert_eq!(hydrated.project_identity_version, Some(9));
        assert_eq!(hydrated.effort.as_deref(), Some("high"));
        assert_eq!(hydrated.runtime_id.as_deref(), Some("native"));
    });
}
