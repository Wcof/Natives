//! Grant store (batch 6 CR-602/603, T08 production hardening).
//!
//! OAuth allowlist and per-app capability grants. All grants default to deny —
//! the user must explicitly approve each capability.
//!
//! T08: `check_grant` is the single decision entry (default deny, one-time
//! grants consumed atomically, persistent grants path-scope checked) and
//! `grant_events` records the lifecycle for the permission-history UI.

use super::model::{AppGrant, GrantEvent, OAuthAllowlistEntry};
use crate::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Outcome of a grant decision (see [`check_grant`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GrantOutcome {
    /// Persistent grant; path scope satisfied (if any was required).
    AllowedPersistent,
    /// One-time grant was available and has been atomically consumed.
    AllowedOneTimeConsumed,
    /// No grant row, or the row policy is `default_deny`.
    DeniedNoGrant,
    /// A grant exists but the requested path is outside its configured scope.
    DeniedScopeMismatch,
    /// A one-time grant was already consumed by a concurrent check.
    DeniedConcurrent,
}

impl GrantOutcome {
    pub fn allowed(&self) -> bool {
        matches!(self, Self::AllowedPersistent | Self::AllowedOneTimeConsumed)
    }
}

// ── OAuth Allowlist ─────────────────────────────────────────────────

/// Add an allowed domain for OAuth popup.
pub fn add_oauth_domain(conn: &Connection, application_id: &str, domain: &str) -> Result<String> {
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
pub fn is_oauth_domain_allowed(
    conn: &Connection,
    application_id: &str,
    domain: &str,
) -> Result<bool> {
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

/// Set a grant policy for an application+kind. Records a `set` history event.
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
        params![
            Uuid::new_v4().to_string(),
            application_id,
            kind,
            policy,
            path,
            t
        ],
    )
    .map_err(Error::Database)?;
    record_grant_event(conn, application_id, kind, "set", Some(policy), path)?;
    Ok(())
}

/// Get the grant policy for an application+kind. Returns default_deny if not set.
pub fn get_grant(conn: &Connection, application_id: &str, kind: &str) -> Result<AppGrant> {
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
    .ok_or_else(|| Error::NotFound(format!("grant {kind} for app {application_id}")))
}

/// The configured path scope for an application+kind grant (no consumption).
pub fn grant_scope(conn: &Connection, application_id: &str, kind: &str) -> Result<Option<String>> {
    conn.query_row(
        "SELECT path FROM app_grants WHERE application_id = ?1 AND kind = ?2",
        params![application_id, kind],
        |row| row.get(0),
    )
    .optional()
    .map_err(Error::Database)
}

/// Lexically normalize a path for scope comparison (no filesystem access).
/// Resolves `.`/`..` segments; a `..` above root is clamped.
fn normalize_scope_path(p: &str) -> String {
    let normalized = p.replace('\\', "/");
    let mut out: Vec<&str> = Vec::new();
    for seg in normalized.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            s => out.push(s),
        }
    }
    let joined = out.join("/");
    if joined.is_empty() {
        "/".to_string()
    } else {
        format!("/{joined}")
    }
}

/// Whether `requested` is the scope directory itself or a descendant.
/// `..` traversal cannot escape the scope because both sides are normalized.
pub fn is_path_within_scope(requested: &str, scope: &str) -> bool {
    let req = normalize_scope_path(requested);
    let sc = normalize_scope_path(scope);
    req == sc || req.starts_with(&format!("{sc}/"))
}

