use super::*;

/// Build a terminal source run with a fully watermarked checkpoint and a
/// fresh store, under `with_env_lock`. Returns `(store, source_run_id)`.
fn resume_fixture() -> (std::sync::Arc<crate::storage::DataStore>, String) {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("resume.db");
    std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db_path);
    std::env::set_var("NATIVES_DB_PATH", &db_path);
    crate::storage::set_test_db_override(Some(db_path.clone()), Some(dir.path().join("artifacts")));
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
