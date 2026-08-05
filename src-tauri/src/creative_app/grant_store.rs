//! Grant store (batch 6 CR-602/603).
//!
//! OAuth allowlist and per-app capability grants. All grants default to deny
//! — the user must explicitly approve each capability.

use super::model::{AppGrant, OAuthAllowlistEntry};
use crate::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

// ── OAuth Allowlist ─────────────────────────────────────────────────

/// Add an allowed domain for OAuth popup.
pub fn add_oauth_domain(
    conn: &Connection,
    application_id: &str,
    domain: &str,
) -> Result<String> {
    let id = Uuid::new_v4().to_string();
    let t = now();
    conn.execute(
        "INSERT OR IGNORE INTO oauth_allowlist (id, application_id, domain, created_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![id, application_id, domain, t],
    )
    .map_err(Error::Database)?;
    Ok(id)
}

/// Check if a domain is allowed for OAuth popup.
pub fn is_oauth_domain_allowed(conn: &Connection, application_id: &str, domain: &str) -> Result<bool> {
    let exists = conn
        .query_row(
            "SELECT 1 FROM oauth_allowlist WHERE application_id = ?1 AND domain = ?2",
            params![application_id, domain],
            |_| Ok(()),
        )
        .optional()
        .map_err(Error::Database)?;
    Ok(exists.is_some())
}

/// List allowed OAuth domains for an application.
pub fn list_oauth_domains(
    conn: &Connection,
    application_id: &str,
) -> Result<Vec<OAuthAllowlistEntry>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, application_id, domain, created_at
             FROM oauth_allowlist WHERE application_id = ?1 ORDER BY created_at",
        )
        .map_err(Error::Database)?;
    let rows = stmt
        .query_map(params![application_id], |row| {
            Ok(OAuthAllowlistEntry {
                id: row.get(0)?,
                application_id: row.get(1)?,
                domain: row.get(2)?,
                created_at: row.get(3)?,
            })
        })
        .map_err(Error::Database)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(Error::Database)?);
    }
    Ok(out)
}

/// Remove an OAuth domain from the allowlist.
pub fn remove_oauth_domain(conn: &Connection, entry_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM oauth_allowlist WHERE id = ?1",
        params![entry_id],
    )
    .map_err(Error::Database)?;
    Ok(())
}

// ── App Grants ───────────────────────────────────────────────────────

/// Set a grant policy for an application+kind.
pub fn set_grant(
    conn: &Connection,
    application_id: &str,
    kind: &str,
    policy: &str,
    path: Option<&str>,
) -> Result<()> {
    let t = now();
    conn.execute(
        "INSERT INTO app_grants (id, application_id, kind, policy, path, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
         ON CONFLICT(application_id, kind) DO UPDATE SET
             policy = excluded.policy,
             path = excluded.path,
             updated_at = excluded.updated_at",
        params![Uuid::new_v4().to_string(), application_id, kind, policy, path, t],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// Get the grant policy for an application+kind. Returns default_deny if not set.
pub fn get_grant(
    conn: &Connection,
    application_id: &str,
    kind: &str,
) -> Result<AppGrant> {
    conn.query_row(
        "SELECT id, application_id, kind, policy, path, created_at, updated_at
         FROM app_grants WHERE application_id = ?1 AND kind = ?2",
        params![application_id, kind],
        |row| {
            Ok(AppGrant {
                id: row.get(0)?,
                application_id: row.get(1)?,
                kind: row.get(2)?,
                policy: row.get(3)?,
                path: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        },
    )
    .optional()
    .map_err(Error::Database)?
    .ok_or_else(|| {
        Error::NotFound(format!("grant {kind} for app {application_id}"))
    })
}

/// Check if a grant is allowed (persistent or one_time).
pub fn is_grant_allowed(
    conn: &Connection,
    application_id: &str,
    kind: &str,
) -> Result<bool> {
    let grant = conn
        .query_row(
            "SELECT policy FROM app_grants WHERE application_id = ?1 AND kind = ?2",
            params![application_id, kind],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(Error::Database)?;
    match grant.as_deref() {
        Some(AppGrant::POLICY_PERSISTENT) => Ok(true),
        Some(AppGrant::POLICY_ONE_TIME) => {
            // One-time grant: allow once, then reset to default_deny
            set_grant(conn, application_id, kind, AppGrant::POLICY_DEFAULT_DENY, None)?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

/// List all grants for an application.
pub fn list_grants(conn: &Connection, application_id: &str) -> Result<Vec<AppGrant>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, application_id, kind, policy, path, created_at, updated_at
             FROM app_grants WHERE application_id = ?1 ORDER BY kind",
        )
        .map_err(Error::Database)?;
    let rows = stmt
        .query_map(params![application_id], |row| {
            Ok(AppGrant {
                id: row.get(0)?,
                application_id: row.get(1)?,
                kind: row.get(2)?,
                policy: row.get(3)?,
                path: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })
        .map_err(Error::Database)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(Error::Database)?);
    }
    Ok(out)
}

/// Delete a grant.
pub fn delete_grant(conn: &Connection, grant_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM app_grants WHERE id = ?1",
        params![grant_id],
    )
    .map_err(Error::Database)?;
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

    fn ensure_app(conn: &Connection, id: &str) {
        conn.execute(
            "INSERT OR IGNORE INTO applications (id, source, source_id, title, version, created_at, updated_at)
             VALUES (?1, 'local_project', ?1, 'Test', '1', 't', 't')",
            params![id],
        )
        .unwrap();
    }

    #[test]
    fn oauth_domain_add_and_check() {
        let conn = fixture();
        ensure_app(&conn, "app-1");
        add_oauth_domain(&conn, "app-1", "accounts.google.com").unwrap();
        assert!(is_oauth_domain_allowed(&conn, "app-1", "accounts.google.com").unwrap());
        assert!(!is_oauth_domain_allowed(&conn, "app-1", "evil.com").unwrap());
    }

    #[test]
    fn grant_default_deny() {
        let conn = fixture();
        ensure_app(&conn, "app-1");
        assert!(!is_grant_allowed(&conn, "app-1", AppGrant::KIND_CLIPBOARD).unwrap());
    }

    #[test]
    fn grant_persistent_allowed() {
        let conn = fixture();
        ensure_app(&conn, "app-1");
        set_grant(&conn, "app-1", AppGrant::KIND_UPLOAD, AppGrant::POLICY_PERSISTENT, None).unwrap();
        assert!(is_grant_allowed(&conn, "app-1", AppGrant::KIND_UPLOAD).unwrap());
    }

    #[test]
    fn grant_one_time_consumed() {
        let conn = fixture();
        ensure_app(&conn, "app-1");
        set_grant(&conn, "app-1", AppGrant::KIND_DOWNLOAD, AppGrant::POLICY_ONE_TIME, None).unwrap();
        assert!(is_grant_allowed(&conn, "app-1", AppGrant::KIND_DOWNLOAD).unwrap());
        // Second call should return false (one-time consumed)
        assert!(!is_grant_allowed(&conn, "app-1", AppGrant::KIND_DOWNLOAD).unwrap());
    }

    #[test]
    fn grant_list_and_delete() {
        let conn = fixture();
        ensure_app(&conn, "app-1");
        set_grant(&conn, "app-1", AppGrant::KIND_CLIPBOARD, AppGrant::POLICY_PERSISTENT, None).unwrap();
        let grants = list_grants(&conn, "app-1").unwrap();
        assert_eq!(grants.len(), 1);
        delete_grant(&conn, &grants[0].id).unwrap();
        assert!(list_grants(&conn, "app-1").unwrap().is_empty());
    }
}