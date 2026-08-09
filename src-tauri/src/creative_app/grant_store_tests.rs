use super::*;
use crate::db;
use std::sync::{Arc, Barrier};

fn fixture() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    db::create_tables(&conn).unwrap();
    db::apply_migrations(&conn).unwrap();
    conn
}

fn file_fixture(dir: &std::path::Path) -> Connection {
    let conn = Connection::open(dir.join("test.db")).unwrap();
    conn.busy_timeout(std::time::Duration::from_secs(10))
        .unwrap();
    db::create_tables(&conn).unwrap();
    db::apply_migrations(&conn).unwrap();
    conn
}

fn ensure_app(conn: &Connection, id: &str) {
    conn.execute(
            "INSERT OR IGNORE INTO applications (id, source, source_id, title, version, created_at, updated_at)
             VALUES (?1, 'local_project', ?1, 'Test', '1', 't', 't')",
            params![id],
        )
        .unwrap();
}

#[test]
fn oauth_domain_add_and_check() {
    let conn = fixture();
    ensure_app(&conn, "app-1");
    add_oauth_domain(&conn, "app-1", "accounts.google.com").unwrap();
    assert!(is_oauth_domain_allowed(&conn, "app-1", "accounts.google.com").unwrap());
    assert!(!is_oauth_domain_allowed(&conn, "app-1", "evil.com").unwrap());
}

#[test]
fn grant_default_deny() {
    let conn = fixture();
    ensure_app(&conn, "app-1");
    assert!(!is_grant_allowed(&conn, "app-1", AppGrant::KIND_CLIPBOARD).unwrap());
}

#[test]
fn grant_persistent_allowed() {
    let conn = fixture();
    ensure_app(&conn, "app-1");
    set_grant(
        &conn,
        "app-1",
        AppGrant::KIND_UPLOAD,
        AppGrant::POLICY_PERSISTENT,
        None,
    )
    .unwrap();
    assert!(is_grant_allowed(&conn, "app-1", AppGrant::KIND_UPLOAD).unwrap());
}

#[test]
fn grant_one_time_consumed() {
    let conn = fixture();
    ensure_app(&conn, "app-1");
    set_grant(
        &conn,
        "app-1",
        AppGrant::KIND_DOWNLOAD,
        AppGrant::POLICY_ONE_TIME,
        None,
    )
    .unwrap();
    assert!(is_grant_allowed(&conn, "app-1", AppGrant::KIND_DOWNLOAD).unwrap());
    // Second call should return false (one-time consumed)
    assert!(!is_grant_allowed(&conn, "app-1", AppGrant::KIND_DOWNLOAD).unwrap());
}

#[test]
fn grant_list_and_delete() {
    let conn = fixture();
    ensure_app(&conn, "app-1");
    set_grant(
        &conn,
        "app-1",
        AppGrant::KIND_CLIPBOARD,
        AppGrant::POLICY_PERSISTENT,
        None,
    )
    .unwrap();
    let grants = list_grants(&conn, "app-1").unwrap();
    assert_eq!(grants.len(), 1);
    delete_grant(&conn, &grants[0].id).unwrap();
    assert!(list_grants(&conn, "app-1").unwrap().is_empty());
}

// ── T08: check_grant / atomicity / path scope / history ─────────────

#[test]
fn check_grant_denies_all_four_kinds_by_default() {
    let conn = fixture();
    ensure_app(&conn, "app-1");
    for kind in [
        AppGrant::KIND_CLIPBOARD,
        AppGrant::KIND_UPLOAD,
        AppGrant::KIND_DOWNLOAD,
        AppGrant::KIND_WINDOW_OPEN,
    ] {
        assert_eq!(
            check_grant(&conn, "app-1", kind, None).unwrap(),
            GrantOutcome::DeniedNoGrant,
            "kind {kind} must default to deny"
        );
    }
}

