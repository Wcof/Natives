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

mod repository_profile;
mod repository_source;
mod repository_version;

pub use repository_profile::{
    ProfileRow, archive_profile, get_profile, insert_profile, list_profiles,
};
pub use repository_source::{
    acknowledge_source_drift, ensure_source_drift_candidate, publish_source_drift_manifest,
};
pub use repository_version::{
    DraftRow, VersionRow, clear_draft, current_version, get_draft, get_or_create_draft,
    get_version, list_versions, publish_draft_version, publish_version, save_draft,
};

/// Identity of the seeded global template. Fixed so re-seeding is a no-op.
pub const DEFAULT_GLOBAL_PROFILE_ID: &str = "harness.global.default";
/// The single global binding row's scope id.
pub const GLOBAL_SCOPE_ID: &str = "global";

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
    // W2: single Daemon DataStore open path (assistant.db authority + test hook).
    crate::storage::open_daemon_store()
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
