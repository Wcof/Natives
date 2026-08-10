//! Version and draft rows plus their CRUD for the Harness SQLite store.
//!
//! Split out of `repository.rs` during the modular-architecture remediation.
//! A published version is immutable; a draft carries a `revision` so a stale
//! save is a conflict rather than a silent overwrite (see the parent module's
//! doc comment). Callers keep using `harness::repository::*` via the parent
//! module's re-exports.

use super::repository_profile::get_profile;
use super::repository_source::publish_source_drift_manifest;
use super::{now, sql};
use crate::harness::HarnessError;
use harness_core::blueprint::HarnessBlueprint;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;

/// An immutable published version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionRow {
    pub id: String,
    pub profile_id: String,
    pub version_number: i64,
    pub parent_version_id: Option<String>,
    pub document_json: String,
    pub canonical_hash: String,
    pub validation_summary_json: String,
    pub created_at: String,
}

impl VersionRow {
    pub fn document(&self) -> Result<HarnessBlueprint, HarnessError> {
        let value: Value = serde_json::from_str(&self.document_json)
            .map_err(|e| HarnessError::internal(format!("stored document is not JSON: {e}")))?;
        HarnessBlueprint::parse(&value).map_err(HarnessError::validation_failed)
    }

    /// Metadata only — the document body is fetched separately so a version
    /// list stays small.
    pub fn to_summary_json(&self) -> Value {
        serde_json::json!({
            "id": self.id,
            "profile_id": self.profile_id,
            "version_number": self.version_number,
            "parent_version_id": self.parent_version_id,
            "canonical_hash": self.canonical_hash,
            "created_at": self.created_at,
        })
    }
}

/// The editable state in front of a profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftRow {
    pub profile_id: String,
    pub base_version_id: Option<String>,
    pub document_json: String,
    pub revision: i64,
    pub updated_at: String,
    pub source_candidate_json: Option<String>,
}

impl DraftRow {
    pub fn document(&self) -> Result<HarnessBlueprint, HarnessError> {
        let value: Value = serde_json::from_str(&self.document_json)
            .map_err(|e| HarnessError::internal(format!("stored draft is not JSON: {e}")))?;
        HarnessBlueprint::parse(&value).map_err(HarnessError::validation_failed)
    }

    pub fn to_json(&self) -> Value {
        serde_json::json!({
            "profile_id": self.profile_id,
            "base_version_id": self.base_version_id,
            "document_json": self.document_json,
            "revision": self.revision,
            "updated_at": self.updated_at,
            "source_candidate_json": self.source_candidate_json,
        })
    }
}

// ── versions ────────────────────────────────────────────────────────────────

fn version_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<VersionRow> {
    Ok(VersionRow {
        id: row.get(0)?,
        profile_id: row.get(1)?,
        version_number: row.get(2)?,
        parent_version_id: row.get(3)?,
        document_json: row.get(4)?,
        canonical_hash: row.get(5)?,
        validation_summary_json: row.get(6)?,
        created_at: row.get(7)?,
    })
}

const VERSION_COLUMNS: &str = "id, profile_id, version_number, parent_version_id, \
                               document_json, canonical_hash, validation_summary_json, created_at";

pub fn get_version(conn: &Connection, id: &str) -> Result<Option<VersionRow>, HarnessError> {
    let query = format!("SELECT {VERSION_COLUMNS} FROM harness_version WHERE id = ?1");
    conn.query_row(&query, params![id], version_from_row)
        .optional()
        .map_err(sql)
}

pub fn list_versions(
    conn: &Connection,
    profile_id: &str,
    limit: i64,
) -> Result<Vec<VersionRow>, HarnessError> {
    let query = format!(
        "SELECT {VERSION_COLUMNS} FROM harness_version
          WHERE profile_id = ?1 ORDER BY version_number DESC LIMIT ?2"
    );
    let mut stmt = conn.prepare(&query).map_err(sql)?;
    let rows = stmt
        .query_map(params![profile_id, limit], version_from_row)
        .map_err(sql)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(sql)
}

/// The version a profile currently publishes, if any.
pub fn current_version(
    conn: &Connection,
    profile_id: &str,
) -> Result<Option<VersionRow>, HarnessError> {
    let Some(profile) = get_profile(conn, profile_id)? else {
        return Ok(None);
    };
    match profile.current_published_version_id {
        Some(id) => get_version(conn, &id),
        None => Ok(None),
    }
}

/// Append an immutable version and point the profile at it, atomically.
///
/// `version_number` is derived inside the transaction rather than passed in, so
/// two concurrent publishes cannot both claim the same number — the
/// `UNIQUE(profile_id, version_number)` index turns the loser into an error
/// rather than a lost write.
pub fn publish_version(
    conn: &Connection,
    profile_id: &str,
    document: &HarnessBlueprint,
    validation_summary: &Value,
) -> Result<VersionRow, HarnessError> {
    let tx_started = conn.execute_batch("BEGIN IMMEDIATE").is_ok();
    let result = publish_version_inner(conn, profile_id, document, validation_summary);
    if tx_started {
        let _ = conn.execute_batch(if result.is_ok() { "COMMIT" } else { "ROLLBACK" });
    }
    result
}

