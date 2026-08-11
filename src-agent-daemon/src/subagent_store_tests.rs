use super::*;

fn with_temp_db<F: FnOnce()>(f: F) {
    let _guard = crate::storage::DataStore::env_test_lock();
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join(format!("subagent-{}.db", Uuid::new_v4()));
    let art = dir.path().join("artifacts");
    crate::storage::set_test_db_override(Some(db.clone()), Some(art.clone()));
    let _warm = crate::storage::DataStore::new(&db, &art).expect("subagent temp db migrate");
    // Ensure parent conversation exists for FK tests.
    crate::conversation_store::ensure_conversation_stub("parent-1", "openai", "gpt-4o", None, None)
        .unwrap();
    f();
    crate::storage::set_test_db_override(None, None);
}

#[test]
fn migration_010_tables_exist() {
    with_temp_db(|| {
        let s = store().unwrap();
        assert!(s.has_table("subagent_route_policy"));
        assert!(s.has_table("subagent_session"));
        let conn = s.conn().unwrap();
        let has_col: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('conversation')
                 WHERE name = 'parent_conversation_id'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(has_col, 1);
    });
}

#[test]
fn create_hidden_child_session_persists_permission_and_project_id() {
    with_temp_db(|| {
        let binding = RouteBinding {
            provider_id: "openai".into(),
            key_id: "key-1".into(),
            model_id: "gpt-4o".into(),
        };
        let (sid, _child) = create_hidden_child_session(
            "parent-1",
            None,
            None,
            "sub",
            "task",
            &binding,
            Some("ask"),
            Some("proj-path-as-id"),
        )
        .unwrap();
        let loaded = get_subagent_session(&sid).unwrap().expect("session");
        assert_eq!(loaded.permission_profile.as_deref(), Some("ask"));
        assert_eq!(loaded.project_id.as_deref(), Some("proj-path-as-id"));
    });
}

#[test]
fn create_hidden_child_and_list() {
    with_temp_db(|| {
        let binding = RouteBinding {
            provider_id: "openai".into(),
            key_id: "k1".into(),
            model_id: "gpt-4o".into(),
        };
        let (sid, child) = create_hidden_child_session(
            "parent-1",
            Some("run-1"),
            None,
            "worker",
            "do the thing",
            &binding,
            Some("ask"),
            None,
        )
        .unwrap();
        assert!(!sid.is_empty());
        assert!(!child.is_empty());
        let listed = list_subagent_sessions(Some("parent-1"), true).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].child_conversation_id, child);
        assert_eq!(listed[0].name, "worker");
        assert_eq!(listed[0].task, "do the thing");
        touch_subagent_session(&sid).unwrap();
        close_subagent_session(&sid, "closed", None).unwrap();
        let open = list_subagent_sessions(Some("parent-1"), false).unwrap();
        assert!(open.is_empty());
    });
}

#[test]
fn child_session_name_falls_back_to_prompt() {
    with_temp_db(|| {
        let binding = RouteBinding {
            provider_id: "openai".into(),
            key_id: "k1".into(),
            model_id: "gpt-4o".into(),
        };
        let (sid, _) = create_hidden_child_session(
            "parent-1",
            Some("run-1"),
            None,
            "",
            "  investigate   renderer sidebar leak  ",
            &binding,
            Some("ask"),
            None,
        )
        .unwrap();
        let sess = get_subagent_session(&sid).unwrap().unwrap();
        assert_eq!(sess.name, "investigate renderer sidebar leak");
    });
}

#[test]
fn failover_error_classification() {
    assert!(is_failover_eligible_error("HTTP 401 unauthorized"));
    assert!(is_failover_eligible_error("429 rate limit"));
    assert!(is_failover_eligible_error("network timeout"));
    assert!(is_failover_eligible_error("502 bad gateway"));
    assert!(!is_failover_eligible_error("permission denied"));
    assert!(!is_failover_eligible_error("tool_not_allowlisted"));
    assert!(!is_failover_eligible_error("max steps exceeded"));
}

