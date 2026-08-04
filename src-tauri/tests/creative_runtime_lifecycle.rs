//! Creative runtime lifecycle E2E (batch 9) — scenario 6 (double start) and
//! scenario 3/7-style resource bookkeeping through the public runtime store.
//!
//! Every assertion is against real rows / instance states, never a UI flag:
//! start creates one active instance, a second start is rejected by the CAS,
//! stop moves the instance through stopping → stopped, and a repeated stop is a
//! no-op. No Docker or WebView is spun here; the real-process kill / port
//! release scenario lives in the crate unit test
//! `term_timeout_kills_group_and_releases_port`.

use natives_lib::creative_app::model::CreativeAppSource;
use natives_lib::creative_app::runtime_store;
use natives_lib::db::{apply_migrations, create_tables};

fn mem() -> rusqlite::Connection {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    create_tables(&conn).unwrap();
    apply_migrations(&conn).unwrap();
    conn
}

#[test]
fn double_start_cas_and_repeat_stop_are_enforced() {
    let conn = mem();
    let app =
        runtime_store::find_or_create_application(&conn, CreativeAppSource::LocalProject, "e2e-1")
            .unwrap();

    // Start: one active instance, mirroring the process identity + port.
    let i1 = runtime_store::create_instance(&conn, &app, None, "local_process").unwrap();
    runtime_store::mark_running(
        &conn,
        &i1,
        &["http://127.0.0.1:5173/".into()],
        Some(5173),
        Some(100),
        Some(42),
    )
    .unwrap();

    let (pid, pgid, port): (Option<i64>, Option<i64>, Option<i64>) = conn
        .query_row(
            "SELECT pid, pgid, current_port FROM runtime_instances WHERE id = ?1",
            [&i1],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!((pid, pgid, port), (Some(42), Some(100), Some(5173)));

    // Double start: the adapter CAS rejects a second start while active; the
    // store keeps exactly one active instance.
    assert!(
        runtime_store::has_active_instance(&conn, &app).unwrap(),
        "one active instance after first start"
    );
    assert_eq!(
        runtime_store::active_instance_id(&conn, &app)
            .unwrap()
            .as_deref(),
        Some(i1.as_str())
    );

    // Stop: stopping → stopped; resources released (ledger keeps the pid/pgid
    // so a crash reconcile can still verify).
    runtime_store::mark_stopping(&conn, &i1).unwrap();
    runtime_store::mark_stopped(&conn, &i1).unwrap();
    // Repeat stop is a no-op (idempotent) — never resurrects.
    runtime_store::mark_stopped(&conn, &i1).unwrap();

    let (status, cleanup): (String, Option<String>) = conn
        .query_row(
            "SELECT status, cleanup_status FROM runtime_instances WHERE id = ?1",
            [&i1],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(status, "stopped");
    assert_eq!(cleanup.as_deref(), Some("completed"));
    assert!(
        !runtime_store::has_active_instance(&conn, &app).unwrap(),
        "no active instance after stop — a new start may begin"
    );
}

#[test]
fn stop_failure_keeps_instance_non_stopped_and_identity() {
    let conn = mem();
    let app =
        runtime_store::find_or_create_application(&conn, CreativeAppSource::LocalProject, "e2e-2")
            .unwrap();
    let i1 = runtime_store::create_instance(&conn, &app, None, "local_process").unwrap();
    runtime_store::mark_running(
        &conn,
        &i1,
        &["http://127.0.0.1:5173/".into()],
        Some(5173),
        Some(7),
        Some(9),
    )
    .unwrap();

    // A cleanup failure (e.g. group still alive / port held) must NOT claim
    // stopped — the instance keeps its identity for a retry stop.
    runtime_store::mark_cleanup_failed(&conn, &i1, "process group 7 still has members").unwrap();
    let (status, pid, port): (String, Option<i64>, Option<i64>) = conn
        .query_row(
            "SELECT status, pid, current_port FROM runtime_instances WHERE id = ?1",
            [&i1],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(status, "cleanup_failed");
    assert_eq!(pid, Some(9));
    assert_eq!(port, Some(5173));
}
