//! Merge Host `assistant_*` tables into Daemon canonical tables inside assistant.db.
//!
//! Phase 0: both Host and Daemon may open the same assistant.db file. Host historically
//! wrote `assistant_conversations` / `assistant_messages` / `assistant_runs` /
//! `assistant_run_events` / `assistant_prompt_queue`. Daemon owns unprefixed
//! `conversation` / `message` / `run` / `run_event` / `prompt_queue`.
//!
//! Rules (scheme):
//! - Same Run ID: Daemon events win; Host messages win.
//! - Transactional; records `_host_authority_migration`.
//! - Idempotent: re-run uses INSERT OR IGNORE / ON CONFLICT DO NOTHING.
//! - On failure: mark status=failed and leave existing rows intact (caller may
//!   refuse execution capabilities).

use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

const MIGRATION_VERSION: i64 = 1;

#[derive(Debug, Default)]
pub struct HostAuthorityMigrationResult {
    pub conversations: u64,
    pub messages: u64,
    pub message_blocks: u64,
    pub runs: u64,
    pub events: u64,
    pub prompt_queue: u64,
    pub already_done: bool,
    pub errors: Vec<String>,
}

/// Returns true if Host legacy tables exist in this DB file.
pub fn has_host_assistant_tables(conn: &Connection) -> bool {
    table_exists(conn, "assistant_conversations")
}

fn table_exists(conn: &Connection, name: &str) -> bool {
    conn.query_row(
        "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name=?1",
        params![name],
        |row| row.get(0),
    )
    .unwrap_or(false)
}

fn migration_status(conn: &Connection) -> Option<(i64, String)> {
    if !table_exists(conn, "_host_authority_migration") {
        return None;
    }
    conn.query_row(
        "SELECT version, status FROM _host_authority_migration WHERE id = 1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .optional()
    .ok()
    .flatten()
}

/// Backup assistant.db next to itself before merging.
pub fn backup_assistant_db(path: &Path) -> Result<std::path::PathBuf, String> {
    if !path.exists() {
        return Err(format!("assistant.db not found: {}", path.display()));
    }
    let stamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
    let backup = path.with_extension(format!("db.backup.{stamp}"));
    std::fs::copy(path, &backup).map_err(|e| format!("backup failed: {e}"))?;
    Ok(backup)
}

/// Run host→daemon merge. Safe to call on every open; no-ops when already completed.
pub fn migrate_host_authority(conn: &Connection) -> Result<HostAuthorityMigrationResult, String> {
    let mut result = HostAuthorityMigrationResult::default();

    if let Some((ver, status)) = migration_status(conn) {
        if status == "completed" && ver >= MIGRATION_VERSION {
            result.already_done = true;
            return Ok(result);
        }
    }

    if !has_host_assistant_tables(conn) {
        // Nothing to merge — record completed so we don't re-check forever.
        record_status(conn, "completed", "no host tables")?;
        result.already_done = true;
        return Ok(result);
    }

    conn.execute_batch("BEGIN IMMEDIATE")
        .map_err(|e| format!("begin migration tx: {e}"))?;

    let outcome = (|| -> Result<(), String> {
        result.conversations = copy_conversations(conn)?;
        result.messages = copy_messages(conn)?;
        result.message_blocks = copy_message_blocks(conn)?;
        result.runs = copy_runs(conn)?;
        // Events: daemon wins for same (run_id, sequence) — OR IGNORE preserves daemon.
        result.events = copy_events(conn)?;
        result.prompt_queue = copy_prompt_queue(conn)?;
        Ok(())
    })();

    match outcome {
        Ok(()) => {
            record_status(conn, "completed", "host assistant_* merged")?;
            conn.execute_batch("COMMIT")
                .map_err(|e| format!("commit migration: {e}"))?;
            Ok(result)
        }
        Err(e) => {
            result.errors.push(e.clone());
            let _ = conn.execute_batch("ROLLBACK");
            // Best-effort failure marker outside the rolled-back tx.
            let _ = record_status(conn, "failed", &e);
            Err(e)
        }
    }
}

fn record_status(conn: &Connection, status: &str, detail: &str) -> Result<(), String> {
    if !table_exists(conn, "_host_authority_migration") {
        return Ok(());
    }
    conn.execute(
        "INSERT INTO _host_authority_migration (id, version, status, detail, applied_at)
         VALUES (1, ?1, ?2, ?3, datetime('now'))
         ON CONFLICT(id) DO UPDATE SET
           version=excluded.version,
           status=excluded.status,
           detail=excluded.detail,
           applied_at=excluded.applied_at",
        params![MIGRATION_VERSION, status, detail],
    )
    .map_err(|e| format!("record migration status: {e}"))?;
    Ok(())
}

fn copy_conversations(conn: &Connection) -> Result<u64, String> {
    if !table_exists(conn, "conversation") {
        return Err("canonical conversation table missing".into());
    }
    let n = conn
        .execute(
            "INSERT OR IGNORE INTO conversation (
                id, mode, project_id, title, provider_id, model_id,
                permission_profile_id, created_at, updated_at, archived_at
             )
             SELECT
                id,
                CASE WHEN mode IN ('chat','agent','goal') THEN mode ELSE 'agent' END,
                project_id, title, provider_id, model_id,
                permission_profile_id, created_at, updated_at, archived_at
             FROM assistant_conversations",
            [],
        )
        .map_err(|e| format!("copy conversations: {e}"))?;
    Ok(n as u64)
}

