//! SQLite persistence for the Harness control plane.
//!
//! Every table lives in the Daemon-owned `assistant.db` (migration 022). This
//! module is the only place that knows the schema; `control_plane` speaks in
//! domain types and never writes SQL.
//!
//! Three storage decisions worth stating once, because the rest of the module
//! depends on them:
//!
//! - **A published version is immutable.** Rollback re-publishes an old
//!   document as a *new* version rather than moving a pointer backwards, so a
//!   Run snapshot's `version_id` always resolves to the bytes that Run used.
//! - **A draft carries a `revision`.** Saving with a stale revision is a
//!   conflict, never a silent overwrite of another editor's work.
//! - **The default global template is seeded, not assumed.** `ensure_defaults`
//!   is idempotent and runs before every read, so a fresh install and an
//!   upgraded one reach the same state without a bootstrap RPC.

use super::HarnessError;
use crate::storage::DataStore;
use harness_core::blueprint::HarnessBlueprint;
use harness_core::resolver::ProfileLayer;
use harness_core::snapshot::ResolvedHarnessSnapshot;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
use std::path::PathBuf;

/// Identity of the seeded global template. Fixed so re-seeding is a no-op.
pub const DEFAULT_GLOBAL_PROFILE_ID: &str = "harness.global.default";
/// The single global binding row's scope id.
pub const GLOBAL_SCOPE_ID: &str = "global";

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

/// Which published profile a scope selects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingRow {
    pub scope_type: String,
    pub scope_id: String,
    pub profile_id: String,
    /// Set only when `mode = pinned`.
    pub version_id: Option<String>,
    pub mode: String,
    pub updated_at: String,
}

impl BindingRow {
    pub fn to_json(&self) -> Value {
        serde_json::json!({
            "scope_type": self.scope_type,
            "scope_id": self.scope_id,
            "profile_id": self.profile_id,
            "version_id": self.version_id,
            "mode": self.mode,
            "updated_at": self.updated_at,
        })
    }
}

/// Open the Daemon-owned `assistant.db`.
///
/// Duplicated from `conversation_store::store` on purpose: that function is
/// private to its module, and every store in this crate carries its own copy.
/// Unifying them is a worthwhile cleanup, but it touches five modules owned by
/// other workstreams and does not belong in this change.
pub fn store() -> Result<DataStore, String> {
    #[cfg(test)]
    let _env_guard = crate::storage::DataStore::env_test_lock();
    #[cfg(test)]
    if let Some((db_path, artifact_dir)) = crate::storage::test_db_override() {
        if let Some(parent) = db_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        return DataStore::new(&db_path, &artifact_dir);
    }
    let db_path = std::env::var("NATIVES_ASSISTANT_DB_PATH")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var("NATIVES_DB_PATH")
                .ok()
                .filter(|s| !s.trim().is_empty())
                .map(PathBuf::from)
        });
    #[cfg(test)]
    let db_path = db_path.ok_or_else(|| {
        "test store() requires NATIVES_ASSISTANT_DB_PATH or NATIVES_DB_PATH \
         (refusing ~/.natives default)"
            .to_string()
    })?;
    #[cfg(not(test))]
    let db_path = db_path.unwrap_or_else(crate::default_assistant_db_path);
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let artifact_dir = std::env::var("NATIVES_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            PathBuf::from(home).join(".natives").join("runtime")
        })
        .join("artifacts");
    DataStore::new(&db_path, &artifact_dir)
}

