//! Profile rows and CRUD for the Harness SQLite store.
//!
//! Split out of `repository.rs` during the modular-architecture remediation so
//! no single file owns the whole harness schema. `ProfileRow` is the row shape
//! of `harness_profile`; every profile read/write lives here. Callers keep
//! using `harness::repository::*` via the parent module's re-exports.

use super::{now, sql, DEFAULT_GLOBAL_PROFILE_ID};
use crate::rpc::harness::HarnessError;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;

/// A logical profile or overlay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileRow {
    pub id: String,
    pub name: String,
    pub description: String,
    pub kind: String,
    pub project_id: Option<String>,
    pub current_published_version_id: Option<String>,
    pub archived_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl ProfileRow {
    pub fn to_json(&self) -> Value {
        serde_json::json!({
            "id": self.id,
            "name": self.name,
            "description": self.description,
            "kind": self.kind,
            "project_id": self.project_id,
            "current_published_version_id": self.current_published_version_id,
            "archived_at": self.archived_at,
            "created_at": self.created_at,
            "updated_at": self.updated_at,
        })
    }
}

// ── profiles ────────────────────────────────────────────────────────────────

fn profile_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProfileRow> {
    Ok(ProfileRow {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        kind: row.get(3)?,
        project_id: row.get(4)?,
        current_published_version_id: row.get(5)?,
        archived_at: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

const PROFILE_COLUMNS: &str = "id, name, description, kind, project_id, \
                               current_published_version_id, archived_at, created_at, updated_at";

pub fn list_profiles(
    conn: &Connection,
    include_archived: bool,
) -> Result<Vec<ProfileRow>, HarnessError> {
    let query = format!(
        "SELECT {PROFILE_COLUMNS} FROM harness_profile
          WHERE (?1 = 1 OR archived_at IS NULL)
          ORDER BY kind ASC, name ASC, id ASC"
    );
    let mut stmt = conn.prepare(&query).map_err(sql)?;
    let rows = stmt
        .query_map(params![i64::from(include_archived)], profile_from_row)
        .map_err(sql)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(sql)
}

pub fn get_profile(conn: &Connection, id: &str) -> Result<Option<ProfileRow>, HarnessError> {
    let query = format!("SELECT {PROFILE_COLUMNS} FROM harness_profile WHERE id = ?1");
    conn.query_row(&query, params![id], profile_from_row)
        .optional()
        .map_err(sql)
}

pub fn insert_profile(
    conn: &Connection,
    id: &str,
    name: &str,
    description: &str,
    kind: &str,
    project_id: Option<&str>,
) -> Result<ProfileRow, HarnessError> {
    let stamp = now();
    conn.execute(
        "INSERT INTO harness_profile
            (id, name, description, kind, project_id, current_published_version_id,
             created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6, ?6)",
        params![id, name, description, kind, project_id, stamp],
    )
    .map_err(sql)?;
    get_profile(conn, id)?.ok_or_else(|| HarnessError::internal("profile vanished after insert"))
}

pub fn archive_profile(conn: &Connection, id: &str) -> Result<(), HarnessError> {
    if id == DEFAULT_GLOBAL_PROFILE_ID {
        return Err(HarnessError::invalid(
            "the seeded global template cannot be archived; every Run needs a global layer",
        ));
    }
    let changed = conn
        .execute(
            "UPDATE harness_profile SET archived_at = ?2, updated_at = ?2
              WHERE id = ?1 AND archived_at IS NULL",
            params![id, now()],
        )
        .map_err(sql)?;
    if changed == 0 {
        return Err(HarnessError::not_found(format!(
            "profile not found or already archived: {id}"
        )));
    }
    // A bound-but-archived profile would resolve into a Run silently. Drop the
    // bindings in the same transaction as the archive.
    conn.execute(
        "DELETE FROM harness_binding WHERE profile_id = ?1 AND scope_type != 'global'",
        params![id],
    )
    .map_err(sql)?;
    Ok(())
}