pub fn publish_draft_version(
    conn: &Connection,
    profile_id: &str,
    document: &HarnessBlueprint,
    validation_summary: &Value,
    draft: &DraftRow,
) -> Result<VersionRow, HarnessError> {
    let tx_started = conn.execute_batch("BEGIN IMMEDIATE").is_ok();
    let result = (|| {
        let version = publish_version_inner(conn, profile_id, document, validation_summary)?;
        publish_source_drift_manifest(conn, draft)?;
        clear_draft(conn, profile_id)?;
        Ok(version)
    })();
    if tx_started {
        let _ = conn.execute_batch(if result.is_ok() { "COMMIT" } else { "ROLLBACK" });
    }
    result
}

fn publish_version_inner(
    conn: &Connection,
    profile_id: &str,
    document: &HarnessBlueprint,
    validation_summary: &Value,
) -> Result<VersionRow, HarnessError> {
    let parent: Option<String> = conn
        .query_row(
            "SELECT current_published_version_id FROM harness_profile WHERE id = ?1",
            params![profile_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(sql)?
        .flatten();
    let next: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version_number), 0) + 1 FROM harness_version WHERE profile_id = ?1",
            params![profile_id],
            |row| row.get(0),
        )
        .map_err(sql)?;

    let id = format!("hv-{}", uuid::Uuid::new_v4());
    let document_json =
        serde_json::to_string(document).map_err(|e| HarnessError::internal(e.to_string()))?;
    let stamp = now();
    conn.execute(
        "INSERT INTO harness_version
            (id, profile_id, version_number, parent_version_id, document_json,
             canonical_hash, source_manifest_json, validation_summary_json, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, '{}', ?7, ?8)",
        params![
            id,
            profile_id,
            next,
            parent,
            document_json,
            document.canonical_hash(),
            validation_summary.to_string(),
            stamp
        ],
    )
    .map_err(sql)?;
    conn.execute(
        "UPDATE harness_profile SET current_published_version_id = ?2, updated_at = ?3
          WHERE id = ?1",
        params![profile_id, id, stamp],
    )
    .map_err(sql)?;
    get_version(conn, &id)?.ok_or_else(|| HarnessError::internal("version vanished after insert"))
}

// ── drafts ──────────────────────────────────────────────────────────────────

pub fn get_draft(conn: &Connection, profile_id: &str) -> Result<Option<DraftRow>, HarnessError> {
    conn.query_row(
        "SELECT profile_id, base_version_id, document_json, revision, updated_at, source_candidate_json
           FROM harness_draft WHERE profile_id = ?1",
        params![profile_id],
        |row| {
            Ok(DraftRow {
                profile_id: row.get(0)?,
                base_version_id: row.get(1)?,
                document_json: row.get(2)?,
                revision: row.get(3)?,
                updated_at: row.get(4)?,
                source_candidate_json: row.get(5)?,
            })
        },
    )
    .optional()
    .map_err(sql)
}

/// Return the draft, creating one from the current published version if absent.
///
/// Opening an editor must not require a separate "create draft" call: the
/// document a user sees is always either their work in progress or an exact
/// copy of what is live.
pub fn get_or_create_draft(conn: &Connection, profile_id: &str) -> Result<DraftRow, HarnessError> {
    if let Some(existing) = get_draft(conn, profile_id)? {
        return Ok(existing);
    }
    let current = current_version(conn, profile_id)?;
    let document_json = current
        .as_ref()
        .map(|v| v.document_json.clone())
        .unwrap_or_else(|| {
            serde_json::to_string(&HarnessBlueprint::default()).unwrap_or_else(|_| "{}".into())
        });
    conn.execute(
        "INSERT INTO harness_draft (profile_id, base_version_id, document_json, revision, updated_at, source_candidate_json)
         VALUES (?1, ?2, ?3, 0, ?4, NULL)",
        params![
            profile_id,
            current.as_ref().map(|v| v.id.clone()),
            document_json,
            now()
        ],
    )
    .map_err(sql)?;
    get_draft(conn, profile_id)?
        .ok_or_else(|| HarnessError::internal("draft vanished after insert"))
}

/// Save a draft under optimistic concurrency.
///
/// `expected_revision` is the revision the editor last read. A mismatch is
/// reported as a conflict and nothing is written — design 第 16 节 requires
/// "return conflict and diff; never overwrite".
pub fn save_draft(
    conn: &Connection,
    profile_id: &str,
    document: &HarnessBlueprint,
    expected_revision: i64,
) -> Result<DraftRow, HarnessError> {
    let existing = get_or_create_draft(conn, profile_id)?;
    if existing.revision != expected_revision {
        return Err(HarnessError::draft_conflict(format!(
            "draft revision is {}, not {expected_revision}; reload before saving",
            existing.revision
        )));
    }
    let document_json =
        serde_json::to_string(document).map_err(|e| HarnessError::internal(e.to_string()))?;
    let changed = conn
        .execute(
            "UPDATE harness_draft
                SET document_json = ?2, revision = revision + 1, updated_at = ?3
              WHERE profile_id = ?1 AND revision = ?4",
            params![profile_id, document_json, now(), expected_revision],
        )
        .map_err(sql)?;
    if changed == 0 {
        return Err(HarnessError::draft_conflict(
            "draft changed while saving; reload before saving",
        ));
    }
    get_draft(conn, profile_id)?
        .ok_or_else(|| HarnessError::internal("draft vanished after update"))
}

/// Drop a draft once it has been published. The published version is the record.
pub fn clear_draft(conn: &Connection, profile_id: &str) -> Result<(), HarnessError> {
    conn.execute(
        "DELETE FROM harness_draft WHERE profile_id = ?1",
        params![profile_id],
    )
    .map_err(sql)?;
    Ok(())
}