/// Check a grant for `application_id + kind`. One-time grants are consumed
/// atomically (a single `UPDATE ... WHERE policy='one_time'`), so concurrent
/// checks can never both succeed. Persistent grants are path-scope checked.
///
/// This is the single decision entry used by WebView policy handlers and
/// commands. It records no history event on the deny path — a denied check is
/// side-effect free at the store level.
pub fn check_grant(
    conn: &Connection,
    application_id: &str,
    kind: &str,
    requested_path: Option<&str>,
) -> Result<GrantOutcome> {
    let row: Option<(String, String, Option<String>)> = conn
        .query_row(
            "SELECT id, policy, path FROM app_grants
             WHERE application_id = ?1 AND kind = ?2",
            params![application_id, kind],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(Error::Database)?;

    let Some((grant_id, policy, scope)) = row else {
        return Ok(GrantOutcome::DeniedNoGrant);
    };

    let scope_ok = match (requested_path, scope.as_deref()) {
        (_, None) => true,
        (Some(requested), Some(scope)) => is_path_within_scope(requested, scope),
        // A scope is required but the caller did not provide a path.
        (None, Some(_)) => false,
    };

    match policy.as_str() {
        AppGrant::POLICY_PERSISTENT => {
            if scope_ok {
                Ok(GrantOutcome::AllowedPersistent)
            } else {
                Ok(GrantOutcome::DeniedScopeMismatch)
            }
        }
        AppGrant::POLICY_ONE_TIME => {
            // Atomic consumption: the UPDATE matches the row only while it is
            // still `one_time`. A concurrent consumer sees 0 rows changed.
            let consumed = conn
                .execute(
                    "UPDATE app_grants
                     SET policy = ?1, path = NULL, updated_at = ?2
                     WHERE id = ?3 AND policy = ?4",
                    params![
                        AppGrant::POLICY_DEFAULT_DENY,
                        now(),
                        grant_id,
                        AppGrant::POLICY_ONE_TIME
                    ],
                )
                .map_err(Error::Database)?;
            if consumed == 0 {
                return Ok(GrantOutcome::DeniedConcurrent);
            }
            record_grant_event(
                conn,
                application_id,
                kind,
                "consumed",
                Some(&policy),
                scope.as_deref(),
            )?;
            if scope_ok {
                Ok(GrantOutcome::AllowedOneTimeConsumed)
            } else {
                Ok(GrantOutcome::DeniedScopeMismatch)
            }
        }
        _ => Ok(GrantOutcome::DeniedNoGrant),
    }
}

/// Check if a grant is allowed (persistent or one_time). One-time grants are
/// atomically consumed on success. Kept for compatibility; prefer
/// [`check_grant`] when a requested path is involved.
pub fn is_grant_allowed(conn: &Connection, application_id: &str, kind: &str) -> Result<bool> {
    Ok(check_grant(conn, application_id, kind, None)?.allowed())
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

/// Delete a grant. Records a `revoked` history event.
pub fn delete_grant(conn: &Connection, grant_id: &str) -> Result<()> {
    let row: Option<(String, String)> = conn
        .query_row(
            "SELECT application_id, kind FROM app_grants WHERE id = ?1",
            params![grant_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(Error::Database)?;
    let Some((application_id, kind)) = row else {
        return Ok(());
    };
    conn.execute("DELETE FROM app_grants WHERE id = ?1", params![grant_id])
        .map_err(Error::Database)?;
    record_grant_event(conn, &application_id, &kind, "revoked", None, None)?;
    Ok(())
}

// ── Grant history ────────────────────────────────────────────────────

fn record_grant_event(
    conn: &Connection,
    application_id: &str,
    kind: &str,
    event: &str,
    policy: Option<&str>,
    path: Option<&str>,
) -> Result<()> {
    conn.execute(
        "INSERT INTO grant_events (id, application_id, kind, event, policy, path, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            Uuid::new_v4().to_string(),
            application_id,
            kind,
            event,
            policy,
            path,
            now()
        ],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// List the grant lifecycle history for an application (newest first).
pub fn list_grant_events(
    conn: &Connection,
    application_id: &str,
    limit: i64,
) -> Result<Vec<GrantEvent>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, application_id, kind, event, policy, path, created_at
             FROM grant_events WHERE application_id = ?1
             ORDER BY created_at DESC, rowid DESC LIMIT ?2",
        )
        .map_err(Error::Database)?;
    let rows = stmt
        .query_map(params![application_id, limit], |row| {
            Ok(GrantEvent {
                id: row.get(0)?,
                application_id: row.get(1)?,
                kind: row.get(2)?,
                event: row.get(3)?,
                policy: row.get(4)?,
                path: row.get(5)?,
                created_at: row.get(6)?,
            })
        })
        .map_err(Error::Database)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(Error::Database)?);
    }
    Ok(out)
}

#[cfg(test)]
#[path = "grant_store_tests.rs"]
mod grant_store_tests;