/// Run `f` against an open connection with defaults already seeded.
pub fn with_conn<T>(
    f: impl FnOnce(&Connection) -> Result<T, HarnessError>,
) -> Result<T, HarnessError> {
    let store = store().map_err(HarnessError::internal)?;
    let conn = store.conn().map_err(HarnessError::internal)?;
    ensure_defaults(&conn)?;
    f(&conn)
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn sql(e: rusqlite::Error) -> HarnessError {
    HarnessError::internal(format!("harness storage: {e}"))
}

/// Seed the default global template and its binding. Idempotent.
///
/// The seeded document is [`HarnessBlueprint::default`] — no overlays at all —
/// which is the only document that provably compiles to today's production
/// behaviour (design 第 5.3 节).
pub fn ensure_defaults(conn: &Connection) -> Result<(), HarnessError> {
    reconcile_legacy_project_identities(conn)?;
    let exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM harness_profile WHERE id = ?1",
            params![DEFAULT_GLOBAL_PROFILE_ID],
            |row| row.get(0),
        )
        .map_err(sql)?;
    if exists {
        return Ok(());
    }

    let document = HarnessBlueprint::default();
    let document_json =
        serde_json::to_string(&document).map_err(|e| HarnessError::internal(e.to_string()))?;
    let version_id = format!("{DEFAULT_GLOBAL_PROFILE_ID}.v1");
    let stamp = now();

    conn.execute(
        "INSERT OR IGNORE INTO harness_profile
            (id, name, description, kind, project_id, current_published_version_id,
             created_at, updated_at)
         VALUES (?1, ?2, ?3, 'global_template', NULL, ?4, ?5, ?5)",
        params![
            DEFAULT_GLOBAL_PROFILE_ID,
            "Default Native Harness",
            "Seeded global template. No overlays: resolves to the engine's own defaults.",
            version_id,
            stamp
        ],
    )
    .map_err(sql)?;
    conn.execute(
        "INSERT OR IGNORE INTO harness_version
            (id, profile_id, version_number, parent_version_id, document_json,
             canonical_hash, source_manifest_json, validation_summary_json, created_at)
         VALUES (?1, ?2, 1, NULL, ?3, ?4, '{}', '{\"findings\":[]}', ?5)",
        params![
            version_id,
            DEFAULT_GLOBAL_PROFILE_ID,
            document_json,
            document.canonical_hash(),
            stamp
        ],
    )
    .map_err(sql)?;
    conn.execute(
        "INSERT OR IGNORE INTO harness_binding
            (scope_type, scope_id, profile_id, version_id, mode, updated_at)
         VALUES ('global', ?1, ?2, NULL, 'follow_published', ?3)",
        params![GLOBAL_SCOPE_ID, DEFAULT_GLOBAL_PROFILE_ID, stamp],
    )
    .map_err(sql)?;
    Ok(())
}