#[test]
fn check_grant_one_time_consumed_atomically_sequential() {
    let conn = fixture();
    ensure_app(&conn, "app-1");
    set_grant(
        &conn,
        "app-1",
        AppGrant::KIND_CLIPBOARD,
        AppGrant::POLICY_ONE_TIME,
        None,
    )
    .unwrap();
    assert_eq!(
        check_grant(&conn, "app-1", AppGrant::KIND_CLIPBOARD, None).unwrap(),
        GrantOutcome::AllowedOneTimeConsumed
    );
    // Follow-up check sees the consumed row (policy is now default_deny).
    assert!(!check_grant(&conn, "app-1", AppGrant::KIND_CLIPBOARD, None)
        .unwrap()
        .allowed());
    // The row is now default_deny (durable).
    assert_eq!(
        get_grant(&conn, "app-1", AppGrant::KIND_CLIPBOARD)
            .unwrap()
            .policy,
        AppGrant::POLICY_DEFAULT_DENY
    );
}

#[test]
fn one_time_grant_concurrent_consumption_only_one_succeeds() {
    // File-backed DB so two connections can race the same row.
    let dir = tempfile::tempdir().unwrap();
    {
        let conn = file_fixture(dir.path());
        ensure_app(&conn, "app-conc");
        set_grant(
            &conn,
            "app-conc",
            AppGrant::KIND_CLIPBOARD,
            AppGrant::POLICY_ONE_TIME,
            None,
        )
        .unwrap();
    }

    let barrier = Arc::new(Barrier::new(3));
    let b1 = barrier.clone();
    let b2 = barrier.clone();

    let p1 = dir.path().join("test.db");
    let p2 = dir.path().join("test.db");
    let h1 = std::thread::spawn(move || {
        let conn = Connection::open(p1).unwrap();
        conn.busy_timeout(std::time::Duration::from_secs(10))
            .unwrap();
        b1.wait();
        check_grant(&conn, "app-conc", AppGrant::KIND_CLIPBOARD, None)
    });
    let h2 = std::thread::spawn(move || {
        let conn = Connection::open(p2).unwrap();
        conn.busy_timeout(std::time::Duration::from_secs(10))
            .unwrap();
        b2.wait();
        check_grant(&conn, "app-conc", AppGrant::KIND_CLIPBOARD, None)
    });
    barrier.wait();

    let r1 = h1.join().unwrap().unwrap();
    let r2 = h2.join().unwrap().unwrap();
    let allowed = [r1, r2].iter().filter(|o| o.allowed()).count();
    assert_eq!(allowed, 1, "only one concurrent one-time check may succeed");
}

#[test]
fn check_grant_persistent_path_scope() {
    let conn = fixture();
    ensure_app(&conn, "app-1");
    set_grant(
        &conn,
        "app-1",
        AppGrant::KIND_DOWNLOAD,
        AppGrant::POLICY_PERSISTENT,
        Some("/Users/me/Downloads/AppA"),
    )
    .unwrap();
    assert_eq!(
        check_grant(
            &conn,
            "app-1",
            AppGrant::KIND_DOWNLOAD,
            Some("/Users/me/Downloads/AppA/report.pdf")
        )
        .unwrap(),
        GrantOutcome::AllowedPersistent
    );
    assert_eq!(
        check_grant(
            &conn,
            "app-1",
            AppGrant::KIND_DOWNLOAD,
            Some("/Users/me/Downloads/Other/evil.pdf")
        )
        .unwrap(),
        GrantOutcome::DeniedScopeMismatch
    );
    // Missing requested path with a scoped grant must be denied.
    assert_eq!(
        check_grant(&conn, "app-1", AppGrant::KIND_DOWNLOAD, None).unwrap(),
        GrantOutcome::DeniedScopeMismatch
    );
}

#[test]
fn check_grant_one_time_scope_mismatch_still_consumes() {
    let conn = fixture();
    ensure_app(&conn, "app-1");
    set_grant(
        &conn,
        "app-1",
        AppGrant::KIND_UPLOAD,
        AppGrant::POLICY_ONE_TIME,
        Some("/tmp/app-scope"),
    )
    .unwrap();
    assert_eq!(
        check_grant(&conn, "app-1", AppGrant::KIND_UPLOAD, Some("/etc/passwd")).unwrap(),
        GrantOutcome::DeniedScopeMismatch
    );
    // The one-time attempt was spent: a follow-up with a valid path is denied.
    assert!(!check_grant(
        &conn,
        "app-1",
        AppGrant::KIND_UPLOAD,
        Some("/tmp/app-scope/f")
    )
    .unwrap()
    .allowed());
}

