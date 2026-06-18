use crate::{Error, Result};
use rusqlite::OptionalExtension;
use rusqlite::Connection;
use std::path::Path;

/// Initialize the SQLite database with WAL mode, foreign keys, and all tables.
pub fn init_db(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA foreign_keys = ON;
         PRAGMA busy_timeout = 5000;",
    )?;
    create_tables(&conn)?;
    apply_migrations(&conn)?;
    Ok(conn)
}

fn create_tables(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        -- 1. Installed plugin registry
        CREATE TABLE IF NOT EXISTS modules (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            version TEXT NOT NULL,
            entry TEXT NOT NULL,
            type TEXT NOT NULL,
            description TEXT,
            author TEXT,
            icon TEXT,
            enabled INTEGER DEFAULT 1,
            min_natives_version TEXT,
            state TEXT DEFAULT 'installed',
            created_at TEXT,
            updated_at TEXT
        );

        -- 2. Per-module permission grants
        CREATE TABLE IF NOT EXISTS module_permissions (
            module_id TEXT NOT NULL REFERENCES modules(id) ON DELETE CASCADE,
            permission TEXT NOT NULL,
            granted INTEGER DEFAULT 0,
            PRIMARY KEY (module_id, permission)
        );

        -- 3. Key-value app settings
        CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            updated_at TEXT
        );

        -- 4. Per-module key-value storage
        CREATE TABLE IF NOT EXISTS module_data (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            module_id TEXT NOT NULL,
            key TEXT NOT NULL,
            value TEXT,
            UNIQUE(module_id, key)
        );

        -- 5. Workshop/marketplace cache
        CREATE TABLE IF NOT EXISTS workshop_cache (
            id TEXT PRIMARY KEY,
            name TEXT,
            version TEXT,
            description TEXT,
            author TEXT,
            icon TEXT,
            permissions TEXT,
            installed INTEGER DEFAULT 0
        );

        -- 6. Environment configuration profiles
        CREATE TABLE IF NOT EXISTS env_profiles (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL UNIQUE,
            is_default INTEGER DEFAULT 0,
            created_at TEXT
        );

        -- 7. Encrypted env vars per profile
        CREATE TABLE IF NOT EXISTS env_variables (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            profile_id INTEGER NOT NULL REFERENCES env_profiles(id) ON DELETE CASCADE,
            key TEXT NOT NULL,
            value_encrypted TEXT NOT NULL,
            UNIQUE(profile_id, key)
        );

        -- 8. Notification queue
        CREATE TABLE IF NOT EXISTS notifications (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            module_id TEXT,
            title TEXT,
            body TEXT,
            level TEXT DEFAULT 'info',
            read INTEGER DEFAULT 0,
            created_at TEXT
        );

        -- 9. Sidebar ordering
        CREATE TABLE IF NOT EXISTS module_order (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            module_id TEXT NOT NULL REFERENCES modules(id) ON DELETE CASCADE UNIQUE,
            sort_order INTEGER NOT NULL
        );

        -- 10. Permission audit trail
        CREATE TABLE IF NOT EXISTS permission_audit_log (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            module_id TEXT NOT NULL,
            permission TEXT NOT NULL,
            action TEXT NOT NULL CHECK(action IN ('grant','revoke','deny','approve')),
            granted INTEGER NOT NULL,
            reason TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        -- Indexes
        CREATE INDEX IF NOT EXISTS idx_notifications_unread ON notifications(read, created_at);
        CREATE INDEX IF NOT EXISTS idx_audit_log_module ON permission_audit_log(module_id, created_at);
        ",
    )?;
    Ok(())
}

fn apply_migrations(conn: &Connection) -> Result<()> {
    // Migration system: check existing columns, add missing ones.
    // Currently no migrations needed — schema is at v1.
    let _ = conn;
    Ok(())
}

// ──────────────────────────────────────────────
// Module data CRUD (used by db:get/set/delete/list)
// Maps to the "module_data" table with a fixed module_id
// The Electron preload's db.get/set uses a single flat namespace,
// stored in module_data with module_id = '_app'.
// ──────────────────────────────────────────────

const APP_MODULE_ID: &str = "_app";

/// Generic module_data read (used by state persistence)
pub fn db_get_module_data(conn: &Connection, module_id: &str, key: &str) -> Result<Option<serde_json::Value>> {
    let mut stmt = conn
        .prepare("SELECT value FROM module_data WHERE module_id = ?1 AND key = ?2")
        .map_err(Error::Database)?;
    let result: Option<String> = stmt
        .query_row(rusqlite::params![module_id, key], |row| {
            row.get::<_, String>(0)
        })
        .optional()
        .map_err(Error::Database)?;
    match result {
        Some(s) => {
            let v: serde_json::Value =
                serde_json::from_str(&s).unwrap_or(serde_json::Value::String(s));
            Ok(Some(v))
        }
        None => Ok(None),
    }
}