#[test]
fn migration_011_status_completed_and_parent_heartbeat() {
    with_temp_db(|| {
        let s = store().unwrap();
        let _conn = s.conn().unwrap();
        // completed is accepted by CHECK
        let binding = RouteBinding {
            provider_id: "openai".into(),
            key_id: "k1".into(),
            model_id: "gpt-4o".into(),
        };
        let (sid, _) = create_hidden_child_session(
            "parent-1",
            Some("run-1"),
            Some("call-1"),
            "worker",
            "do",
            &binding,
            Some("ask"),
            None,
        )
        .unwrap();
        update_subagent_session_status(&sid, "completed", None).unwrap();
        let sess = get_subagent_session(&sid).unwrap().unwrap();
        assert_eq!(sess.status, "completed");

        touch_parent_heartbeat("parent-1").unwrap();
        assert!(parent_heartbeat_recent("parent-1", 90));
        let before = get_subagent_session(&sid)
            .unwrap()
            .unwrap()
            .last_activity_at;
        // Parent touch must not bump child activity.
        touch_parent_heartbeat("parent-1").unwrap();
        let after = get_subagent_session(&sid)
            .unwrap()
            .unwrap()
            .last_activity_at;
        assert_eq!(before, after);
    });
}

/// TASK-009 (N05/E04): a child scope binds the parent's REAL project id +
/// version — never the project path masquerading as an id — and a closed
/// session leaves no active orphan.
#[test]
fn subagent_lifecycle_binds_real_identity_and_closes_without_orphan() {
    with_temp_db(|| {
        let binding = RouteBinding {
            provider_id: "openai".into(),
            key_id: "key-1".into(),
            model_id: "gpt-4o".into(),
        };
        let (sid, _child) = create_hidden_child_session(
            "parent-1",
            None,
            None,
            "sub",
            "task",
            &binding,
            Some("ask"),
            Some("real-project-uuid"),
        )
        .unwrap();
        persist_subagent_scope(
            &sid,
            &SubagentScope {
                project_path: Some("/tmp/proj".into()),
                project_id: Some("real-project-uuid".into()),
                project_identity_version: Some(7),
                permission_profile: Some("ask".into()),
                agent_profile_id: None,
                max_steps: Some(30),
                tool_allowlist: vec![],
            },
        )
        .unwrap();
        let loaded = get_subagent_session(&sid).unwrap().expect("session");
        assert_eq!(loaded.project_id.as_deref(), Some("real-project-uuid"));
        assert_ne!(
            loaded.project_id.as_deref(),
            loaded.project_path.as_deref(),
            "the project path must never be stored as project_id"
        );
        assert_eq!(loaded.project_identity_version, Some(7));

        // E04: closing the session settles it — no active orphan remains.
        close_subagent_session(&sid, "completed", None).unwrap();
        let closed = get_subagent_session(&sid).unwrap().expect("session");
        assert_eq!(closed.status, "completed");
        assert!(
            closed.closed_at.is_some(),
            "closed session records its close"
        );
    });
}

// ── NE-P0-08 / §19.1: durable Child Directive persistence ──

/// §19.1: the pending directive is persisted into the protected pending
/// execution plan (reservation scope snapshot) BEFORE the child run is
/// created. A daemon crash between reservation and child start does not
/// lose the persona — the durable copy survives.
#[test]
fn pending_directive_persists_before_child_run_creation() {
    with_temp_db(|| {
        let binding = RouteBinding {
            provider_id: "openai".into(),
            key_id: "k1".into(),
            model_id: "gpt-4o".into(),
        };
        let (sid, _) = create_hidden_child_session(
            "parent-1",
            Some("parent-run"),
            None,
            "worker",
            "do",
            &binding,
            Some("ask"),
            None,
        )
        .unwrap();
        let text = "You are a crash-safe persona.";
        let digest = directive_sha256_hex(text);
        // Embed the directive in the reservation scope snapshot and
        // persist — this happens BEFORE the child run is created.
        let scope_snapshot =
            with_pending_directive(json!({ "project_id": "p1" }), Some((text, &digest)));
        let mut res = SubagentReservation {
            session_id: sid.clone(),
            parent_run_id: "parent-run".into(),
            tree_root_run_id: "tree-root".into(),
            depth: 1,
            max_tokens: Some(1_000),
            max_cost_usd: None,
            failure_policy: "fail_fast".into(),
            max_retries: 2,
            scope_snapshot,
        };
        reserve_subagent_slot(&res).unwrap();
        // The durable directive is readable from the session — the
        // child run does NOT need to exist for the persona to survive.
        let pd = pending_directive_for_session(&sid)
            .unwrap()
            .expect("durable directive survives before child run creation");
        assert_eq!(pd.text, text, "same text survives crash");
        assert_eq!(pd.digest, digest, "same digest survives crash");
    });
}

