use super::*;
use crate::db::{apply_migrations, create_tables};

fn mem() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    create_tables(&conn).unwrap();
    apply_migrations(&conn).unwrap();
    conn
}

fn count(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |r| r.get(0)).unwrap()
}

#[test]
fn identity_is_idempotent_and_stable() {
    let conn = mem();
    let a1 = find_or_create_application(&conn, CreativeAppSource::LocalProject, "loc1").unwrap();
    let a2 = find_or_create_application(&conn, CreativeAppSource::LocalProject, "loc1").unwrap();
    assert_eq!(a1, a2, "same source id must map to one application id");
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM applications"), 1);
}

#[test]
fn start_cas_keeps_one_active_instance() {
    let conn = mem();
    let app = find_or_create_application(&conn, CreativeAppSource::LocalProject, "loc1").unwrap();
    assert!(!has_active_instance(&conn, &app).unwrap());

    let i1 = create_instance(&conn, &app, None, "local_process").unwrap();
    assert!(has_active_instance(&conn, &app).unwrap());
    assert_eq!(
        active_instance_id(&conn, &app).unwrap().as_deref(),
        Some(i1.as_str())
    );

    // The adapter CAS rejects a second start while an instance is active;
    // the store contract is "at most one active" — active_instance_id points
    // at the running one and has_active_instance stays true.
    assert!(has_active_instance(&conn, &app).unwrap());

    mark_stopping(&conn, &i1).unwrap();
    mark_stopped(&conn, &i1).unwrap();
    assert!(!has_active_instance(&conn, &app).unwrap());
    assert!(active_instance_id(&conn, &app).unwrap().is_none());
}

