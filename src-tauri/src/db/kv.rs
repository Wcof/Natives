use crate::{Error, Result};
use rusqlite::{Connection, OptionalExtension};

const APP_MODULE_ID: &str = "_app";

/// Generic module_data read (used by state persistence)
pub fn db_get_module_data(
    conn: &Connection,
    module_id: &str,
    key: &str,
) -> Result<Option<serde_json::Value>> {
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
pub fn db_set_module_data(
    conn: &Connection,
    module_id: &str,
    key: &str,
    value: &serde_json::Value,
) -> Result<()> {
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
    let stmt = match prefix {
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