/// §19.5: crash recovery restores the EXACT same persona. The durable
/// directive (text + digest) is byte-identical across reads — the
/// loader is deterministic, not re-rolled or truncated.
#[test]
fn crash_recovery_restores_exact_same_persona() {
    with_temp_db(|| {
        let binding = RouteBinding {
            provider_id: "openai".into(),
            key_id: "k1".into(),
            model_id: "gpt-4o".into(),
        };
        let (sid, _) = create_hidden_child_session(
            "parent-1",
            Some("parent-run"),
            None,
            "worker",
            "do",
            &binding,
            Some("ask"),
            None,
        )
        .unwrap();
        let text = "You are a terse Rust reviewer. Check safety and logic.";
        let digest = directive_sha256_hex(text);
        let scope_snapshot = with_pending_directive(json!({}), Some((text, &digest)));
        let mut res = SubagentReservation {
            session_id: sid.clone(),
            parent_run_id: "parent-run".into(),
            tree_root_run_id: "tree-root".into(),
            depth: 1,
            max_tokens: Some(1_000),
            max_cost_usd: None,
            failure_policy: "fail_fast".into(),
            max_retries: 2,
            scope_snapshot,
        };
        reserve_subagent_slot(&res).unwrap();
        // Simulate crash: read the directive multiple times — each read
        // returns the exact same text + digest (deterministic recovery).
        let first = pending_directive_for_session(&sid).unwrap().unwrap();
        let second = pending_directive_for_session(&sid).unwrap().unwrap();
        assert_eq!(first.text, second.text, "text is deterministic");
        assert_eq!(first.digest, second.digest, "digest is deterministic");
        assert_eq!(first.text, text, "recovered text matches original");
        assert_eq!(first.digest, digest, "recovered digest matches original");
        // The digest verifies the text (same SHA-256).
        assert_eq!(
            directive_sha256_hex(&first.text),
            first.digest,
            "digest verifies the text"
        );
    });
}

/// §19.1: persona digest consistency — same text always produces the
/// same digest, so retry/restart/continue can verify they restore the
/// exact same persona by comparing digests.
#[test]
fn persona_digest_consistency_same_text_same_digest() {
    with_temp_db(|| {
        let text_a = "You are a reviewer.";
        let text_b = "You are a reviewer.";
        let text_c = "You are a different reviewer.";
        let digest_a = directive_sha256_hex(text_a);
        let digest_b = directive_sha256_hex(text_b);
        let digest_c = directive_sha256_hex(text_c);
        assert_eq!(digest_a, digest_b, "same text => same digest");
        assert_ne!(digest_a, digest_c, "different text => different digest");
        // SHA-256 hex is 64 chars.
        assert_eq!(digest_a.len(), 64);
    });
}