#[test]
fn restart_creates_new_instance_and_old_is_completed() {
    let conn = mem();
    let app = find_or_create_application(&conn, CreativeAppSource::LocalProject, "loc1").unwrap();
    let i1 = create_instance(&conn, &app, None, "local_process").unwrap();
    mark_running(
        &conn,
        &i1,
        &["http://127.0.0.1:5173/".into()],
        Some(5173),
        Some(100),
        Some(42),
    )
    .unwrap();
    // Restart = stop old then start new.
    mark_stopping(&conn, &i1).unwrap();
    mark_stopped(&conn, &i1).unwrap();

    let i2 = create_instance(&conn, &app, None, "local_process").unwrap();
    assert_eq!(
        active_instance_id(&conn, &app).unwrap().as_deref(),
        Some(i2.as_str())
    );

    let old_status: String = conn
        .query_row(
            "SELECT status FROM runtime_instances WHERE id = ?1",
            params![i1],
            |r| r.get(0),
        )
        .unwrap();
    let old_cleanup: Option<String> = conn
        .query_row(
            "SELECT cleanup_status FROM runtime_instances WHERE id = ?1",
            params![i1],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(old_status, "stopped");
    assert_eq!(old_cleanup.as_deref(), Some("completed"));
}

#[test]
fn attach_identity_binds_summary_and_active_instance() {
    let conn = mem();
    let summary = CreativeAppSummary {
        id: "loc1".into(),
        application_id: String::new(),
        runtime_instance_id: None,
        source: CreativeAppSource::LocalProject,
        runtime: CreativeAppRuntime::LocalStatic,
        title: "Local".into(),
        description: None,
        icon: None,
        version: "1".into(),
        state: CreativeAppState::InstalledStopped,
        open_url: None,
        repository_url: None,
        last_error: None,
        status_detail: None,
        local_project: None,
        actions: CreativeAppActions::default(),
    };
    // Identity is created by the registration write path, then attach_identity
    // (read-only) binds it onto the summary without ever fabricating one.
    let app = find_or_create_application(&conn, CreativeAppSource::LocalProject, "loc1").unwrap();
    let bound = attach_identity(&conn, summary.clone()).unwrap();
    assert_eq!(bound.application_id, app);
    assert!(bound.runtime_instance_id.is_none());

    let iid = create_instance(&conn, &app, None, "host_http").unwrap();
    let bound2 = attach_identity(&conn, summary).unwrap();
    assert_eq!(bound2.runtime_instance_id.as_deref(), Some(iid.as_str()));
}

/// Batch 3 CR-301: instance→application→source mapping routes late resource
/// events to the right owner. A stale runtime id resolves to its original
/// application even after a newer instance exists.
#[test]
fn instance_to_application_to_source_mapping() {
    let conn = mem();
    let app = find_or_create_application(&conn, CreativeAppSource::LocalProject, "loc1").unwrap();
    let i1 = create_instance(&conn, &app, None, "local_process").unwrap();
    assert_eq!(
        instance_application_id(&conn, &i1).unwrap().as_deref(),
        Some(app.as_str())
    );
    assert_eq!(
        source_id_for_application(&conn, &app).unwrap().as_deref(),
        Some("loc1")
    );
    // A later instance of the same app does not change the first mapping.
    mark_stopping(&conn, &i1).unwrap();
    mark_stopped(&conn, &i1).unwrap();
    let i2 = create_instance(&conn, &app, None, "local_process").unwrap();
    assert_eq!(
        instance_application_id(&conn, &i1).unwrap().as_deref(),
        Some(app.as_str())
    );
    assert_ne!(i1, i2);
    assert_eq!(
        instance_application_id(&conn, &i2).unwrap().as_deref(),
        Some(app.as_str())
    );

    // Unknown ids resolve to None (never fabricated).
    assert!(instance_application_id(&conn, "nope").unwrap().is_none());
}

/// Batch 3 CR-301: settle_instance_by_id touches exactly the named instance,
/// so a stale run's reconcile cannot clobber a newer active instance.
#[test]
fn settle_by_id_only_touches_named_instance() {
    let conn = mem();
    let app = find_or_create_application(&conn, CreativeAppSource::LocalProject, "loc1").unwrap();
    let i1 = create_instance(&conn, &app, None, "local_process").unwrap();
    mark_stopping(&conn, &i1).unwrap();
    mark_stopped(&conn, &i1).unwrap();
    let i2 = create_instance(&conn, &app, None, "local_process").unwrap();
    mark_running(
        &conn,
        &i2,
        &["http://127.0.0.1:5173/".into()],
        Some(5173),
        None,
        None,
    )
    .unwrap();

    settle_instance_by_id(&conn, &i1, "failed").unwrap();
    let (s1, s2): (String, String) = conn
        .query_row(
            "SELECT (SELECT status FROM runtime_instances WHERE id = ?1),
                        (SELECT status FROM runtime_instances WHERE id = ?2)",
            params![i1, i2],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(s1, "failed", "named instance must settle");
    assert_eq!(s2, "running", "newer instance must be untouched");
}

/// Batch 2 race guard: a start must never resurrect an instance that a
/// concurrent stop already settled to stopped — and a 0-row transition is
/// surfaced as a typed conflict instead of silent success (#07).
#[test]
fn mark_running_does_not_resurrect_stopped_instance() {
    let conn = mem();
    let app = find_or_create_application(&conn, CreativeAppSource::LocalProject, "loc1").unwrap();
    let iid = create_instance(&conn, &app, None, "local_process").unwrap();
    // Stop wins the race: instance is stopping then stopped.
    mark_stopping(&conn, &iid).unwrap();
    mark_stopped(&conn, &iid).unwrap();

    // Late start health pass must NOT flip it back to running; the guarded
    // update returns a typed conflict rather than an invisible 0-row Ok.
    let err = mark_running(
        &conn,
        &iid,
        &["http://127.0.0.1:5173/".into()],
        Some(5173),
        None,
        None,
    )
    .unwrap_err();
    assert!(
        matches!(err, Error::Conflict(_)),
        "late mark_running on a stopped instance must conflict, got {err:?}"
    );
    let status: String = conn
        .query_row(
            "SELECT status FROM runtime_instances WHERE id = ?1",
            params![iid],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        status, "stopped",
        "mark_running must be guarded on starting"
    );
}

/// Natural process exit settles the instance with its exit code.
#[test]
fn mark_exited_records_natural_exit() {
    let conn = mem();
    let app = find_or_create_application(&conn, CreativeAppSource::LocalProject, "loc1").unwrap();
    let iid = create_instance(&conn, &app, None, "local_process").unwrap();
    mark_running(&conn, &iid, &[], None, Some(7), Some(42)).unwrap();

    let (pid, owner_pid): (Option<i64>, Option<i64>) = conn
        .query_row(
            "SELECT pid, owner_pid FROM runtime_instances WHERE id = ?1",
            params![iid],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(pid, Some(42));
    assert_eq!(owner_pid, Some(42));

    mark_exited(&conn, &iid, 3).unwrap();
    let (status, code): (String, Option<i64>) = conn
        .query_row(
            "SELECT status, exit_code FROM runtime_instances WHERE id = ?1",
            params![iid],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(status, "stopped");
    assert_eq!(code, Some(3));
    assert!(active_instance_id(&conn, &app).unwrap().is_none());
}

/// Batch 1 CR-102: the partial unique index makes the second active instance
/// a DB-level conflict even when the caller skips the pre-check.
#[test]
fn create_instance_cas_rejects_second_active() {
    let conn = mem();
    let app = find_or_create_application(&conn, CreativeAppSource::LocalProject, "loc1").unwrap();
    let i1 = create_instance(&conn, &app, None, "local_process").unwrap();
    assert!(active_instance_id(&conn, &app).unwrap().as_deref() == Some(i1.as_str()));

    let err = create_instance(&conn, &app, None, "local_process").unwrap_err();
    assert!(
        matches!(err, Error::Conflict(_)),
        "second active instance must be a typed conflict, got {err:?}"
    );
}

/// Batch 1 CR-102 (#07): transitions out of the expected status surface as
/// typed conflicts; idempotent terminal transitions stay Ok; a missing row
/// is NotFound — never a silent 0-row success.
#[test]
fn transitions_surface_wrong_status_as_conflict() {
    let conn = mem();
    let app = find_or_create_application(&conn, CreativeAppSource::LocalProject, "loc1").unwrap();
    let iid = create_instance(&conn, &app, None, "local_process").unwrap();
    // Stop preempts the starting instance directly.
    mark_stopped(&conn, &iid).unwrap();
    assert_eq!(current_status(&conn, &iid).unwrap(), "stopped");

    // mark_failed only applies from 'starting'.
    let err = mark_failed(&conn, &iid, "boom").unwrap_err();
    assert!(
        matches!(err, Error::Conflict(_)),
        "mark_failed on a stopped instance must conflict, got {err:?}"
    );

    // Idempotent terminal transitions remain Ok.
    assert!(mark_stopped(&conn, &iid).is_ok());
    assert!(mark_exited(&conn, &iid, 1).is_ok());

    // A missing instance is NotFound, not a silent success.
    let err = mark_stopped(&conn, "no-such-id").unwrap_err();
    assert!(
        matches!(err, Error::NotFound(_)),
        "missing instance must be NotFound, got {err:?}"
    );
}

/// Batch 1 CR-102: two SQLite writers racing to start the same app — only
/// the first insert wins; the DB partial unique index rejects the second
/// (concurrent double-start from #06).
#[test]
fn two_connections_reject_second_active_instance() {
    let path = std::env::temp_dir().join(format!("natives-cas-two-conn-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let conn1 = Connection::open(&path).unwrap();
    conn1
        .busy_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    create_tables(&conn1).unwrap();
    apply_migrations(&conn1).unwrap();
    let conn2 = Connection::open(&path).unwrap();
    conn2
        .busy_timeout(std::time::Duration::from_secs(5))
        .unwrap();

    let app = find_or_create_application(&conn1, CreativeAppSource::LocalProject, "loc1").unwrap();
    let i1 = create_instance(&conn1, &app, None, "local_process").unwrap();
    assert!(active_instance_id(&conn1, &app).unwrap().as_deref() == Some(i1.as_str()));

    // The second writer cannot insert a second active instance for the same app.
    let err = create_instance(&conn2, &app, None, "local_process").unwrap_err();
    assert!(
        matches!(err, Error::Conflict(_)),
        "second writer must hit the DB CAS, got {err:?}"
    );
    let count: i64 = conn1
        .query_row(
            "SELECT COUNT(*) FROM runtime_instances
                 WHERE application_id = ?1 AND status IN ('starting','running','stopping')",
            params![app],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);

    drop(conn1);
    drop(conn2);
    let _ = std::fs::remove_file(&path);
}

/// Batch 1 CR-102 (invariant #3): cleanup_failed is active-like — it is
/// visible to has_active_instance / active_instance_id so a new start is
/// blocked until the resources are proven released or the user recovers.
#[test]
fn cleanup_failed_is_active_like_and_blocks_new_start() {
    let conn = mem();
    let app = find_or_create_application(&conn, CreativeAppSource::LocalProject, "loc1").unwrap();
    let iid = create_instance(&conn, &app, None, "local_process").unwrap();
    mark_running(&conn, &iid, &[], None, None, None).unwrap();
    mark_cleanup_failed(&conn, &iid, "port not released").unwrap();

    assert!(
        has_active_instance(&conn, &app).unwrap(),
        "cleanup_failed must count as active-like"
    );
    assert!(
        active_instance_id(&conn, &app).unwrap().as_deref() == Some(iid.as_str()),
        "cleanup_failed instance must be the active one"
    );

    // The instance can be re-stopped (retry cleanup) and then a new start is
    // allowed again.
    mark_stopping(&conn, &iid).unwrap();
    mark_stopped(&conn, &iid).unwrap();
    assert!(!has_active_instance(&conn, &app).unwrap());
}

/// Batch 1 CR-102: orphaned is active-like and blocks new starts until
/// reconcile proves the identity or the user resolves it.
#[test]
fn orphaned_is_active_like_and_blocks_new_start() {
    let conn = mem();
    let app = find_or_create_application(&conn, CreativeAppSource::LocalProject, "loc1").unwrap();
    let iid = create_instance(&conn, &app, None, "local_process").unwrap();
    mark_running(&conn, &iid, &[], None, Some(100), Some(42)).unwrap();
    mark_orphaned(&conn, &iid, "host ownership lost").unwrap();

    assert!(has_active_instance(&conn, &app).unwrap());
    assert!(active_instance_id(&conn, &app).unwrap().as_deref() == Some(iid.as_str()));
}

/// Batch 1 CR-103: upserting a plan writes the LaunchProfile v1 columns
/// (schema_version / driver_kind / ownership_mode) and keeps one active plan.
#[test]
fn upsert_active_plan_writes_versioned_columns_and_single_active() {
    let conn = mem();
    let app = find_or_create_application(&conn, CreativeAppSource::LocalProject, "loc1").unwrap();
    let plan = serde_json::json!({
        "schemaVersion": 1,
        "runtime": "node_dev_server",
        "program": "npm",
        "script": "dev",
    })
    .to_string();

    let id = upsert_active_plan(&conn, &app, &plan).unwrap();
    let profile = get_active_plan(&conn, &app).unwrap().expect("active plan");
    assert_eq!(profile.id, id);
    assert_eq!(profile.schema_version, 1);
    assert_eq!(profile.driver_kind, "node_dev_server");
    assert_eq!(profile.ownership_mode, "managed");
    assert!(profile.is_active);

    // An external docker_compose config is classified docker_compose.
    let ext = serde_json::json!({ "kind": "docker_compose", "projectName": "x" }).to_string();
    let _ = upsert_active_plan(&conn, &app, &ext).unwrap();
    let profile = get_active_plan(&conn, &app).unwrap().expect("active plan");
    assert_eq!(profile.driver_kind, "docker_compose");

    // Exactly one active plan per app.
    let active: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM startup_plans WHERE application_id = ?1 AND is_active = 1",
            params![app],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(active, 1);
}

/// Batch 1 CR-103: a legacy plan row with NULL versioned columns is upgraded
/// on read — driver_kind / schema_version / ownership_mode derived from the
/// stored plan JSON, no rewrite needed.
#[test]
fn get_active_plan_read_upgrades_legacy_null_columns() {
    let conn = mem();
    let app = find_or_create_application(&conn, CreativeAppSource::LocalProject, "loc1").unwrap();
    let plan = serde_json::json!({
        "schemaVersion": 1,
        "runtime": "static_http",
        "program": "internal",
    })
    .to_string();
    conn.execute(
            "INSERT INTO startup_plans (id, application_id, plan_version, plan_json, is_active, created_at, updated_at)
             VALUES ('legacy-plan', ?1, 1, ?2, 1, 't', 't')",
            params![app, plan],
        )
        .unwrap();

    let profile = get_active_plan(&conn, &app)
        .unwrap()
        .expect("legacy plan read");
    assert_eq!(profile.id, "legacy-plan");
    assert_eq!(profile.schema_version, 1);
    assert_eq!(profile.driver_kind, "local_static");
    assert_eq!(profile.ownership_mode, "managed");
}

/// Batch 1 CR-103: a LaunchPlan with an unsupported future schema version
/// fails closed; unknown extra fields are tolerated on read.
#[test]
fn parse_launch_plan_fails_closed_on_future_version() {
    let v1 = serde_json::json!({
        "schemaVersion": 1,
        "source": "rule",
        "projectKind": "html",
        "runtime": "static_http",
        "program": "internal",
        "cwdRelative": ".",
        "entryFile": "index.html",
        "args": [],
        "environmentKeys": [],
        "port": { "mode": "auto" },
        "openPath": "/",
        "healthPath": "/",
        "startupTimeoutMs": 60000,
        "autoOpen": true,
        "reason": "test",
        "someFutureField": true,
    })
    .to_string();
    let plan = parse_launch_plan(&v1).unwrap();
    assert_eq!(plan.schema_version, 1);
    assert_eq!(plan.runtime, LocalLaunchRuntime::StaticHttp);

    let future = serde_json::json!({
        "schemaVersion": 2,
        "source": "rule",
        "projectKind": "html",
        "runtime": "static_http",
        "program": "internal",
        "cwdRelative": ".",
        "port": { "mode": "auto" },
        "openPath": "/",
        "healthPath": "/",
        "startupTimeoutMs": 60000,
        "autoOpen": true,
        "reason": "test",
    })
    .to_string();
    let err = parse_launch_plan(&future).unwrap_err();
    assert!(
        matches!(err, Error::InvalidInput(_)),
        "future schema must fail closed, got {err:?}"
    );
}

/// Batch 6: a preview target binds to the running instance and clears on close.
#[test]
fn preview_target_binds_to_running_instance() {
    let conn = mem();
    let app = find_or_create_application(&conn, CreativeAppSource::LocalProject, "loc1").unwrap();
    let iid = create_instance(&conn, &app, None, "host_http").unwrap();
    // Not running yet → no preview target is active.
    assert!(
        active_preview_target(&conn, CreativeAppSource::LocalProject, "loc1")
            .unwrap()
            .is_none()
    );

    mark_running(
        &conn,
        &iid,
        &["http://127.0.0.1:5173/".into()],
        Some(5173),
        None,
        None,
    )
    .unwrap();
    let tid =
        upsert_preview_target(&conn, &iid, "http://127.0.0.1:5173/", "child_webview").unwrap();
    let active = active_preview_target(&conn, CreativeAppSource::LocalProject, "loc1")
        .unwrap()
        .expect("running app has a preview");
    assert_eq!(active.0, tid);
    assert_eq!(active.1, "http://127.0.0.1:5173/");

    // A new preview replaces the old one (at most one live preview).
    let tid2 = upsert_preview_target(
        &conn,
        &iid,
        "http://127.0.0.1:5173/#/other",
        "child_webview",
    )
    .unwrap();
    assert_ne!(tid, tid2);
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM preview_targets WHERE runtime_instance_id = ?1",
            params![iid],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);

    clear_preview_targets(&conn, &iid).unwrap();
    assert!(
        active_preview_target(&conn, CreativeAppSource::LocalProject, "loc1")
            .unwrap()
            .is_none()
    );
}
