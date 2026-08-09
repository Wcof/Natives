use super::*;

fn open_fresh() -> LegacyMigrationService {
    LegacyMigrationService::open(":memory:").expect("open migration service")
}

fn table_exists(conn: &Connection, name: &str) -> bool {
    conn.query_row(
        "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name=?1",
        rusqlite::params![name],
        |row| row.get(0),
    )
    .unwrap_or(false)
}

#[test]
fn test_service_opens_fresh_db() {
    let service = open_fresh();
    service.run().expect("one-way migration runs");
    assert!(service.conn().is_autocommit());
}

/// Negative assertion (MIG-004/DATA-002): the Host migration service does
/// NOT create the Daemon-authority `assistant_*` runtime schema. On a fresh
/// database, none of the conversation/run/message/event/queue tables exist.
#[test]
fn host_does_not_own_assistant_runtime_schema() {
    let service = open_fresh();
    service.run().unwrap();

    // Daemon-authority tables must NOT be created by the Host.
    for table in [
        "assistant_conversations",
        "assistant_messages",
        "assistant_message_blocks",
        "assistant_runs",
        "assistant_run_events",
        "assistant_tool_calls",
        "assistant_permission_requests",
        "assistant_artifacts",
        "assistant_context_snapshots",
        "assistant_prompt_queue",
    ] {
        assert!(
            !table_exists(&service.conn(), table),
            "Host must not own assistant runtime schema: {table} was created"
        );
    }

    // Host-owned tables still exist (provider mirror / projects / settings).
    // (`scheduled_tasks` / `task_runs` are created by the separate
    // `db::init_assistant_db` pool setup, not by this migration service.)
    for table in [
        "assistant_provider_configs",
        "assistant_provider_keys",
        "assistant_model_cache",
        "assistant_projects",
        "settings",
    ] {
        assert!(
            table_exists(&service.conn(), table),
            "Host-owned table {table} must exist"
        );
    }
}

