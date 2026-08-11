use super::*;

fn table_exists(conn: &Connection, name: &str) -> bool {
    conn.query_row(
        "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name=?1",
        rusqlite::params![name],
        |row| row.get(0),
    )
    .unwrap_or(false)
}

/// The Host-owned provider mirror schema applies to a natives.db connection:
/// the mirror tables (provider configs / keys / model cache / projects) are
/// created, and the Daemon-authority `assistant_*` runtime schema is NOT.
#[test]
fn ensure_provider_mirror_schema_creates_host_owned_tables_only() {
    let conn = Connection::open_in_memory().unwrap();
    ensure_provider_mirror_schema(&conn).expect("mirror schema applies to natives.db");

    // Host-owned tables must exist.
    for table in [
        "assistant_provider_configs",
        "assistant_provider_keys",
        "assistant_model_cache",
        "assistant_projects",
    ] {
        assert!(
            table_exists(&conn, table),
            "Host-owned table {table} must exist on natives.db"
        );
    }

    // Daemon-authority runtime tables must NOT be created by the Host.
    for table in [
        "conversation",
        "message",
        "assistant_conversations",
        "assistant_messages",
        "assistant_message_blocks",
        "assistant_runs",
        "assistant_run_events",
        "assistant_prompt_queue",
    ] {
        assert!(
            !table_exists(&conn, table),
            "Host must not own assistant runtime schema: {table} was created"
        );
    }
}

/// 回归：`ensure_provider_mirror_schema` 每次 natives.db init 都会执行，
/// 必须幂等。此前 MIGRATION_007 无条件 `ALTER TABLE ... ADD COLUMN website_url`，
/// 列已存在时（上次启动已加列，或 crud.rs 的 CREATE TABLE IF NOT EXISTS 已含该列）
/// 第二次启动即报 duplicate column name: website_url。
#[test]
fn ensure_provider_mirror_schema_is_idempotent_on_second_run() {
    let conn = Connection::open_in_memory().unwrap();
    ensure_provider_mirror_schema(&conn).expect("first apply succeeds");
    // 第二次执行模拟下一次启动：不得因重复补列失败。
    ensure_provider_mirror_schema(&conn).expect("second apply must be idempotent");
}

/// R-D3 regression: no active migration constant may DROP or rename-rebuild
/// a table. What remains is CREATE TABLE IF NOT EXISTS / ALTER / dedup only.
#[test]
fn no_active_migration_drops_tables() {
    let active: Vec<(&str, &str)> = vec![
        ("004", MIGRATION_004_PROVIDERS),
        ("005", MIGRATION_005),
        ("007", MIGRATION_007),
        ("008", MIGRATION_008),
        ("012", MIGRATION_012),
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

/// Legacy provider rows migrate within the Host-owned natives.db: source
/// (user_providers / provider_api_keys) and target (assistant_provider_configs /
/// assistant_provider_keys / assistant_model_cache) are the same DB, so no
/// cross-db access is involved.
#[test]
fn legacy_provider_keys_migrate_within_natives_db() {
    let mut conn = Connection::open_in_memory().unwrap();
    ensure_provider_mirror_schema(&conn).expect("mirror schema applies");
    conn.execute_batch(
        "CREATE TABLE user_providers (
            id TEXT PRIMARY KEY,
            preset_name TEXT,
            name TEXT,
            base_url TEXT,
            website_url TEXT,
            default_model TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
         );
         CREATE TABLE provider_api_keys (
            id TEXT PRIMARY KEY,
            provider_id TEXT NOT NULL,
            api_key_encrypted TEXT NOT NULL,
            label TEXT,
            is_active INTEGER NOT NULL DEFAULT 1,
            is_primary INTEGER NOT NULL DEFAULT 0,
            test_status TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT
         );
         INSERT INTO user_providers
           (id, preset_name, name, base_url, website_url, default_model, created_at, updated_at)
         VALUES
           ('p1', 'openai', 'OpenAI', 'https://api.openai.com/v1', '', 'gpt-4o',
            '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'),
           ('p2', 'anthropic', 'Anthropic', 'https://api.anthropic.com', '', 'claude-3-5-sonnet',
            '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');
         INSERT INTO provider_api_keys
           (id, provider_id, api_key_encrypted, label, is_active, is_primary, test_status, created_at, updated_at)
         VALUES
           ('k1', 'p1', 'sk-1234567890abcdef', 'primary', 1, 1, 'valid',
            '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'),
           ('k2', 'p2', 'sk-ant-abcdef1234567890', 'primary', 1, 1, 'valid',
            '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');",
    )
    .unwrap();

    migrate_legacy_provider_keys(&mut conn).expect("natives-only provider migration runs");

    let providers: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM assistant_provider_configs",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        providers, 2,
        "user_providers must migrate into the natives.db mirror"
    );
    let keys: i64 = conn
        .query_row("SELECT COUNT(*) FROM assistant_provider_keys", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(
        keys, 2,
        "provider_api_keys must migrate into the natives.db mirror"
    );
    let cache: i64 = conn
        .query_row("SELECT COUNT(*) FROM assistant_model_cache", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(
        cache, 2,
        "default models must seed the natives.db model cache"
    );

    // The migration is idempotent: a second run adds nothing.
    migrate_legacy_provider_keys(&mut conn).expect("second run is a no-op");
    let providers_again: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM assistant_provider_configs",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(providers_again, 2, "re-run must not duplicate mirror rows");
}

/// With no legacy tables present, the natives-only migration is a no-op.
#[test]
fn provider_keys_migration_noops_without_legacy_tables() {
    let mut conn = Connection::open_in_memory().unwrap();
    ensure_provider_mirror_schema(&conn).expect("mirror schema applies");
    migrate_legacy_provider_keys(&mut conn).expect("no-op when no legacy tables");
    let providers: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM assistant_provider_configs",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(providers, 0);
}

/// Foreign keys on Host-owned mirror tables are enforced.
#[test]
fn test_foreign_keys_enforced() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
    ensure_provider_mirror_schema(&conn).expect("mirror schema applies");

    // Try to insert a provider key with a non-existent provider_id
    let result = conn.execute(
        "INSERT INTO assistant_provider_keys
                (id, provider_id, encrypted_key, masked_key, created_at)
             VALUES ('k1', 'nonexistent', 'enc', '***', '2024-01-01T00:00:00Z')",
        [],
    );
    assert!(result.is_err());
}
