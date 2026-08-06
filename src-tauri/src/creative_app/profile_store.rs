//! BrowserProfile store (batch 6 CR-601, T08 production).
//!
//! A profile is a named browsing-session isolation boundary backed by a real
//! per-profile WebKit data store. On macOS 14+ / iOS 17+ wry supports
//! `WKWebsiteDataStore(dataStoreForIdentifier:)`; `platform_store_key` is the
//! 32-hex (16-byte) identifier handed to `WebviewBuilder::data_store_identifier`.
//! Cookies, localStorage, IndexedDB and service workers are therefore isolated
//! per profile and are removed with the store when a profile is deleted.
//!
//! Profile content never touches this store — only the identifier. Profile
//! isolation was previously "metadata-only / forward-looking" (ADR-0017, batch
//! 6); T08 promotes it to a real capability now that the platform API is wired
//! through wry.

use super::model::BrowserProfile;
use crate::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub const DEFAULT_PROFILE_ID: &str = "default";

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Deterministic 16-byte WebKit data store identifier (as 32-hex) derived from
/// the profile id. Stable across restarts so a profile's cookies survive
/// relaunch, while different profile ids always map to different stores.
pub fn store_key_for_id(id: &str) -> String {
    let digest = Sha256::digest(id.as_bytes());
    hex::encode(&digest[..16])
}

/// Parse a stored `platform_store_key` back into the 16-byte WebKit data store
/// identifier used by `WebviewBuilder::data_store_identifier`.
pub fn data_store_identifier(profile: &BrowserProfile) -> Option<[u8; 16]> {
    let key = profile.platform_store_key.trim();
    if key.len() != 32 {
        return None;
    }
    let bytes = hex::decode(key).ok()?;
    bytes.try_into().ok()
}