/// Negative assertion: the Host no longer writes run lifecycle tables at
/// startup. A stale `assistant_runs` row (left by a pre-D2-01 version) is
/// left untouched by the migration service — run recovery is Daemon
/// authority (`run_manager.rs`), never the Host.
#[test]
fn host_never_writes_stale_run_state() {
    let tmp = std::env::temp_dir().join(format!("natives-host-legacy-{}.db", uuid::Uuid::new_v4()));
    {
        // Build an old-style DB: legacy run tables with a stale active run.
        let conn = rusqlite::Connection::open(&tmp).unwrap();
        conn.execute_batch(
                "PRAGMA foreign_keys=ON;
                 CREATE TABLE assistant_conversations (
                    id TEXT PRIMARY KEY,
                    mode TEXT NOT NULL DEFAULT 'chat',
                    project_id TEXT,
                    title TEXT NOT NULL DEFAULT '',
                    provider_id TEXT NOT NULL,
                    model_id TEXT NOT NULL,
                    permission_profile_id TEXT,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    archived_at TEXT
                 );
                 CREATE TABLE assistant_runs (
                    id TEXT PRIMARY KEY,
                    conversation_id TEXT NOT NULL,
                    status TEXT NOT NULL DEFAULT 'queued',
                    provider_id TEXT NOT NULL,
                    model_id TEXT NOT NULL,
                    started_at TEXT
                 );
                 CREATE TABLE assistant_run_events (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    run_id TEXT NOT NULL,
                    sequence INTEGER NOT NULL,
                    timestamp TEXT NOT NULL,
                    event_type TEXT NOT NULL,
                    payload TEXT NOT NULL
                 );
                 INSERT INTO assistant_conversations
                   (id, mode, title, provider_id, model_id, created_at, updated_at)
                 VALUES ('c1', 'agent', 't', 'p', 'm', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');
                 INSERT INTO assistant_runs (id, conversation_id, status, provider_id, model_id, started_at)
                 VALUES ('r1', 'c1', 'running', 'p', 'm', '2026-01-01T00:00:00Z');
                 INSERT INTO assistant_run_events (run_id, sequence, timestamp, event_type, payload)
                 VALUES ('r1', 1, '2026-01-01T00:00:00Z', 'text_delta', '{\"text\":\"partial\"}');",
            )
            .unwrap();
    }

    let service = LegacyMigrationService::open(&tmp.to_string_lossy()).expect("open");
    service.run().expect("migration runs");

    let conn = service.conn();
    // The stale run row and its events are untouched (Host no longer owns
    // run recovery — that is Daemon authority).
    let status: String = conn
        .query_row(
            "SELECT status FROM assistant_runs WHERE id='r1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        status, "running",
        "Host migration service must not rewrite stale run state"
    );
    let event_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM assistant_run_events WHERE run_id='r1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        event_count, 1,
        "Host migration service must not add recovery events"
    );

    drop(conn);
    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn test_migration_idempotency() {
    let service = open_fresh();
    // Running the one-way migration twice should be safe
    service.run().unwrap();
    service.run().unwrap();

    let version: i64 = service
        .conn()
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM _schema_version",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(version, 14);
}

/// One-way session → conversation conversion still works on old DBs: a
/// session-schema `assistant_messages` table is preserved and converted so
/// the Daemon's host_authority_migration can read the conversation shape.
#[test]
fn legacy_session_messages_are_converted_not_dropped() {
    let tmp =
        std::env::temp_dir().join(format!("natives-host-sessions-{}.db", uuid::Uuid::new_v4()));
    {
        let conn = rusqlite::Connection::open(&tmp).unwrap();
        conn.execute_batch(
                "CREATE TABLE assistant_sessions (
                    id TEXT PRIMARY KEY,
                    project_id TEXT,
                    title TEXT NOT NULL DEFAULT '',
                    model_id TEXT NOT NULL DEFAULT '',
                    provider_id TEXT NOT NULL DEFAULT '',
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    summary TEXT NOT NULL DEFAULT '',
                    token_used INTEGER NOT NULL DEFAULT 0,
                    status TEXT NOT NULL DEFAULT 'active'
                 );
                 CREATE TABLE assistant_messages (
                    id TEXT PRIMARY KEY,
                    session_id TEXT NOT NULL,
                    role TEXT NOT NULL,
                    content TEXT NOT NULL DEFAULT '',
                    tool_calls TEXT,
                    tool_result TEXT,
                    status TEXT NOT NULL DEFAULT '',
                    token_count INTEGER NOT NULL DEFAULT 0,
                    created_at TEXT NOT NULL,
                    sequence INTEGER NOT NULL DEFAULT 0
                 );
                 INSERT INTO assistant_sessions
                   (id, project_id, title, model_id, provider_id, created_at, updated_at)
                 VALUES ('s1', NULL, 'Old session', 'm', 'p', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');
                 INSERT INTO assistant_messages
                   (id, session_id, role, content, created_at, sequence)
                 VALUES ('m1', 's1', 'user', 'hello old', '2026-01-01T00:00:00Z', 1);",
            )
            .unwrap();
    }

    let service = LegacyMigrationService::open(&tmp.to_string_lossy()).expect("open");
    service.run().expect("migration runs");

    let conn = service.conn();
    // The original rows are preserved under legacy_assistant_messages.
    let preserved: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM legacy_assistant_messages",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(preserved, 1, "old rows must be preserved");
    // The conversation shape now holds the converted conversation + message.
    let conversations: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM assistant_conversations WHERE id='s1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(conversations, 1, "session must convert into a conversation");
    let messages: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM assistant_messages WHERE id='m1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(messages, 1, "session message must convert");

    drop(conn);
    let _ = std::fs::remove_file(&tmp);
}

/// R-D3 regression: no active migration constant may DROP or rename-rebuild
/// a table. What remains is CREATE TABLE IF NOT EXISTS / ALTER / dedup only.
#[test]
fn no_active_migration_drops_tables() {
    let active: Vec<(&str, &str)> = vec![
        ("004", MIGRATION_004_PROVIDERS),
        ("005", MIGRATION_005),
        ("006", MIGRATION_006),
        ("007", MIGRATION_007),
        ("008", MIGRATION_008),
        ("012", MIGRATION_012),
        ("014", MIGRATION_014),
    ];
    for (name, sql) in active {
        let upper = sql.to_ascii_uppercase();
        assert!(
            !upper.contains("DROP TABLE"),
            "migration v{name} must not contain DROP TABLE (R-D3)"
        );
        assert!(
            !upper.contains("RENAME TO"),
            "migration v{name} must not rename-rebuild tables (R-D3)"
        );
    }
}

/// Legacy tables from old installs stay readable as a one-way migration
/// source: re-running the migration service against a DB that already has
/// the conversation schema must leave both `assistant_conversations` and
/// `assistant_messages` intact.
#[test]
fn legacy_conversation_schema_is_left_intact_not_rebuilt() {
    let tmp = std::env::temp_dir().join(format!("natives-host-conv-{}.db", uuid::Uuid::new_v4()));
    {
        let conn = rusqlite::Connection::open(&tmp).unwrap();
        conn.execute_batch(
                "PRAGMA foreign_keys=ON;
                 CREATE TABLE assistant_conversations (
                    id TEXT PRIMARY KEY,
                    mode TEXT NOT NULL DEFAULT 'chat',
                    project_id TEXT,
                    title TEXT NOT NULL DEFAULT '',
                    provider_id TEXT NOT NULL,
                    model_id TEXT NOT NULL,
                    permission_profile_id TEXT,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    archived_at TEXT
                 );
                 CREATE TABLE assistant_messages (
                    id TEXT PRIMARY KEY,
                    conversation_id TEXT NOT NULL REFERENCES assistant_conversations(id) ON DELETE CASCADE,
                    parent_message_id TEXT,
                    role TEXT NOT NULL CHECK(role IN ('system','user','assistant')),
                    status TEXT NOT NULL DEFAULT 'complete',
                    input_tokens INTEGER DEFAULT 0,
                    output_tokens INTEGER DEFAULT 0,
                    reasoning_tokens INTEGER,
                    cost_usd REAL,
                    created_at TEXT NOT NULL
                 );",
            )
            .unwrap();
    }

    let service = LegacyMigrationService::open(&tmp.to_string_lossy()).expect("open");
    service.run().expect("migration runs");

    let conn = service.conn();
    assert!(
        table_exists(&conn, "assistant_conversations"),
        "legacy migration source table must stay readable"
    );
    assert!(
        table_exists(&conn, "assistant_messages"),
        "legacy migration source table must stay readable"
    );

    drop(conn);
    let _ = std::fs::remove_file(&tmp);
}

/// Foreign keys on Host-owned tables are enforced.
#[test]
fn test_foreign_keys_enforced() {
    let service = open_fresh();
    service.run().unwrap();

    // Try to insert a provider key with a non-existent provider_id
    let result = service.conn().execute(
        "INSERT INTO assistant_provider_keys
                (id, provider_id, encrypted_key, masked_key, created_at)
             VALUES ('k1', 'nonexistent', 'enc', '***', '2024-01-01T00:00:00Z')",
        [],
    );
    assert!(result.is_err());
}