/// §19.1: the durable directive survives session status changes —
/// closing and re-reading the session does not lose the persona text.
/// (The scope snapshot is a durable column, not an in-memory field.)
#[test]
fn durable_directive_survives_session_status_change() {
    with_temp_db(|| {
        let binding = RouteBinding {
            provider_id: "openai".into(),
            key_id: "k1".into(),
            model_id: "gpt-4o".into(),
        };
        let (sid, _) = create_hidden_child_session(
            "parent-1",
            Some("parent-run"),
            None,
            "worker",
            "do",
            &binding,
            Some("ask"),
            None,
        )
        .unwrap();
        let text = "Persistent persona across status changes.";
        let digest = directive_sha256_hex(text);
        let scope_snapshot = with_pending_directive(json!({}), Some((text, &digest)));
        let mut res = SubagentReservation {
            session_id: sid.clone(),
            parent_run_id: "parent-run".into(),
            tree_root_run_id: "tree-root".into(),
            depth: 1,
            max_tokens: Some(1_000),
            max_cost_usd: None,
            failure_policy: "fail_fast".into(),
            max_retries: 2,
            scope_snapshot,
        };
        reserve_subagent_slot(&res).unwrap();
        // Change status to running, then completed — the directive
        // text is still readable (durable column, not in-memory).
        update_subagent_session_status(&sid, "running", None).unwrap();
        update_subagent_session_status(&sid, "completed", None).unwrap();
        let pd = pending_directive_for_session(&sid)
            .unwrap()
            .expect("directive survives status changes");
        assert_eq!(pd.text, text);
        assert_eq!(pd.digest, digest);
    });
}

/// §19.3/§19.5: redact_session_for_export hides the directive text and
/// the scope snapshot from the RPC surface — only the digest marker
/// is visible. This is field-level visibility for logs/exports.
#[test]
fn redact_session_export_for_subagent_list_hides_directive() {
    with_temp_db(|| {
        let binding = RouteBinding {
            provider_id: "openai".into(),
            key_id: "k1".into(),
            model_id: "gpt-4o".into(),
        };
        let (sid, _) = create_hidden_child_session(
            "parent-1",
            Some("parent-run"),
            None,
            "worker",
            "do",
            &binding,
            Some("ask"),
            None,
        )
        .unwrap();
        let secret_text = "secret persona that must never appear in exports";
        let digest = directive_sha256_hex(secret_text);
        let scope_snapshot = with_pending_directive(json!({}), Some((secret_text, &digest)));
        let mut res = SubagentReservation {
            session_id: sid.clone(),
            parent_run_id: "parent-run".into(),
            tree_root_run_id: "tree-root".into(),
            depth: 1,
            max_tokens: Some(1_000),
            max_cost_usd: None,
            failure_policy: "fail_fast".into(),
            max_retries: 2,
            scope_snapshot,
        };
        reserve_subagent_slot(&res).unwrap();
        let sess = get_subagent_session(&sid).unwrap().unwrap();
        let export = redact_session_for_export(&sess);
        let serialized = export.to_string();
        // The directive text never appears in the export.
        assert!(
            !serialized.contains(secret_text),
            "directive text must not leak through subagent.list export"
        );
        // The digest marker IS visible for field-level verification.
        assert!(
            serialized.contains(&digest),
            "digest stays visible for verification"
        );
        // The scope_snapshot_json field is redacted (no raw text).
        let snapshot_json = export["scope_snapshot_json"].as_str().unwrap_or("");
        assert!(
            !snapshot_json.contains(secret_text),
            "scope_snapshot_json must not carry raw directive text"
        );
    });
}

/// §19.1: the in-memory directive (run_agent_directives) is the
/// transient copy; the durable copy is the scope snapshot. A session
/// created WITHOUT a persisted directive returns None — callers fail
/// closed rather than inventing a persona (legacy session).
#[test]
fn session_without_persisted_directive_returns_none() {
    with_temp_db(|| {
        let binding = RouteBinding {
            provider_id: "openai".into(),
            key_id: "k1".into(),
            model_id: "gpt-4o".into(),
        };
        let (sid, _) = create_hidden_child_session(
            "parent-1",
            None,
            None,
            "worker",
            "do",
            &binding,
            Some("ask"),
            None,
        )
        .unwrap();
        // No reservation with directive → None (legacy session).
        let pd = pending_directive_for_session(&sid).unwrap();
        assert!(
            pd.is_none(),
            "legacy session without directive returns None"
        );
    });
}