/// Generic module_data write (used by state persistence)
pub fn db_set_module_data(conn: &Connection, module_id: &str, key: &str, value: &serde_json::Value) -> Result<()> {
    let serialized = serde_json::to_string(value).map_err(Error::Json)?;
    conn.execute(
        "INSERT INTO module_data (module_id, key, value) VALUES (?1, ?2, ?3)
         ON CONFLICT(module_id, key) DO UPDATE SET value = excluded.value",
        rusqlite::params![module_id, key, serialized],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// Generic module_data delete (used by state persistence)
pub fn db_delete_module_data(conn: &Connection, module_id: &str, key: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM module_data WHERE module_id = ?1 AND key = ?2",
        rusqlite::params![module_id, key],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// db:get — read a value from module_data
pub fn db_get(conn: &Connection, key: &str) -> Result<Option<serde_json::Value>> {
    let mut stmt = conn
        .prepare("SELECT value FROM module_data WHERE module_id = ?1 AND key = ?2")
        .map_err(Error::Database)?;
    let result: Option<String> = stmt
        .query_row(rusqlite::params![APP_MODULE_ID, key], |row| {
            row.get::<_, String>(0)
        })
        .optional()
        .map_err(Error::Database)?;

    match result {
        Some(s) => {
            let v: serde_json::Value =
                serde_json::from_str(&s).unwrap_or(serde_json::Value::String(s));
            Ok(Some(v))
        }
        None => Ok(None),
    }
}

/// db:set — upsert a value into module_data
pub fn db_set(conn: &Connection, key: &str, value: &serde_json::Value) -> Result<()> {
    let serialized = serde_json::to_string(value).map_err(Error::Json)?;
    conn.execute(
        "INSERT INTO module_data (module_id, key, value) VALUES (?1, ?2, ?3)
         ON CONFLICT(module_id, key) DO UPDATE SET value = excluded.value",
        rusqlite::params![APP_MODULE_ID, key, serialized],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// db:delete — remove a key from module_data
pub fn db_delete(conn: &Connection, key: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM module_data WHERE module_id = ?1 AND key = ?2",
        rusqlite::params![APP_MODULE_ID, key],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// db:list — list keys in module_data, optionally filtered by prefix
pub fn db_list(conn: &Connection, prefix: Option<&str>) -> Result<Vec<String>> {
    let mut stmt = match prefix {
        Some(_) => {
            let mut s = conn
                .prepare("SELECT key FROM module_data WHERE module_id = ?1 AND key LIKE ?2")
                .map_err(Error::Database)?;
            let pattern = format!("{}%", prefix.unwrap());
            let rows = s
                .query_map(rusqlite::params![APP_MODULE_ID, pattern], |row| {
                    row.get::<_, String>(0)
                })
                .map_err(Error::Database)?;
            rows.collect::<std::result::Result<Vec<_>, _>>()
                .map_err(Error::Database)
        }
        None => {
            let mut s = conn
                .prepare("SELECT key FROM module_data WHERE module_id = ?1")
                .map_err(Error::Database)?;
            let rows = s
                .query_map(rusqlite::params![APP_MODULE_ID], |row| {
                    row.get::<_, String>(0)
                })
                .map_err(Error::Database)?;
            rows.collect::<std::result::Result<Vec<_>, _>>()
                .map_err(Error::Database)
        }
    }?;
    Ok(stmt)
}

// ──────────────────────────────────────────────
// Settings helpers (used by theme, locale, etc.)
// ──────────────────────────────────────────────

/// Read a setting value by key
pub fn get_setting(conn: &Connection, key: &str) -> Result<Option<String>> {
    let mut stmt = conn
        .prepare("SELECT value FROM settings WHERE key = ?1")
        .map_err(Error::Database)?;
    let result: Option<String> = stmt
        .query_row(rusqlite::params![key], |row| row.get::<_, String>(0))
        .optional()
        .map_err(Error::Database)?;
    Ok(result)
}

/// Write a setting value (upsert)
pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        rusqlite::params![key, value, now],
    )
    .map_err(Error::Database)?;
    Ok(())
}

// ──────────────────────────────────────────────
// Notifications
// ──────────────────────────────────────────────

/// Create a notification and return its ID
pub fn create_notification(
    conn: &Connection,
    title: &str,
    body: &str,
    level: &str,
    module_id: Option<&str>,
) -> Result<i64> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO notifications (module_id, title, body, level, read, created_at) VALUES (?1, ?2, ?3, ?4, 0, ?5)",
        rusqlite::params![module_id, title, body, level, now],
    )
    .map_err(Error::Database)?;
    Ok(conn.last_insert_rowid())
}

/// List notifications, optionally filtered by unread-only
pub fn list_notifications(conn: &Connection, unread_only: bool) -> Result<Vec<serde_json::Value>> {
    let sql = if unread_only {
        "SELECT id, module_id, title, body, level, read, created_at FROM notifications WHERE read = 0 ORDER BY created_at DESC"
    } else {
        "SELECT id, module_id, title, body, level, read, created_at FROM notifications ORDER BY created_at DESC"
    };
    let mut stmt = conn.prepare(sql).map_err(Error::Database)?;
    let rows = stmt
        .query_map([], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, i64>(0)?,
                "moduleId": row.get::<_, Option<String>>(1)?,
                "title": row.get::<_, String>(2)?,
                "body": row.get::<_, String>(3)?,
                "level": row.get::<_, String>(4)?,
                "read": row.get::<_, i64>(5)? != 0,
                "createdAt": row.get::<_, String>(6)?,
            }))
        })
        .map_err(Error::Database)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Error::Database)
}

/// Mark a notification as read
pub fn mark_notification_read(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(
        "UPDATE notifications SET read = 1 WHERE id = ?1",
        rusqlite::params![id],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// Mark all notifications as read
pub fn mark_all_notifications_read(conn: &Connection) -> Result<()> {
    conn.execute("UPDATE notifications SET read = 1 WHERE read = 0", [])
        .map_err(Error::Database)?;
    Ok(())
}
