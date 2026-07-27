use crate::{Error, Result};
use serde::{Deserialize, Serialize};

/// Vendor whitelist management for CSP enforcement.
/// Controls which pre-vendored libraries are available at `tauri://assets/vendor/`.

const VENDOR_DB_KEY: &str = "vendor:whitelist";

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct VendorEntry {
    pub name: String,
    pub version: String,
    pub path: String,
    pub enabled: bool,
}

/// Get the current vendor whitelist from the database.
pub fn get_vendor_whitelist(conn: &rusqlite::Connection) -> Result<Vec<VendorEntry>> {
    let mut stmt = conn
        .prepare("SELECT value FROM settings WHERE key = ?1")
        .map_err(|e| Error::Internal(e.to_string()))?;

    let result: Option<String> = stmt
        .query_row(rusqlite::params![VENDOR_DB_KEY], |row| row.get(0))
        .ok();

    match result {
        Some(json_str) => serde_json::from_str(&json_str)
            .map_err(|e| Error::Internal(format!("Failed to parse vendor whitelist: {e}"))),
        None => Ok(default_whitelist()),
    }
}

/// Save the vendor whitelist to the database.
pub fn save_vendor_whitelist(conn: &rusqlite::Connection, entries: &[VendorEntry]) -> Result<()> {
    let json_str = serde_json::to_string(entries)
        .map_err(|e| Error::Internal(format!("Failed to serialize vendor whitelist: {e}")))?;

    conn.execute(
        "INSERT OR REPLACE INTO settings (key, value, updated_at) VALUES (?1, ?2, datetime('now'))",
        rusqlite::params![VENDOR_DB_KEY, json_str],
    )
    .map_err(|e| Error::Internal(e.to_string()))?;

    Ok(())
}

/// Add a vendor entry to the whitelist.
pub fn add_vendor_entry(conn: &rusqlite::Connection, entry: VendorEntry) -> Result<()> {
    let mut list = get_vendor_whitelist(conn)?;
    // Replace if exists with same name
    list.retain(|e| e.name != entry.name);
    list.push(entry);
    save_vendor_whitelist(conn, &list)
}

/// Remove a vendor entry from the whitelist.
pub fn remove_vendor_entry(conn: &rusqlite::Connection, name: &str) -> Result<()> {
    let mut list = get_vendor_whitelist(conn)?;
    list.retain(|e| e.name != name);
    save_vendor_whitelist(conn, &list)
}

/// Check if a vendor path is whitelisted.
pub fn is_vendor_allowed(conn: &rusqlite::Connection, path: &str) -> Result<bool> {
    let list = get_vendor_whitelist(conn)?;
    Ok(list.iter().any(|e| path.starts_with(&e.path) && e.enabled))
}

/// Default vendor whitelist with commonly used lightweight libraries.
pub fn default_whitelist() -> Vec<VendorEntry> {
    vec![
        VendorEntry {
            name: "alpine.js".to_string(),
            version: "3.14.1".to_string(),
            path: "tauri://assets/vendor/alpine.js".to_string(),
            enabled: true,
        },
        VendorEntry {
            name: "htmx".to_string(),
            version: "2.0.0".to_string(),
            path: "tauri://assets/vendor/htmx.js".to_string(),
            enabled: true,
        },
        VendorEntry {
            name: "petite-vue".to_string(),
            version: "0.4.1".to_string(),
            path: "tauri://assets/vendor/petite-vue.js".to_string(),
            enabled: true,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                updated_at TEXT
            );",
        )
        .unwrap();
        conn
    }

    #[test]
    fn test_default_whitelist() {
        let conn = setup_db();
        let list = get_vendor_whitelist(&conn).unwrap();
        assert!(list.iter().any(|e| e.name == "alpine.js"));
        assert!(list.iter().any(|e| e.name == "htmx"));
        assert_eq!(list.len(), 3);
    }

    #[test]
    fn test_add_and_remove_vendor() {
        let conn = setup_db();
        let entry = VendorEntry {
            name: "test-lib".to_string(),
            version: "1.0.0".to_string(),
            path: "tauri://assets/vendor/test-lib.js".to_string(),
            enabled: true,
        };

        add_vendor_entry(&conn, entry.clone()).unwrap();
        let list = get_vendor_whitelist(&conn).unwrap();
        assert!(list.iter().any(|e| e.name == "test-lib"));

        remove_vendor_entry(&conn, "test-lib").unwrap();
        let list = get_vendor_whitelist(&conn).unwrap();
        assert!(!list.iter().any(|e| e.name == "test-lib"));
    }

    #[test]
    fn test_is_vendor_allowed() {
        let conn = setup_db();
        assert!(is_vendor_allowed(&conn, "tauri://assets/vendor/alpine.js").unwrap());
        assert!(!is_vendor_allowed(&conn, "https://cdn.example.com/lib.js").unwrap());
    }
}
