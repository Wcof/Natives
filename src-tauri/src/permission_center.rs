use crate::{Error, Result};
use rusqlite::OptionalExtension;
use rusqlite::Connection;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct PermissionRecord {
    pub module_id: String,
    pub permission: String,
    pub granted: i32,
}

#[derive(Debug, Serialize)]
pub struct AuditEntry {
    pub id: i64,
    pub module_id: String,
    pub permission: String,
    pub action: String,
    pub granted: i32,
    pub reason: Option<String>,
    pub created_at: String,
}

/// List permissions for a module
pub fn list_permissions(conn: &Connection, module_id: &str) -> Result<Vec<PermissionRecord>> {
    let mut stmt = conn
        .prepare(
            "SELECT module_id, permission, granted FROM module_permissions WHERE module_id = ?1 ORDER BY permission",
        )
        .map_err(Error::Database)?;
    let mut results = Vec::new();
    let mut rows = stmt.query(rusqlite::params![module_id]).map_err(Error::Database)?;
    while let Some(row) = rows.next().map_err(Error::Database)? {
        results.push(PermissionRecord {
            module_id: row.get(0).map_err(Error::Database)?,
            permission: row.get(1).map_err(Error::Database)?,
            granted: row.get(2).map_err(Error::Database)?,
        });
    }
    Ok(results)
}

/// Grant a permission with audit logging
pub fn grant_permission(
    conn: &Connection,
    module_id: &str,
    permission: &str,
    reason: Option<&str>,
) -> Result<()> {
    conn.execute(
        "UPDATE module_permissions SET granted = 1 WHERE module_id = ?1 AND permission = ?2",
        rusqlite::params![module_id, permission],
    )
    .map_err(Error::Database)?;

    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO permission_audit_log (module_id, permission, action, granted, reason, created_at) VALUES (?1, ?2, 'grant', 1, ?3, ?4)",
        rusqlite::params![module_id, permission, reason, now],
    ).map_err(Error::Database)?;

    Ok(())
}

/// Revoke a permission with audit logging
pub fn revoke_permission(
    conn: &Connection,
    module_id: &str,
    permission: &str,
    reason: Option<&str>,
) -> Result<()> {
    conn.execute(
        "UPDATE module_permissions SET granted = 0 WHERE module_id = ?1 AND permission = ?2",
        rusqlite::params![module_id, permission],
    )
    .map_err(Error::Database)?;

    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO permission_audit_log (module_id, permission, action, granted, reason, created_at) VALUES (?1, ?2, 'revoke', 0, ?3, ?4)",
        rusqlite::params![module_id, permission, reason, now],
    ).map_err(Error::Database)?;

    Ok(())
}

/// Approve all pending permissions for a module (bulk grant)
pub fn approve_all_permissions(
    conn: &Connection,
    module_id: &str,
    reason: Option<&str>,
) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    let reason_text = reason.unwrap_or("Bulk approval on install");

    // Get all ungranted permissions
    let pending: Vec<String> = {
        let mut stmt = conn
            .prepare("SELECT permission FROM module_permissions WHERE module_id = ?1 AND granted = 0")
            .map_err(Error::Database)?;
        let mut rows = stmt.query(rusqlite::params![module_id]).map_err(Error::Database)?;
        let mut perms = Vec::new();
        while let Some(row) = rows.next().map_err(Error::Database)? {
            perms.push(row.get::<_, String>(0).map_err(Error::Database)?);
        }
        perms
    };

    // Bulk grant in transaction
    let tx = conn.unchecked_transaction().map_err(Error::Database)?;
    for perm in &pending {
        tx.execute(
            "UPDATE module_permissions SET granted = 1 WHERE module_id = ?1 AND permission = ?2",
            rusqlite::params![module_id, perm],
        )
        .map_err(Error::Database)?;
        tx.execute(
            "INSERT INTO permission_audit_log (module_id, permission, action, granted, reason, created_at) VALUES (?1, ?2, 'approve', 1, ?3, ?4)",
            rusqlite::params![module_id, perm, reason_text, now],
        ).map_err(Error::Database)?;
    }
    tx.commit().map_err(Error::Database)?;

    Ok(())
}

/// Get audit log, optionally filtered by module
pub fn get_audit_log(
    conn: &Connection,
    module_id: Option<&str>,
    limit: i64,
) -> Result<Vec<AuditEntry>> {
    let (sql, params): (&str, Vec<Box<dyn rusqlite::types::ToSql>>) = match module_id {
        Some(mid) => (
            "SELECT id, module_id, permission, action, granted, reason, created_at FROM permission_audit_log WHERE module_id = ?1 ORDER BY id DESC LIMIT ?2",
            vec![
                Box::new(mid.to_string()) as Box<dyn rusqlite::types::ToSql>,
                Box::new(limit),
            ],
        ),
        None => (
            "SELECT id, module_id, permission, action, granted, reason, created_at FROM permission_audit_log ORDER BY id DESC LIMIT ?1",
            vec![Box::new(limit) as Box<dyn rusqlite::types::ToSql>],
        ),
    };

    let mut stmt = conn.prepare(sql).map_err(Error::Database)?;
    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut results = Vec::new();
    let mut rows = stmt.query(param_refs.as_slice()).map_err(Error::Database)?;
    while let Some(row) = rows.next().map_err(Error::Database)? {
        results.push(AuditEntry {
            id: row.get(0).map_err(Error::Database)?,
            module_id: row.get(1).map_err(Error::Database)?,
            permission: row.get(2).map_err(Error::Database)?,
            action: row.get(3).map_err(Error::Database)?,
            granted: row.get(4).map_err(Error::Database)?,
            reason: row.get(5).map_err(Error::Database)?,
            created_at: row.get(6).map_err(Error::Database)?,
        });
    }
    Ok(results)
}

/// Check if a module has a specific permission granted
#[allow(dead_code)]
pub fn check_permission(conn: &Connection, module_id: &str, permission: &str) -> Result<bool> {
    let mut stmt = conn
        .prepare("SELECT granted FROM module_permissions WHERE module_id = ?1 AND permission = ?2")
        .map_err(Error::Database)?;
    let result: Option<i32> = stmt
        .query_row(rusqlite::params![module_id, permission], |row| {
            row.get(0)
        })
        .optional()
        .map_err(Error::Database)?;
    Ok(result == Some(1))
}