#[test]
fn denied_checks_leave_one_time_grants_unconsumed() {
    // Unauthorized checks must have no side effects: no grant consumed,
    // no policy row mutated.
    let conn = fixture();
    ensure_app(&conn, "app-1");
    set_grant(
        &conn,
        "app-1",
        AppGrant::KIND_CLIPBOARD,
        AppGrant::POLICY_ONE_TIME,
        None,
    )
    .unwrap();
    // Checking the wrong kind must not touch the clipboard grant.
    assert_eq!(
        check_grant(&conn, "app-1", AppGrant::KIND_UPLOAD, None).unwrap(),
        GrantOutcome::DeniedNoGrant
    );
    assert_eq!(
        get_grant(&conn, "app-1", AppGrant::KIND_CLIPBOARD)
            .unwrap()
            .policy,
        AppGrant::POLICY_ONE_TIME,
        "one-time grant must survive an unrelated denied check"
    );
}

#[test]
fn is_path_within_scope_blocks_traversal() {
    assert!(is_path_within_scope("/a/b", "/a"));
    assert!(is_path_within_scope("/a", "/a"));
    assert!(!is_path_within_scope("/ab", "/a"));
    assert!(!is_path_within_scope("/a/../etc", "/a"));
    assert!(!is_path_within_scope("/etc/passwd", "/a"));
    assert!(is_path_within_scope("/a/b/../c", "/a"));
    assert!(is_path_within_scope("C:/a/b", "C:/a"));
}

#[test]
fn grant_history_records_set_consume_revoke() {
    let conn = fixture();
    ensure_app(&conn, "app-1");
    set_grant(
        &conn,
        "app-1",
        AppGrant::KIND_CLIPBOARD,
        AppGrant::POLICY_ONE_TIME,
        None,
    )
    .unwrap();
    assert_eq!(
        check_grant(&conn, "app-1", AppGrant::KIND_CLIPBOARD, None).unwrap(),
        GrantOutcome::AllowedOneTimeConsumed
    );
    let grants = list_grants(&conn, "app-1").unwrap();
    delete_grant(&conn, &grants[0].id).unwrap();

    let events = list_grant_events(&conn, "app-1", 10).unwrap();
    let kinds: Vec<&str> = events.iter().map(|e| e.event.as_str()).collect();
    assert_eq!(kinds, vec!["revoked", "consumed", "set"]);
}

#[test]
fn grant_policy_survives_reopen() {
    // Restart simulation: file-backed DB, consume one-time, reopen, assert
    // persistent stays allowed and one-time is default_deny.
    let dir = tempfile::tempdir().unwrap();
    {
        let conn = file_fixture(dir.path());
        ensure_app(&conn, "app-restart");
        set_grant(
            &conn,
            "app-restart",
            AppGrant::KIND_CLIPBOARD,
            AppGrant::POLICY_PERSISTENT,
            None,
        )
        .unwrap();
        set_grant(
            &conn,
            "app-restart",
            AppGrant::KIND_DOWNLOAD,
            AppGrant::POLICY_ONE_TIME,
            None,
        )
        .unwrap();
        assert_eq!(
            check_grant(&conn, "app-restart", AppGrant::KIND_DOWNLOAD, None).unwrap(),
            GrantOutcome::AllowedOneTimeConsumed
        );
    }
    let conn = Connection::open(dir.path().join("test.db")).unwrap();
    db::create_tables(&conn).unwrap();
    db::apply_migrations(&conn).unwrap();
    assert_eq!(
        check_grant(&conn, "app-restart", AppGrant::KIND_CLIPBOARD, None).unwrap(),
        GrantOutcome::AllowedPersistent,
        "persistent grant must survive restart"
    );
    assert!(
        !check_grant(&conn, "app-restart", AppGrant::KIND_DOWNLOAD, None)
            .unwrap()
            .allowed(),
        "consumed one-time grant must stay denied after restart"
    );
}