fn copy_messages(conn: &Connection) -> Result<u64, String> {
    if !table_exists(conn, "assistant_messages") {
        return Ok(0);
    }
    // Host messages win: if daemon row exists, update content-bearing columns only when
    // host is newer — for Phase 0 we INSERT OR IGNORE (host wins on first insert only).
    // Prefer host rows when missing in daemon.
    let n = conn
        .execute(
            "INSERT OR IGNORE INTO message (
                id, conversation_id, parent_message_id, role, status,
                input_tokens, output_tokens, reasoning_tokens, cost_usd, created_at
             )
             SELECT
                id, conversation_id, parent_message_id, role, status,
                input_tokens, output_tokens, reasoning_tokens, cost_usd, created_at
             FROM assistant_messages
             WHERE conversation_id IN (SELECT id FROM conversation)",
            [],
        )
        .map_err(|e| format!("copy messages: {e}"))?;
    Ok(n as u64)
}

fn copy_message_blocks(conn: &Connection) -> Result<u64, String> {
    if !table_exists(conn, "assistant_message_blocks") {
        return Ok(0);
    }
    // Host blocks: map block_index → sort_order, content → block_json when needed.
    let n = conn
        .execute(
            "INSERT OR IGNORE INTO message_block (message_id, sort_order, block_type, block_json)
             SELECT
                b.message_id,
                COALESCE(b.block_index, 0),
                b.block_type,
                CASE
                  WHEN b.content LIKE '{%' OR b.content LIKE '[%' THEN b.content
                  ELSE json_object('type', b.block_type, 'text', b.content)
                END
             FROM assistant_message_blocks b
             WHERE b.message_id IN (SELECT id FROM message)",
            [],
        )
        .map_err(|e| format!("copy message_blocks: {e}"))?;
    Ok(n as u64)
}

fn copy_runs(conn: &Connection) -> Result<u64, String> {
    if !table_exists(conn, "assistant_runs") {
        return Ok(0);
    }
    let n = conn
        .execute(
            "INSERT OR IGNORE INTO run (
                id, conversation_id, status, trigger_message_id, provider_id, model_id,
                started_at, finished_at, error_code, step_count, max_steps,
                token_budget, total_input_tokens, total_output_tokens, created_at,
                parent_run_id, permission_profile
             )
             SELECT
                id, conversation_id, status, trigger_message_id, provider_id, model_id,
                started_at, finished_at, error_code, COALESCE(step_count, 0),
                COALESCE(max_steps, 50), token_budget,
                COALESCE(total_input_tokens, 0), COALESCE(total_output_tokens, 0),
                COALESCE(started_at, datetime('now')),
                parent_run_id, COALESCE(permission_profile, 'ask')
             FROM assistant_runs
             WHERE conversation_id IN (SELECT id FROM conversation)",
            [],
        )
        .map_err(|e| format!("copy runs: {e}"))?;
    Ok(n as u64)
}

fn copy_events(conn: &Connection) -> Result<u64, String> {
    if !table_exists(conn, "assistant_run_events") {
        return Ok(0);
    }
    // Daemon wins same (run_id, sequence): OR IGNORE keeps existing daemon rows.
    let n = conn
        .execute(
            "INSERT OR IGNORE INTO run_event (run_id, sequence, event_type, payload, timestamp)
             SELECT run_id, sequence, event_type, payload, timestamp
             FROM assistant_run_events
             WHERE run_id IN (SELECT id FROM run)",
            [],
        )
        .map_err(|e| format!("copy events: {e}"))?;
    Ok(n as u64)
}

