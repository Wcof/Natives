//! BrowserProfile store (batch 6 CR-601).
//!
//! Profiles are metadata-only — never store cookie content. The platform
//! store key identifies the WebKit WKWebsiteDataStore for future per-profile
//! isolation. On macOS 26.5, all WKWebView share the same data store, so
//! profiles are forward-looking (see ADR-0017).

use super::model::BrowserProfile;
use crate::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Create a browser profile. Returns the new profile id.
pub fn create_profile(conn: &Connection, name: &str, platform_store_key: &str) -> Result<String> {
    let id = Uuid::new_v4().to_string();
    let t = now();
    conn.execute(
        "INSERT INTO browser_profiles (id, name, platform_store_key, is_default, created_at, updated_at)
         VALUES (?1, ?2, ?3, 0, ?4, ?4)",
        params![id, name, platform_store_key, t],
    )
    .map_err(Error::Database)?;
    Ok(id)
}

/// List all browser profiles.
pub fn list_profiles(conn: &Connection) -> Result<Vec<BrowserProfile>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, name, platform_store_key, is_default, created_at, updated_at
             FROM browser_profiles ORDER BY is_default DESC, name",
        )
        .map_err(Error::Database)?;
    let rows = stmt
        .query_map([], |row| {
            Ok(BrowserProfile {
                id: row.get(0)?,
                name: row.get(1)?,
                platform_store_key: row.get(2)?,
                is_default: row.get::<_, i32>(3)? != 0,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })
        .map_err(Error::Database)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(Error::Database)?);
    }
    Ok(out)
}

/// Find a profile by id.
pub fn find_profile(conn: &Connection, profile_id: &str) -> Result<Option<BrowserProfile>> {
    conn.query_row(
        "SELECT id, name, platform_store_key, is_default, created_at, updated_at
         FROM browser_profiles WHERE id = ?1",
        params![profile_id],
        |row| {
            Ok(BrowserProfile {
                id: row.get(0)?,
                name: row.get(1)?,
                platform_store_key: row.get(2)?,
                is_default: row.get::<_, i32>(3)? != 0,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        },
    )
    .optional()
    .map_err(Error::Database)
}

/// Get the default profile (creates one if missing).
pub fn default_profile(conn: &Connection) -> Result<BrowserProfile> {
    if let Some(p) = conn
        .query_row(
            "SELECT id, name, platform_store_key, is_default, created_at, updated_at
             FROM browser_profiles WHERE is_default = 1",
            [],
            |row| {
                Ok(BrowserProfile {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    platform_store_key: row.get(2)?,
                    is_default: true,
                    created_at: row.get(4)?,
                    updated_at: row.get(5)?,
                })
            },
        )
        .optional()
        .map_err(Error::Database)?
    {
        return Ok(p);
    }
    // Create default profile
    let t = now();
    conn.execute(
        "INSERT INTO browser_profiles (id, name, platform_store_key, is_default, created_at, updated_at)
         VALUES ('default', 'Default', 'default', 1, ?1, ?1)",
        params![t],
    )
    .map_err(Error::Database)?;
    find_profile(conn, "default")?.ok_or_else(|| Error::Internal("default profile vanished".into()))
}

/// Delete a profile (not the default).
pub fn delete_profile(conn: &Connection, profile_id: &str) -> Result<()> {
    let affected = conn
        .execute(
            "DELETE FROM browser_profiles WHERE id = ?1 AND is_default = 0",
            params![profile_id],
        )
        .map_err(Error::Database)?;
    if affected == 0 {
        return Err(Error::InvalidInput(
            "cannot delete default profile or non-existent profile".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn fixture() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        db::create_tables(&conn).unwrap();
        db::apply_migrations(&conn).unwrap();
        conn
    }

    #[test]
    fn default_profile_exists_after_migration() {
        let conn = fixture();
        let p = default_profile(&conn).unwrap();
        assert_eq!(p.name, "Default");
        assert!(p.is_default);
    }

    #[test]
    fn create_and_list_profiles() {
        let conn = fixture();
        let id = create_profile(&conn, "Work", "work-key").unwrap();
        let profiles = list_profiles(&conn).unwrap();
        // At least the default + our new one
        assert!(profiles.len() >= 2);
        let found = profiles.iter().find(|p| p.id == id).unwrap();
        assert_eq!(found.name, "Work");
    }

    #[test]
    fn delete_non_default_profile() {
        let conn = fixture();
        let id = create_profile(&conn, "Temp", "temp-key").unwrap();
        delete_profile(&conn, &id).unwrap();
        assert!(find_profile(&conn, &id).unwrap().is_none());
    }

    #[test]
    fn cannot_delete_default_profile() {
        let conn = fixture();
        assert!(delete_profile(&conn, "default").is_err());
    }
}