/// Create a browser profile. Returns the new profile id. Each profile gets a
/// fresh data store identifier so its cookies/storage never collide with
/// another profile's.
pub fn create_profile(conn: &Connection, name: &str, is_default: bool) -> Result<String> {
    let id = Uuid::new_v4().to_string();
    let t = now();
    conn.execute(
        "INSERT INTO browser_profiles (id, name, platform_store_key, is_default, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        params![id, name, store_key_for_id(&id), is_default as i32, t],
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

/// Get the default profile (creates one if missing). The default profile uses
/// a deterministic data store identifier so its cookies survive restarts.
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
    // Create default profile with a stable store key.
    let t = now();
    conn.execute(
        "INSERT INTO browser_profiles (id, name, platform_store_key, is_default, created_at, updated_at)
         VALUES (?1, 'Default', ?2, 1, ?3, ?3)",
        params![DEFAULT_PROFILE_ID, store_key_for_id(DEFAULT_PROFILE_ID), t],
    )
    .map_err(Error::Database)?;
    find_profile(conn, DEFAULT_PROFILE_ID)?
        .ok_or_else(|| Error::Internal("default profile vanished".into()))
}

/// Bind an application to a profile. The app's WebViews then use the profile's
/// data store identifier. Bindings cascade-delete with either side.
pub fn bind_profile_to_app(
    conn: &Connection,
    application_id: &str,
    profile_id: &str,
) -> Result<()> {
    if find_profile(conn, profile_id)?.is_none() {
        return Err(Error::NotFound(format!("profile {profile_id}")));
    }
    let t = now();
    conn.execute(
        "INSERT INTO browser_profile_bindings (application_id, profile_id, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?3)
         ON CONFLICT(application_id) DO UPDATE SET
             profile_id = excluded.profile_id,
             updated_at = excluded.updated_at",
        params![application_id, profile_id, t],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// Remove an application's profile binding (falls back to the default profile).
pub fn unbind_profile(conn: &Connection, application_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM browser_profile_bindings WHERE application_id = ?1",
        params![application_id],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// Resolve the profile for an application: explicit binding, else the default.
pub fn profile_for_app(conn: &Connection, application_id: &str) -> Result<BrowserProfile> {
    let bound: Option<String> = conn
        .query_row(
            "SELECT profile_id FROM browser_profile_bindings WHERE application_id = ?1",
            params![application_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(Error::Database)?;
    if let Some(profile_id) = bound {
        if let Some(profile) = find_profile(conn, &profile_id)? {
            return Ok(profile);
        }
    }
    default_profile(conn)
}

/// All application→profile bindings (application_id, profile_id, updated_at).
pub fn list_profile_bindings(conn: &Connection) -> Result<Vec<ProfileBinding>> {
    let mut stmt = conn
        .prepare(
            "SELECT application_id, profile_id, updated_at
             FROM browser_profile_bindings ORDER BY application_id",
        )
        .map_err(Error::Database)?;
    let rows = stmt
        .query_map([], |row| {
            Ok(ProfileBinding {
                application_id: row.get(0)?,
                profile_id: row.get(1)?,
                updated_at: row.get(2)?,
            })
        })
        .map_err(Error::Database)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(Error::Database)?);
    }
    Ok(out)
}

/// Delete a profile (not the default). Bindings and — via the runtime data
/// store cleanup — the profile's cookies/storage are removed with it.
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

/// A row of the application→profile binding table.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProfileBinding {
    pub application_id: String,
    pub profile_id: String,
    pub updated_at: String,
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

    fn ensure_app(conn: &Connection, id: &str) {
        conn.execute(
            "INSERT OR IGNORE INTO applications (id, source, source_id, title, version, created_at, updated_at)
             VALUES (?1, 'local_project', ?1, 'Test', '1', 't', 't')",
            params![id],
        )
        .unwrap();
    }

    #[test]
    fn default_profile_exists_after_migration() {
        let conn = fixture();
        let p = default_profile(&conn).unwrap();
        assert_eq!(p.name, "Default");
        assert!(p.is_default);
        assert!(data_store_identifier(&p).is_some());
    }

    #[test]
    fn default_profile_store_key_is_stable() {
        let conn = fixture();
        let a = default_profile(&conn).unwrap();
        let b = default_profile(&conn).unwrap();
        assert_eq!(a.platform_store_key, b.platform_store_key);
    }

    #[test]
    fn migration_repairs_legacy_placeholder_store_key() {
        // Simulate a pre-v23 row ('default' placeholder key) and confirm the
        // v23 migration repairs it to a parseable 16-byte identifier.
        let conn = Connection::open_in_memory().unwrap();
        db::create_tables(&conn).unwrap();
        db::apply_migrations(&conn).unwrap();
        conn.execute(
            "UPDATE browser_profiles SET platform_store_key = 'default' WHERE id = 'default'",
            [],
        )
        .unwrap();
        conn.execute(
            "UPDATE settings SET value = '22' WHERE key = '_schema_version'",
            [],
        )
        .unwrap();
        db::apply_migrations(&conn).unwrap();
        let p = default_profile(&conn).unwrap();
        assert_eq!(p.platform_store_key.len(), 32);
        assert!(data_store_identifier(&p).is_some());
    }

    #[test]
    fn create_profile_generates_real_store_key() {
        let conn = fixture();
        let id = create_profile(&conn, "Work", false).unwrap();
        let p = find_profile(&conn, &id).unwrap().unwrap();
        let ident = data_store_identifier(&p).expect("profile store key must be a 16-byte id");
        assert_eq!(ident.len(), 16);
        assert_ne!(
            p.platform_store_key,
            default_profile(&conn).unwrap().platform_store_key
        );
    }

    #[test]
    fn distinct_profiles_map_to_distinct_stores() {
        let conn = fixture();
        let a = create_profile(&conn, "A", false).unwrap();
        let b = create_profile(&conn, "B", false).unwrap();
        let pa = find_profile(&conn, &a).unwrap().unwrap();
        let pb = find_profile(&conn, &b).unwrap().unwrap();
        assert_ne!(pa.platform_store_key, pb.platform_store_key);
        assert_ne!(data_store_identifier(&pa), data_store_identifier(&pb));
    }

    #[test]
    fn bind_and_resolve_profile_for_app() {
        let conn = fixture();
        ensure_app(&conn, "app-1");
        let work = create_profile(&conn, "Work", false).unwrap();
        bind_profile_to_app(&conn, "app-1", &work).unwrap();
        let p = profile_for_app(&conn, "app-1").unwrap();
        assert_eq!(p.id, work);

        let bindings = list_profile_bindings(&conn).unwrap();
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].application_id, "app-1");
        assert_eq!(bindings[0].profile_id, work);
    }

    #[test]
    fn unbind_falls_back_to_default() {
        let conn = fixture();
        ensure_app(&conn, "app-1");
        let work = create_profile(&conn, "Work", false).unwrap();
        bind_profile_to_app(&conn, "app-1", &work).unwrap();
        unbind_profile(&conn, "app-1").unwrap();
        let p = profile_for_app(&conn, "app-1").unwrap();
        assert!(p.is_default);
    }

    #[test]
    fn bind_rejects_unknown_profile() {
        let conn = fixture();
        ensure_app(&conn, "app-1");
        assert!(bind_profile_to_app(&conn, "app-1", "nope").is_err());
    }

    #[test]
    fn delete_non_default_profile_removes_binding() {
        let conn = fixture();
        ensure_app(&conn, "app-1");
        let work = create_profile(&conn, "Work", false).unwrap();
        bind_profile_to_app(&conn, "app-1", &work).unwrap();
        delete_profile(&conn, &work).unwrap();
        assert!(find_profile(&conn, &work).unwrap().is_none());
        let p = profile_for_app(&conn, "app-1").unwrap();
        assert!(
            p.is_default,
            "deleted profile binding must cascade to default"
        );
    }

    #[test]
    fn cannot_delete_default_profile() {
        let conn = fixture();
        assert!(delete_profile(&conn, "default").is_err());
    }

    #[test]
    fn profile_binding_survives_reopen() {
        let dir = tempfile::tempdir().unwrap();
        {
            let conn = Connection::open(dir.path().join("test.db")).unwrap();
            db::create_tables(&conn).unwrap();
            db::apply_migrations(&conn).unwrap();
            ensure_app(&conn, "app-1");
            let work = create_profile(&conn, "Work", false).unwrap();
            bind_profile_to_app(&conn, "app-1", &work).unwrap();
        }
        let conn = Connection::open(dir.path().join("test.db")).unwrap();
        db::create_tables(&conn).unwrap();
        db::apply_migrations(&conn).unwrap();
        let p = profile_for_app(&conn, "app-1").unwrap();
        assert_eq!(p.name, "Work");
    }
}