fn copy_prompt_queue(conn: &Connection) -> Result<u64, String> {
    if !table_exists(conn, "assistant_prompt_queue") || !table_exists(conn, "prompt_queue") {
        return Ok(0);
    }
    let n = conn
        .execute(
            "INSERT OR IGNORE INTO prompt_queue (
                id, conversation_id, content, source, attachments, position,
                client_temp_id, created_at, updated_at
             )
             SELECT
                id, conversation_id, content, COALESCE(source, 'user'), attachments,
                COALESCE(position, 0), client_temp_id, created_at, updated_at
             FROM assistant_prompt_queue
             WHERE conversation_id IN (SELECT id FROM conversation)",
            [],
        )
        .map_err(|e| format!("copy prompt_queue: {e}"))?;
    Ok(n as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::DataStore;

    #[test]
    fn migrates_host_conversations_idempotently() {
        let tmp = std::env::temp_dir().join(format!("host-auth-{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&tmp);
        let db = tmp.join("assistant.db");
        let art = tmp.join("artifacts");
        // Create store without auto host migration so the test can seed host tables first.
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(
            "PRAGMA foreign_keys=ON;
             PRAGMA journal_mode=WAL;",
        )
        .unwrap();
        // Manually apply migrations via DataStore helper path: open with env isolation.
        drop(conn);
        let store = DataStore::new(&db, &art).unwrap();
        {
            let conn = store.conn().unwrap();
            // Clear auto-migration completed marker if no host tables were present at open.
            let _ = conn.execute("DELETE FROM _host_authority_migration", []);
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS assistant_conversations (
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
                INSERT INTO assistant_conversations
                  (id, mode, title, provider_id, model_id, created_at, updated_at)
                VALUES ('c1', 'agent', 'Hello', 'p', 'm', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');",
            )
            .unwrap();
            let r1 = migrate_host_authority(&conn).unwrap();
            assert!(
                r1.conversations >= 1,
                "expected at least one conversation migrated, got {:?}",
                r1
            );
            let r2 = migrate_host_authority(&conn).unwrap();
            assert!(r2.already_done || r2.conversations == 0);
            let count: i64 = conn
                .query_row("SELECT COUNT(*) FROM conversation WHERE id='c1'", [], |r| {
                    r.get(0)
                })
                .unwrap();
            assert_eq!(count, 1);
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn daemon_events_win_on_sequence_conflict() {
        let tmp = std::env::temp_dir().join(format!("host-auth-ev-{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&tmp);
        let db = tmp.join("assistant.db");
        let art = tmp.join("artifacts");
        let store = DataStore::new(&db, &art).unwrap();
        {
            let conn = store.conn().unwrap();
            conn.execute_batch(
                "INSERT INTO conversation (id, mode, title, provider_id, model_id)
                 VALUES ('c1', 'agent', 't', 'p', 'm');
                 INSERT INTO run (id, conversation_id, status, provider_id, model_id)
                 VALUES ('r1', 'c1', 'completed', 'p', 'm');
                 INSERT INTO run_event (run_id, sequence, event_type, payload)
                 VALUES ('r1', 1, 'started', '{\"type\":\"started\"}');
                 CREATE TABLE IF NOT EXISTS assistant_conversations (
                    id TEXT PRIMARY KEY, mode TEXT, project_id TEXT, title TEXT,
                    provider_id TEXT, model_id TEXT, permission_profile_id TEXT,
                    created_at TEXT, updated_at TEXT, archived_at TEXT
                 );
                 CREATE TABLE IF NOT EXISTS assistant_runs (
                    id TEXT PRIMARY KEY, conversation_id TEXT, status TEXT,
                    trigger_message_id TEXT, provider_id TEXT, model_id TEXT,
                    started_at TEXT, finished_at TEXT, error_code TEXT,
                    step_count INTEGER, max_steps INTEGER, token_budget INTEGER,
                    total_input_tokens INTEGER, total_output_tokens INTEGER,
                    parent_run_id TEXT, permission_profile TEXT
                 );
                 CREATE TABLE IF NOT EXISTS assistant_run_events (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    run_id TEXT, sequence INTEGER, timestamp TEXT,
                    event_type TEXT, payload TEXT
                 );
                 INSERT INTO assistant_conversations
                   (id, mode, title, provider_id, model_id, created_at, updated_at)
                 VALUES ('c1', 'agent', 't', 'p', 'm', datetime('now'), datetime('now'));
                 INSERT INTO assistant_runs
                   (id, conversation_id, status, provider_id, model_id)
                 VALUES ('r1', 'c1', 'completed', 'p', 'm');
                 INSERT INTO assistant_run_events
                   (run_id, sequence, timestamp, event_type, payload)
                 VALUES ('r1', 1, datetime('now'), 'failed', '{\"type\":\"failed\"}');",
            )
            .unwrap();
            let _ = migrate_host_authority(&conn).unwrap();
            let etype: String = conn
                .query_row(
                    "SELECT event_type FROM run_event WHERE run_id='r1' AND sequence=1",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(etype, "started", "daemon event must win");
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
