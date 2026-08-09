use crate::{Error, Result};
use rusqlite::{Connection, OptionalExtension};

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

/// Delete a setting value by key
pub fn delete_setting(conn: &Connection, key: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM settings WHERE key = ?1",
        rusqlite::params![key],
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

// ──────────────────────────────────────────────
// Builtin tools CRUD (extensible tool registry)
// ──────────────────────────────────────────────
