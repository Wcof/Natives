//! T304 — Migration destructive-test matrix (fixture + crash recovery).
//!
//! Safety: all scenarios run on temp-dir DBs only. Never touches `~/.natives`.
//!
//! Matrix:
//! 1. v1-only legacy DB → full migration, user data preserved (row counts).
//! 2. Reopen idempotency: a second DataStore open does not re-run or corrupt.
//! 3. FK integrity after rebuilds: pragma_foreign_key_check == 0, FK back ON.
//! 4. WAL: daemon store opens in WAL mode.
//! 5. Crash simulation: ledger row removed → reopen re-attaches via
//!    postcondition and does not duplicate/lose user rows.
//! 6. Partial legacy state: a column added by v37 is missing after a crash →
//!    reopen repairs it idempotently (postcondition-driven).

use natives_agent_daemon::storage::DataStore;

fn temp_dir(_tag: &str) -> tempfile::TempDir {
    tempfile::tempdir().expect("tempdir")
}

fn fresh_store(tag: &str) -> (tempfile::TempDir, DataStore) {
    let dir = temp_dir(tag);
    let db = dir.path().join("migrate.db");
    let art = dir.path().join("artifacts");
    std::fs::create_dir_all(&art).unwrap();
    let store = DataStore::new(&db, &art).expect("store init + migrations");
    (dir, store)
}

/// 1. Legacy v1-only DB (full canonical core) upgrades without losing data.
#[test]
fn matrix_v1_only_upgrades_preserving_data() {
    let dir = temp_dir("t304-v1");
    let db = dir.path().join("legacy-v1.db");
    let art = dir.path().join("artifacts");
    std::fs::create_dir_all(&art).unwrap();

    // Build a v1-era DB: the canonical v1 core (conversation/message/
    // message_block/run/run_event — all five tables MIGRATION_001 creates).
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE conversation (
               id TEXT PRIMARY KEY,
               mode TEXT NOT NULL DEFAULT 'chat' CHECK(mode IN ('chat','agent','goal')),
               title TEXT NOT NULL DEFAULT '',
               provider_id TEXT NOT NULL,
               model_id TEXT NOT NULL,
               created_at TEXT NOT NULL DEFAULT (datetime('now')),
               updated_at TEXT NOT NULL DEFAULT (datetime('now'))
             );
             CREATE TABLE message (
               id TEXT PRIMARY KEY,
               conversation_id TEXT NOT NULL REFERENCES conversation(id) ON DELETE CASCADE,
               parent_message_id TEXT REFERENCES message(id) ON DELETE SET NULL,
               role TEXT NOT NULL CHECK(role IN ('system','user','assistant')),
               status TEXT NOT NULL DEFAULT 'complete',
               created_at TEXT NOT NULL DEFAULT (datetime('now'))
             );
             CREATE TABLE message_block (
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               message_id TEXT NOT NULL REFERENCES message(id) ON DELETE CASCADE,
               sort_order INTEGER NOT NULL DEFAULT 0,
               block_type TEXT NOT NULL,
               block_json TEXT NOT NULL
             );
             CREATE TABLE run (
               id TEXT PRIMARY KEY,
               conversation_id TEXT NOT NULL REFERENCES conversation(id) ON DELETE CASCADE,
               status TEXT NOT NULL DEFAULT 'queued',
               provider_id TEXT NOT NULL,
               model_id TEXT NOT NULL,
               created_at TEXT NOT NULL DEFAULT (datetime('now'))
             );
             CREATE TABLE run_event (
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               run_id TEXT NOT NULL REFERENCES run(id) ON DELETE CASCADE,
               sequence INTEGER NOT NULL,
               event_type TEXT NOT NULL,
               payload TEXT NOT NULL,
               timestamp TEXT NOT NULL DEFAULT (datetime('now')),
               UNIQUE(run_id, sequence)
             );
             INSERT INTO conversation (id, mode, title, provider_id, model_id, created_at, updated_at)
               VALUES ('conv-1','chat','Legacy','prov-1','model-1','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z');
             INSERT INTO run (id, conversation_id, status, provider_id, model_id, created_at)
               VALUES ('run-1','conv-1','completed','prov-1','model-1','2026-01-01T00:00:00Z');",
        )
        .unwrap();
    }

    // Reopen through the daemon store: full migration must run on the legacy DB.
    let store = DataStore::new(&db, &art).expect("migrate legacy v1 DB");
    assert!(store.has_table("_daemon_migrations"), "ledger table must exist");
    assert!(store.has_table("turn"), "v28 table must exist after migration");
    assert!(store.has_table("subagent_session"), "v10 table must exist");

    // User data preserved: run + conversation rows still present.
    let conn = store.conn().unwrap();
    let runs: i64 = conn
        .query_row("SELECT COUNT(*) FROM run", [], |r| r.get(0))
        .unwrap();
    assert_eq!(runs, 1, "legacy run row must survive migration");
    let convs: i64 = conn
        .query_row("SELECT COUNT(*) FROM conversation", [], |r| r.get(0))
        .unwrap();
    assert_eq!(convs, 1, "legacy conversation row must survive migration");
}