/// Migration 025 briefly introduced a Harness-specific project identity table.
/// Rebind any rows it created to the RunManager's existing ProjectIdentity
/// authority; missing directories remain untouched and therefore fail closed.
fn reconcile_legacy_project_identities(conn: &Connection) -> Result<(), HarnessError> {
    let mut stmt = conn
        .prepare(
            "SELECT project_id, canonical_path FROM harness_project_identity
              WHERE EXISTS (
                SELECT 1 FROM harness_profile WHERE project_id = harness_project_identity.project_id
              )",
        )
        .map_err(sql)?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(sql)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(sql)?;
    drop(stmt);

    for (legacy_id, path) in rows {
        let Ok(identity) = crate::project_identity::store::register_or_get(conn, &path) else {
            continue;
        };
        if identity.project_id == legacy_id {
            continue;
        }
        conn.execute(
            "UPDATE harness_profile SET project_id = ?2 WHERE project_id = ?1",
            params![legacy_id, identity.project_id],
        )
        .map_err(sql)?;
        conn.execute(
            "DELETE FROM harness_binding
              WHERE scope_type = 'project' AND scope_id = ?1
                AND EXISTS (
                    SELECT 1 FROM harness_binding
                     WHERE scope_type = 'project' AND scope_id = ?2
                )",
            params![legacy_id, identity.project_id],
        )
        .map_err(sql)?;
        conn.execute(
            "UPDATE harness_binding SET scope_id = ?2
              WHERE scope_type = 'project' AND scope_id = ?1",
            params![legacy_id, identity.project_id],
        )
        .map_err(sql)?;
    }
    Ok(())
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

/// Create one drift candidate from the currently published document. The
/// candidate is metadata-only: source bodies never cross this boundary.
pub fn ensure_source_drift_candidate(
    conn: &Connection,
    profile_id: &str,
    mismatches: &Value,
) -> Result<Option<DraftRow>, HarnessError> {
    if mismatches.as_array().is_none_or(Vec::is_empty) {
        return Ok(None);
    }
    let candidate_json = mismatches.to_string();
    if let Some(existing) = get_draft(conn, profile_id)? {
        let same_candidate = existing
            .source_candidate_json
            .as_deref()
            .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
            .and_then(|value| value.as_array().cloned())
            .is_some_and(|items| {
                let mut existing_keys = items
                    .iter()
                    .filter_map(source_candidate_key)
                    .collect::<Vec<_>>();
                let mut observed_keys = mismatches
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(source_candidate_key)
                    .collect::<Vec<_>>();
                existing_keys.sort();
                observed_keys.sort();
                existing_keys == observed_keys
            });
        if same_candidate {
            return Ok(Some(existing));
        }
        return Err(HarnessError::draft_conflict(
            "an unpublished Draft already exists; reload before acknowledging source drift",
        ));
    }
    let current = current_version(conn, profile_id)?
        .ok_or_else(|| HarnessError::not_found("published Harness profile has no version"))?;
    conn.execute(
        "INSERT INTO harness_draft
           (profile_id, base_version_id, document_json, revision, updated_at, source_candidate_json)
         VALUES (?1, ?2, ?3, 0, ?4, ?5)",
        params![
            profile_id,
            current.id,
            current.document_json,
            now(),
            candidate_json
        ],
    )
    .map_err(sql)?;
    Ok(Some(get_draft(conn, profile_id)?.ok_or_else(|| {
        HarnessError::internal("drift candidate vanished")
    })?))
}

fn source_candidate_key(value: &Value) -> Option<(String, String)> {
    Some((
        value.get("source_id")?.as_str()?.to_string(),
        value.get("observed_digest")?.as_str()?.to_string(),
    ))
}

pub fn acknowledge_source_drift(
    conn: &Connection,
    profile_id: &str,
    source_id: &str,
    observed_digest: &str,
    expected_revision: i64,
) -> Result<DraftRow, HarnessError> {
    let tx_started = conn.execute_batch("BEGIN IMMEDIATE").is_ok();
    let result = acknowledge_source_drift_inner(
        conn,
        profile_id,
        source_id,
        observed_digest,
        expected_revision,
    );
    if tx_started {
        let _ = conn.execute_batch(if result.is_ok() { "COMMIT" } else { "ROLLBACK" });
    }
    result
}

fn acknowledge_source_drift_inner(
    conn: &Connection,
    profile_id: &str,
    source_id: &str,
    observed_digest: &str,
    expected_revision: i64,
) -> Result<DraftRow, HarnessError> {
    let draft = get_draft(conn, profile_id)?
        .ok_or_else(|| HarnessError::not_found("no drift candidate exists"))?;
    if draft.revision != expected_revision {
        return Err(HarnessError::draft_conflict(
            "draft revision changed; reload before acknowledging drift",
        ));
    }
    let candidate = draft
        .source_candidate_json
        .as_deref()
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
        .ok_or_else(|| HarnessError::invalid("draft has no source drift candidate"))?;
    let items = candidate
        .as_array()
        .ok_or_else(|| HarnessError::invalid("source drift candidate is not an array"))?;
    if !items.iter().any(|item| {
        item.get("source_id").and_then(Value::as_str) == Some(source_id)
            && item.get("observed_digest").and_then(Value::as_str) == Some(observed_digest)
            && item.get("acknowledged").and_then(Value::as_bool) != Some(true)
    }) {
        return Err(HarnessError::invalid(
            "observed digest does not match drift candidate",
        ));
    }
    let acknowledged = items
        .iter()
        .cloned()
        .map(|mut item| {
            if item.get("source_id").and_then(Value::as_str) == Some(source_id) {
                if let Some(object) = item.as_object_mut() {
                    object.insert("acknowledged".into(), Value::Bool(true));
                }
            }
            item
        })
        .collect::<Vec<_>>();
    let candidate_json = Value::Array(acknowledged).to_string();
    let changed = conn
        .execute(
            "UPDATE harness_draft
            SET revision = revision + 1, updated_at = ?2, source_candidate_json = ?4
          WHERE profile_id = ?1 AND revision = ?3",
            params![profile_id, now(), expected_revision, candidate_json],
        )
        .map_err(sql)?;
    if changed == 0 {
        return Err(HarnessError::draft_conflict(
            "draft changed while acknowledging source drift",
        ));
    }
    get_draft(conn, profile_id)?
        .ok_or_else(|| HarnessError::internal("draft vanished after acknowledgement"))
}

pub fn publish_source_drift_manifest(
    conn: &Connection,
    draft: &DraftRow,
) -> Result<(), HarnessError> {
    let Some(items) = draft
        .source_candidate_json
        .as_deref()
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
        .and_then(|value| value.as_array().cloned())
    else {
        return Ok(());
    };
    if items
        .iter()
        .any(|item| item.get("acknowledged").and_then(Value::as_bool) != Some(true))
    {
        return Err(HarnessError::validation_failed(
            "tracked source drift must be acknowledged before publishing",
        ));
    }
    for item in items {
        let source_id = item
            .get("source_id")
            .and_then(Value::as_str)
            .ok_or_else(|| HarnessError::invalid("drift candidate source_id is missing"))?;
        let digest = item
            .get("observed_digest")
            .and_then(Value::as_str)
            .ok_or_else(|| HarnessError::invalid("drift candidate digest is missing"))?;
        let changed = conn
            .execute(
                "UPDATE harness_source_manifest
                    SET digest = ?2, status = 'current', updated_at = ?3
                  WHERE source_id = ?1",
                params![source_id, digest, now()],
            )
            .map_err(sql)?;
        if changed == 0 {
            return Err(HarnessError::not_found(format!(
                "source manifest not found: {source_id}"
            )));
        }
    }
    Ok(())
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

// ── bindings ────────────────────────────────────────────────────────────────

fn binding_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<BindingRow> {
    Ok(BindingRow {
        scope_type: row.get(0)?,
        scope_id: row.get(1)?,
        profile_id: row.get(2)?,
        version_id: row.get(3)?,
        mode: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

pub fn get_binding(
    conn: &Connection,
    scope_type: &str,
    scope_id: &str,
) -> Result<Option<BindingRow>, HarnessError> {
    conn.query_row(
        "SELECT scope_type, scope_id, profile_id, version_id, mode, updated_at
           FROM harness_binding WHERE scope_type = ?1 AND scope_id = ?2",
        params![scope_type, scope_id],
        binding_from_row,
    )
    .optional()
    .map_err(sql)
}

pub fn set_binding(
    conn: &Connection,
    scope_type: &str,
    scope_id: &str,
    profile_id: &str,
    version_id: Option<&str>,
    mode: &str,
) -> Result<BindingRow, HarnessError> {
    let Some(profile) = get_profile(conn, profile_id)? else {
        return Err(HarnessError::not_found(format!(
            "profile not found: {profile_id}"
        )));
    };
    if profile.archived_at.is_some() {
        return Err(HarnessError::invalid(format!(
            "profile {profile_id} is archived and cannot be bound"
        )));
    }
    if profile.current_published_version_id.is_none() {
        return Err(HarnessError::invalid(format!(
            "profile {profile_id} has no published version; publish before binding"
        )));
    }
    match scope_type {
        "global" if profile.kind != "global_template" => {
            return Err(HarnessError::scope_mismatch(
                "the global scope accepts only global_template profiles",
            ));
        }
        "project" if profile.kind != "project_overlay" => {
            return Err(HarnessError::scope_mismatch(
                "a project scope accepts only project_overlay profiles",
            ));
        }
        "project" if profile.project_id.as_deref() != Some(scope_id) => {
            return Err(HarnessError::scope_mismatch(format!(
                "profile {profile_id} belongs to project {}, not {scope_id}",
                profile.project_id.as_deref().unwrap_or("none")
            )));
        }
        _ => {}
    }
    if mode == "pinned" {
        let Some(version_id) = version_id else {
            return Err(HarnessError::invalid("mode=pinned requires a version_id"));
        };
        match get_version(conn, version_id)? {
            Some(version) if version.profile_id == profile_id => {}
            Some(_) => {
                return Err(HarnessError::invalid(
                    "version_id belongs to a different profile",
                ))
            }
            None => {
                return Err(HarnessError::not_found(format!(
                    "version not found: {version_id}"
                )))
            }
        }
    }
    conn.execute(
        "INSERT INTO harness_binding (scope_type, scope_id, profile_id, version_id, mode, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(scope_type, scope_id) DO UPDATE SET
            profile_id = excluded.profile_id,
            version_id = excluded.version_id,
            mode = excluded.mode,
            updated_at = excluded.updated_at",
        params![
            scope_type,
            scope_id,
            profile_id,
            if mode == "pinned" { version_id } else { None },
            mode,
            now()
        ],
    )
    .map_err(sql)?;
    get_binding(conn, scope_type, scope_id)?
        .ok_or_else(|| HarnessError::internal("binding vanished after upsert"))
}

/// The version a binding currently resolves to, honouring `mode`.
pub fn version_for_binding(
    conn: &Connection,
    binding: &BindingRow,
) -> Result<Option<VersionRow>, HarnessError> {
    match binding.mode.as_str() {
        "pinned" => match binding.version_id.as_deref() {
            Some(id) => get_version(conn, id),
            None => Ok(None),
        },
        _ => current_version(conn, &binding.profile_id),
    }
}

/// Resolve the layer stack for one Run, weakest first.
///
/// Missing project or session bindings are simply absent layers, which is the
/// frozen hierarchy working as designed (decision B): a project that has never
/// been configured inherits the global template rather than failing.
pub fn resolve_layers(
    conn: &Connection,
    project_id: Option<&str>,
    conversation_id: Option<&str>,
) -> Result<Vec<(ProfileLayer, ProfileRow, VersionRow)>, HarnessError> {
    let mut layers = Vec::new();
    let scopes: [(ProfileLayer, &str, Option<&str>); 3] = [
        (ProfileLayer::Global, "global", Some(GLOBAL_SCOPE_ID)),
        (ProfileLayer::Project, "project", project_id),
        (ProfileLayer::Session, "session", conversation_id),
    ];
    for (layer, scope_type, scope_id) in scopes {
        let Some(scope_id) = scope_id.filter(|s| !s.trim().is_empty()) else {
            continue;
        };
        let Some(binding) = get_binding(conn, scope_type, scope_id)? else {
            if layer == ProfileLayer::Global {
                return Err(HarnessError::not_found(
                    "required global Harness binding is missing",
                ));
            }
            continue;
        };
        let profile = get_profile(conn, &binding.profile_id)?.ok_or_else(|| {
            HarnessError::not_found(format!(
                "Harness binding {scope_type}:{scope_id} points to missing profile {}",
                binding.profile_id
            ))
        })?;
        if profile.archived_at.is_some() {
            return Err(HarnessError::invalid(format!(
                "Harness binding {scope_type}:{scope_id} points to archived profile {}",
                binding.profile_id
            )));
        }
        let version = version_for_binding(conn, &binding)?.ok_or_else(|| {
            HarnessError::not_found(format!(
                "Harness binding {scope_type}:{scope_id} has no resolvable published version"
            ))
        })?;
        layers.push((layer, profile, version));
    }
    Ok(layers)
}

// ── run snapshots ───────────────────────────────────────────────────────────

/// Persist a Run's resolved Harness. Fails loudly; the caller must not start
/// the Run when this errors (design 第 16 节).
pub fn insert_run_snapshot(
    conn: &Connection,
    snapshot: &ResolvedHarnessSnapshot,
) -> Result<(), HarnessError> {
    let snapshot_json = serde_json::to_string(snapshot)
        .map_err(|e| HarnessError::snapshot_persist_failed(e.to_string()))?;
    let existing: Option<String> = conn
        .query_row(
            "SELECT canonical_hash FROM harness_run_snapshot WHERE run_id = ?1",
            params![snapshot.run_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| HarnessError::snapshot_persist_failed(e.to_string()))?;
    if let Some(hash) = existing {
        if hash != snapshot.canonical_hash() {
            return Err(HarnessError::new(
                "harness_snapshot_conflict",
                assistant_protocol::error::ErrorCategory::Conflict,
                format!(
                    "run {} already has a different Harness snapshot",
                    snapshot.run_id
                ),
            ));
        }
        return Ok(());
    }
    conn.execute(
        "INSERT INTO harness_run_snapshot
            (run_id, global_version_id, project_version_id, session_version_id,
             snapshot_json, canonical_hash, topology_version, hook_semantics_version, resolved_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(run_id) DO NOTHING",
        params![
            snapshot.run_id,
            snapshot.version_id_for(ProfileLayer::Global),
            snapshot.version_id_for(ProfileLayer::Project),
            snapshot.version_id_for(ProfileLayer::Session),
            snapshot_json,
            snapshot.canonical_hash(),
            snapshot.topology_version,
            snapshot.hook_semantics_version.as_str(),
            snapshot.resolved_at,
        ],
    )
    .map_err(|e| HarnessError::snapshot_persist_failed(format!("harness storage: {e}")))?;
    Ok(())
}

pub fn get_run_snapshot(
    conn: &Connection,
    run_id: &str,
) -> Result<Option<ResolvedHarnessSnapshot>, HarnessError> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT snapshot_json FROM harness_run_snapshot WHERE run_id = ?1",
            params![run_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(sql)?;
    let Some(raw) = raw else {
        return Ok(None);
    };
    serde_json::from_str(&raw)
        .map(Some)
        .map_err(|e| HarnessError::internal(format!("stored snapshot is not readable: {e}")))
}

// ── audit ───────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub fn append_audit(
    conn: &Connection,
    action: &str,
    profile_id: Option<&str>,
    version_id: Option<&str>,
    scope_type: Option<&str>,
    scope_id: Option<&str>,
    summary: &Value,
) -> Result<(), HarnessError> {
    conn.execute(
        "INSERT INTO harness_audit
            (id, action, profile_id, version_id, scope_type, scope_id, actor, summary_json, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'user', ?7, ?8)",
        params![
            format!("ha-{}", uuid::Uuid::new_v4()),
            action,
            profile_id,
            version_id,
            scope_type,
            scope_id,
            summary.to_string(),
            now()
        ],
    )
    .map_err(sql)?;
    // Migration 026 inserts the notice in this statement's transaction.
    super::publish_notice(0);
    Ok(())
}

pub struct NoticePage {
    pub notices: Vec<Value>,
    pub next_cursor: i64,
    pub reset_required: bool,
}

/// Persist a bounded invalidation notice. Documents, payloads, and secrets are
/// deliberately absent; the receiver refetches an authoritative projection.
pub fn append_notice(
    conn: &Connection,
    kind: &str,
    profile_id: Option<&str>,
    run_id: Option<&str>,
) -> Result<i64, HarnessError> {
    conn.execute(
        "INSERT INTO harness_notice(kind, profile_id, run_id) VALUES(?1, ?2, ?3)",
        params![kind, profile_id, run_id],
    )
    .map_err(sql)?;
    let cursor = conn.last_insert_rowid();
    Ok(cursor)
}

pub fn list_notices(
    conn: &Connection,
    after_cursor: i64,
    limit: i64,
) -> Result<NoticePage, HarnessError> {
    let bounds: (i64, i64) = conn
        .query_row(
            "SELECT COALESCE(MIN(cursor), 0), COALESCE(MAX(cursor), 0) FROM harness_notice",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(sql)?;
    let reset_required = after_cursor > 0 && bounds.0 > 0 && after_cursor < bounds.0 - 1;
    let start = if reset_required {
        bounds.1
    } else {
        after_cursor
    };
    let mut stmt = conn
        .prepare(
            "SELECT cursor, kind, profile_id, run_id, created_at
               FROM harness_notice WHERE cursor > ?1
              ORDER BY cursor ASC LIMIT ?2",
        )
        .map_err(sql)?;
    let rows = stmt
        .query_map(params![start, limit.clamp(1, 200)], |row| {
            Ok(serde_json::json!({
                "cursor": row.get::<_, i64>(0)?,
                "kind": row.get::<_, String>(1)?,
                "profile_id": row.get::<_, Option<String>>(2)?,
                "run_id": row.get::<_, Option<String>>(3)?,
                "created_at": row.get::<_, String>(4)?,
            }))
        })
        .map_err(sql)?;
    let notices = rows.collect::<rusqlite::Result<Vec<_>>>().map_err(sql)?;
    let next_cursor = notices
        .last()
        .and_then(|notice| notice.get("cursor"))
        .and_then(Value::as_i64)
        .unwrap_or(if reset_required {
            bounds.1
        } else {
            after_cursor
        });
    Ok(NoticePage {
        notices,
        next_cursor,
        reset_required,
    })
}

pub fn list_audit(conn: &Connection, limit: i64) -> Result<Vec<Value>, HarnessError> {
    let mut stmt = conn
        .prepare(
            "SELECT id, action, profile_id, version_id, scope_type, scope_id, actor,
                    summary_json, created_at
               FROM harness_audit ORDER BY created_at DESC, id DESC LIMIT ?1",
        )
        .map_err(sql)?;
    let rows = stmt
        .query_map(params![limit], |row| {
            let summary: String = row.get(7)?;
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "action": row.get::<_, String>(1)?,
                "profile_id": row.get::<_, Option<String>>(2)?,
                "version_id": row.get::<_, Option<String>>(3)?,
                "scope_type": row.get::<_, Option<String>>(4)?,
                "scope_id": row.get::<_, Option<String>>(5)?,
                "actor": row.get::<_, String>(6)?,
                "summary": serde_json::from_str::<Value>(&summary).unwrap_or(Value::Null),
                "created_at": row.get::<_, String>(8)?,
            }))
        })
        .map_err(sql)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(sql)
}

pub fn sync_source(
    conn: &Connection,
    source_id: &str,
    digest: &str,
    mode: &str,
) -> Result<bool, HarnessError> {
    let old: Option<(String, String)> = conn
        .query_row(
            "SELECT digest, mode FROM harness_source_manifest WHERE source_id = ?1",
            params![source_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(sql)?;
    let drifted = old.as_ref().is_some_and(|(previous, _)| previous != digest);
    if let Some((_, previous_mode)) = old {
        if drifted && previous_mode == "pinned" {
            let cursor = append_notice(conn, "source_drift", None, None)?;
            super::publish_notice(cursor);
            return Err(HarnessError::invalid(format!(
                "pinned Harness source drifted: {source_id}"
            )));
        }
    }
    conn.execute(
        "INSERT INTO harness_source_manifest(source_id,digest,mode,status,updated_at)
         VALUES(?1,?2,?3,?4,datetime('now'))
         ON CONFLICT(source_id) DO UPDATE SET
           digest=CASE WHEN excluded.status='current' THEN excluded.digest ELSE harness_source_manifest.digest END,
           mode=excluded.mode, status=excluded.status, updated_at=excluded.updated_at",
        params![source_id, digest, mode, if drifted { "drifted" } else { "current" }],
    )
    .map_err(sql)?;
    Ok(drifted)
}

pub fn source_digest(
    conn: &Connection,
    source_id: &str,
) -> Result<Option<(String, String)>, HarnessError> {
    conn.query_row(
        "SELECT digest, mode FROM harness_source_manifest WHERE source_id = ?1",
        params![source_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .optional()
    .map_err(sql)
}

pub fn list_sources(conn: &Connection, limit: i64) -> Result<Vec<Value>, HarnessError> {
    let mut stmt = conn.prepare("SELECT source_id,digest,mode,status,updated_at FROM harness_source_manifest ORDER BY updated_at DESC LIMIT ?1").map_err(sql)?;
    let rows = stmt
        .query_map(params![limit], |row| {
            Ok(serde_json::json!({
                "source_id": row.get::<_, String>(0)?, "digest": row.get::<_, String>(1)?,
                "mode": row.get::<_, String>(2)?, "status": row.get::<_, String>(3)?,
                "updated_at": row.get::<_, String>(4)?
            }))
        })
        .map_err(sql)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(sql)
}

pub fn insert_hook_trace(
    conn: &Connection,
    run_id: &str,
    hook_id: &str,
    phase: &str,
    status: &str,
    duration_ms: Option<u64>,
) -> Result<(), HarnessError> {
    conn.execute("INSERT INTO harness_hook_trace(id,run_id,hook_id,phase,status,duration_ms) VALUES(?1,?2,?3,?4,?5,?6)", params![format!("ht-{}", uuid::Uuid::new_v4()), run_id, hook_id, phase, status, duration_ms.map(|v| v as i64)]).map_err(sql)?;
    Ok(())
}

pub fn list_hook_trace(
    conn: &Connection,
    run_id: Option<&str>,
    limit: i64,
) -> Result<Vec<Value>, HarnessError> {
    let mut stmt = conn.prepare("SELECT id,run_id,hook_id,phase,status,duration_ms,created_at FROM harness_hook_trace WHERE (?1 IS NULL OR run_id=?1) ORDER BY created_at DESC,id DESC LIMIT ?2").map_err(sql)?;
    let rows = stmt.query_map(params![run_id, limit], |row| Ok(serde_json::json!({
        "id": row.get::<_, String>(0)?, "run_id": row.get::<_, String>(1)?, "hook_id": row.get::<_, String>(2)?,
        "phase": row.get::<_, String>(3)?, "status": row.get::<_, String>(4)?, "duration_ms": row.get::<_, Option<i64>>(5)?, "created_at": row.get::<_, String>(6)?
    }))).map_err(sql)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(sql)
}

/// Hook invocation projection from the single durable run_event authority.
pub fn list_run_hook_trace(
    conn: &Connection,
    run_id: Option<&str>,
    after_sequence: i64,
    limit: i64,
) -> Result<Vec<Value>, HarnessError> {
    let capped = limit.clamp(1, 200);
    let mut stmt = conn
        .prepare(
            "SELECT run_id, sequence, payload, timestamp FROM run_event
         WHERE (?1 IS NULL OR run_id = ?1) AND sequence > ?2
           AND event_type IN ('hook_invocation_started','hook_invocation_completed')
         ORDER BY sequence ASC LIMIT ?3",
        )
        .map_err(sql)?;
    let rows = stmt
        .query_map(params![run_id, after_sequence, capped], |row| {
            let payload: String = row.get(2)?;
            let mut value: Value = serde_json::from_str(&payload).unwrap_or(Value::Null);
            if let Some(obj) = value.as_object_mut() {
                obj.insert("run_id".into(), Value::String(row.get(0)?));
                obj.insert("sequence".into(), Value::from(row.get::<_, i64>(1)?));
                obj.insert("timestamp".into(), Value::String(row.get(3)?));
            }
            Ok(value)
        })
        .map_err(sql)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(sql)
}

pub fn register_project_identity(
    conn: &Connection,
    canonical_path: &str,
    name: &str,
) -> Result<Value, HarnessError> {
    if canonical_path.trim().is_empty() {
        return Err(HarnessError::invalid("canonical_path cannot be empty"));
    }
    let identity = crate::project_identity::store::register_or_get(conn, canonical_path)
        .map_err(HarnessError::invalid)?;
    Ok(serde_json::json!({
        "project_id": identity.project_id,
        "canonical_path": identity.canonical_path,
        "identity_version": identity.identity_version,
        "name": name,
    }))
}

pub fn list_project_identities(conn: &Connection) -> Result<Vec<Value>, HarnessError> {
    let mut stmt = conn
        .prepare(
            "SELECT project_id, canonical_path, identity_version, created_at
               FROM project_identity WHERE orphaned = 0 ORDER BY canonical_path ASC",
        )
        .map_err(sql)?;

    let rows = stmt
        .query_map([], |row| {
            let path = row.get::<_, String>(1)?;
            let name = PathBuf::from(&path)
                .file_name()
                .map(|value| value.to_string_lossy().to_string())
                .unwrap_or_else(|| path.clone());
            Ok(serde_json::json!({
                "project_id": row.get::<_, String>(0)?,
                "canonical_path": path,
                "name": name,
                "identity_version": row.get::<_, i64>(2)?,
                "created_at": row.get::<_, String>(3)?,
            }))
        })
        .map_err(sql)?;

    rows.collect::<Result<Vec<_>, _>>().map_err(sql)
}