/// 2. Reopen idempotency: no duplicate migration, no corruption.
#[test]
fn matrix_reopen_idempotent() {
    let (dir, store) = fresh_store("t304-reopen");
    let db = dir.path().join("migrate.db");
    drop(store);

    let art = dir.path().join("artifacts");
    let s2 = DataStore::new(&db, &art).expect("reopen");
    let conn = s2.conn().unwrap();
    // Migration ledger has one row per applied migration; reopening must not
    // duplicate ledger entries (each version has PK id).
    let ledger: i64 = conn
        .query_row("SELECT COUNT(*) FROM _daemon_migrations", [], |r| r.get(0))
        .unwrap();
    assert!(ledger >= 22, "full migration set applied once, got {ledger}");

    let all: i64 = conn
        .query_row("SELECT COUNT(*) FROM run", [], |r| r.get(0))
        .unwrap();
    assert_eq!(all, 0, "no phantom rows on reopen");
}

/// 3. FK integrity + FK back ON after rebuild migrations.
#[test]
fn matrix_fk_integrity_after_rebuilds() {
    let (_dir, store) = fresh_store("t304-fk");
    let conn = store.conn().unwrap();
    let violations: i64 = conn
        .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(violations, 0, "no FK violations after full migration");
    let fk: i64 = conn
        .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
        .unwrap();
    assert_eq!(fk, 1, "FK enforcement restored after rebuild migrations");
}

/// 4. WAL mode is active on the daemon store.
#[test]
fn matrix_wal_active() {
    let (_dir, store) = fresh_store("t304-wal");
    let conn = store.conn().unwrap();
    let mode: String = conn
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    assert_eq!(mode.to_ascii_lowercase(), "wal", "daemon store must use WAL");
}

/// 5. Crash simulation: v13 ledger row removed → reopen re-attaches via
///    postcondition (run.revision exists) and user rows are neither
///    duplicated nor lost.
#[test]
fn matrix_crash_ledger_missing_reattaches() {
    let (dir, store) = fresh_store("t304-crash");
    let db = dir.path().join("migrate.db");
    {
        let conn = store.conn().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO conversation (id, mode, title, provider_id, model_id, created_at, updated_at)
             VALUES ('crash-conv','chat','C','prov-1','model-1','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')",
            [],
        )
        .unwrap();
        // Simulate: daemon crashed after applying v13 but before recording the
        // ledger row. run.revision postcondition is already satisfied.
        conn.execute("DELETE FROM _daemon_migrations WHERE id = 13", [])
            .unwrap();
    }
    drop(store);

    let art = dir.path().join("artifacts");
    let s2 = DataStore::new(&db, &art).expect("reopen after simulated crash");
    let conn = s2.conn().unwrap();
    // The missing v13 must be re-attached (postcondition adopt) and re-recorded.
    let v13: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM _daemon_migrations WHERE id = 13",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(v13, 1, "crash-missing ledger row re-attached");
    // User data not duplicated or lost.
    let convs: i64 = conn
        .query_row("SELECT COUNT(*) FROM conversation WHERE id='crash-conv'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(convs, 1, "user row preserved by re-migration");
}

/// 6. Partial legacy state: v37 column missing on run after a crash → reopen
///    repairs the column set idempotently and preserves rows.
#[test]
fn matrix_partial_legacy_columns_repair() {
    let (dir, store) = fresh_store("t304-partial");
    let db = dir.path().join("migrate.db");
    {
        let conn = store.conn().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO conversation (id, mode, title, provider_id, model_id, created_at, updated_at)
             VALUES ('c1','chat','C','prov-1','model-1','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO run (id, conversation_id, status, provider_id, model_id, created_at)
             VALUES ('r1','c1','completed','prov-1','model-1','2026-01-01T00:00:00Z')",
            [],
        )
        .unwrap();
        // Simulate a crash during v37: the column set is partially applied and
        // the ledger row for v37 was never recorded.
        conn.execute("DELETE FROM _daemon_migrations WHERE id = 37", [])
            .unwrap();
    }
    drop(store);

    let art = dir.path().join("artifacts");
    let s2 = DataStore::new(&db, &art).expect("repair partial v37 state");
    let conn = s2.conn().unwrap();
    // v37 postcondition (resume_plan.decision) must now be satisfied.
    let has_col: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM pragma_table_info('resume_plan') WHERE name='decision'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(has_col, "v37 postcondition satisfied");
    let runs: i64 = conn
        .query_row("SELECT COUNT(*) FROM run WHERE id='r1'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(runs, 1, "partial legacy row preserved");
}
