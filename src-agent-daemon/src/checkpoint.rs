//! Checkpoint capture + rewind (Phase 3).
//!
//! Logical checkpoints per run; lazy before-images on first write; after hash at
//! run end. Rewind refuses external modifications when conflict_policy=fail.
//!
//! Capture is bounded (T03): file content is read in bounded chunks with a
//! streaming SHA-256, content is capped per file and per run, file count is
//! capped, and sensitive paths (`.env`, credentials, keys, databases) are
//! recorded as redacted metadata + hash only — their content is never loaded
//! into memory or persisted. Sync file I/O runs on a blocking pool via the
//! `*_async` entry points used by async callers, and the live-map mutex is
//! never held during file I/O.

use crate::storage::DataStore;
use capability_gateway::TrustedPath;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use uuid::Uuid;

/// Content bytes one captured file may persist. Larger files are recorded as
/// redacted metadata + a streaming hash; their content is never held in memory
/// or written to SQLite.
pub const MAX_CAPTURE_FILE_CONTENT_BYTES: u64 = 256 * 1024;
/// Total content bytes one run's checkpoint may persist. Files beyond the
/// quota are redacted instead of growing SQLite without bound.
pub const MAX_CAPTURE_TOTAL_BYTES: u64 = 8 * 1024 * 1024;
/// Maximum number of files one run's checkpoint records.
pub const MAX_CAPTURE_FILE_COUNT: usize = 512;
/// Streaming-hash chunk size (memory stays bounded for arbitrarily large files).
const STREAM_CHUNK_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSnapshot {
    pub path: String,
    pub before_hash: Option<String>,
    pub after_hash: Option<String>,
    pub before_content: Option<String>,
    pub after_content: Option<String>,
    pub existed_before: bool,
    /// True when content was NOT captured (sensitive path, over size, or over
    /// the run's byte/file quota). Rewind can report but cannot restore it.
    #[serde(default)]
    pub redacted: bool,
    /// Machine-readable reason: `sensitive_path` | `too_large` | `over_byte_quota` |
    /// `too_many_files`.
    #[serde(default)]
    pub redaction_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointRecord {
    pub id: String,
    pub run_id: String,
    pub conversation_id: String,
    pub sequence: i64,
    pub label: Option<String>,
    pub files: Vec<FileSnapshot>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewindPreview {
    pub checkpoint_id: String,
    pub run_id: String,
    pub files: Vec<RewindFilePreview>,
    pub conflicts: Vec<RewindConflict>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewindFilePreview {
    pub path: String,
    pub checkpoint_after_hash: Option<String>,
    pub current_hash: Option<String>,
    pub change_type: String,
    /// Whether rewind can actually restore this file. False when content was
    /// not captured (sensitive path, or over a size/byte quota) — rewind
    /// reports the file as non-restorable instead of pretending.
    pub restorable: bool,
    /// Why the file is non-restorable, when it is.
    pub non_restorable_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewindConflict {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Default)]
/// Process-local checkpoint manager with optional SQLite persistence.
pub struct CheckpointManager {
    live: std::sync::Mutex<HashMap<String, LiveCheckpoint>>,
    store: Option<Arc<DataStore>>,
    /// Optional bounded storage actor (TASK-006 / B04); when present, durable
    /// checkpoint flushes execute on its single writer thread instead of
    /// locking the DataStore Mutex from the calling (async) thread.
    actor: Option<Arc<crate::storage::actor::StorageActor>>,
}

// W9: LiveCheckpoint / StreamedRead / stream_read_capped moved to
// `checkpoint_stream` (bounded streaming reads + in-memory per-run state).
mod checkpoint_stream;
pub(crate) use checkpoint_stream::stream_read_capped;
use checkpoint_stream::{LiveCheckpoint, StreamedRead};
// W9: rewind preview / restore / sensitivity moved to `checkpoint_rewind`.
mod checkpoint_rewind;
use checkpoint_rewind::{
    apply_rewind_files, build_rewind_preview, is_sensitive_checkpoint_path, parse_files_json,
};

impl CheckpointManager {
    pub fn new() -> Self {
        Self {
            live: std::sync::Mutex::new(HashMap::new()),
            store: None,
            actor: None,
        }
    }

    pub fn with_store(store: Arc<DataStore>) -> Self {
        Self {
            live: std::sync::Mutex::new(HashMap::new()),
            store: Some(store),
            actor: None,
        }
    }

    /// Checkpoint manager that routes durable flushes through a bounded
    /// storage actor (TASK-006 / B04).
    pub fn with_store_and_actor(
        store: Arc<DataStore>,
        actor: Arc<crate::storage::actor::StorageActor>,
    ) -> Self {
        Self {
            live: std::sync::Mutex::new(HashMap::new()),
            store: Some(store),
            actor: Some(actor),
        }
    }

    pub fn begin_run(
        &self,
        run_id: &str,
        conversation_id: &str,
        project_root: &Path,
    ) -> Result<String, String> {
        let id = Uuid::new_v4().to_string();
        let mut map = self.live.lock().map_err(|e| e.to_string())?;
        map.insert(
            run_id.to_string(),
            LiveCheckpoint {
                id: id.clone(),
                conversation_id: conversation_id.to_string(),
                run_id: run_id.to_string(),
                project_root: project_root.to_path_buf(),
                files: HashMap::new(),
                captured_bytes: 0,
            },
        );
        // Persist skeleton row
        if let Some(store) = &self.store {
            let conn = match store.conn() {
                Ok(conn) => conn,
                Err(error) => {
                    map.remove(run_id);
                    return Err(error);
                }
            };
            let snap = serde_json::json!({ "files": [] });
            if let Err(error) = conn.execute(
                "INSERT INTO checkpoint (id, run_id, conversation_id, sequence, label, snapshot_json)
                 VALUES (?1, ?2, ?3, 0, 'run_start', ?4)",
                params![id, run_id, conversation_id, snap.to_string()],
            ) {
                map.remove(run_id);
                return Err(error.to_string());
            }
        }
        Ok(id)
    }

    /// Attach the durable Core cursor to the workspace checkpoint.  Keeping
    /// this additive lets old callers continue to create file-only checkpoints
    /// while resume code can refuse to guess a turn or ledger position.
    pub fn set_run_metadata(
        &self,
        run_id: &str,
        turn_id: Option<&str>,
        active_context_snapshot_id: Option<&str>,
        side_effect_ledger_cursor: Option<&str>,
    ) -> Result<(), String> {
        let Some(store) = &self.store else {
            return Ok(());
        };
        let conn = store.conn()?;
        let changed = conn
            .execute(
                "UPDATE checkpoint
             SET turn_id = COALESCE(?1, turn_id),
                 active_context_snapshot_id = COALESCE(?2, active_context_snapshot_id),
                 side_effect_ledger_cursor = COALESCE(?3, side_effect_ledger_cursor)
             WHERE run_id = ?4",
                params![
                    turn_id,
                    active_context_snapshot_id,
                    side_effect_ledger_cursor,
                    run_id
                ],
            )
            .map_err(|e| e.to_string())?;
        if changed != 1 {
            return Err(format!("checkpoint metadata row missing for run {run_id}"));
        }
        Ok(())
    }

    /// Lazy capture before-image the first time a Gateway-authorized path is
    /// touched.
    ///
    /// Only accepts a [`TrustedPath`] produced by Gateway path preflight; a raw
    /// caller-supplied path cannot be represented. Defense in depth: an
    /// escaping `canonical` is rejected before any I/O, so checkpoint reads are
    /// impossible for unauthorized paths even if a future caller bypasses the
    /// Gateway.
    ///
    /// Blocking file I/O happens on the calling thread. Async callers MUST use
    /// [`Self::capture_before_async`] so the read runs on the blocking pool.
    pub fn capture_before(&self, run_id: &str, path: &TrustedPath) -> Result<(), String> {
        let plan = self.plan_before(run_id, path)?;
        let read = match &plan {
            BeforePlan::Capture {
                existed: true,
                canonical,
                ..
            } => Some(stream_read_capped(
                canonical,
                MAX_CAPTURE_FILE_CONTENT_BYTES,
            )),
            _ => None,
        };
        self.commit_before(run_id, plan, read)
    }

    /// Async variant of [`Self::capture_before`]: the file read runs on
    /// `tokio::task::spawn_blocking` and the live-map mutex is never held
    /// during I/O (R-B6 / R-P2).
    pub async fn capture_before_async(
        &self,
        run_id: &str,
        path: &TrustedPath,
    ) -> Result<(), String> {
        let plan = self.plan_before(run_id, path)?;
        let read = match &plan {
            BeforePlan::Capture {
                existed: true,
                canonical,
                ..
            } => {
                let canonical = canonical.clone();
                let cap = MAX_CAPTURE_FILE_CONTENT_BYTES;
                let task = tokio::task::spawn_blocking(move || stream_read_capped(&canonical, cap))
                    .await
                    .map_err(|e| format!("checkpoint read task failed: {e}"))?;
                Some(task)
            }
            _ => None,
        };
        self.commit_before(run_id, plan, read)
    }

    /// After a successful write, record after content/hash for a
    /// Gateway-authorized path (same containment rule as `capture_before`).
    ///
    /// Blocking file I/O happens on the calling thread. Async callers MUST use
    /// [`Self::capture_after_async`].
    pub fn capture_after(&self, run_id: &str, path: &TrustedPath) -> Result<(), String> {
        let plan = self.plan_after(run_id, path)?;
        let read = if plan.existed {
            Some(stream_read_capped(
                &plan.canonical,
                MAX_CAPTURE_FILE_CONTENT_BYTES,
            ))
        } else {
            None
        };
        self.commit_after(run_id, plan, read)?;
        // Durable flush when a store is configured. Without a store (unit tests /
        // pure in-memory) keep live-only success. With a store, fail closed so
        // side-effecting writes are not half-recorded.
        if self.store.is_some() {
            self.flush_live_to_store(run_id)?;
        }
        Ok(())
    }

    /// Async variant of [`Self::capture_after`]; the read runs on the blocking
    /// pool and the mutex is not held across I/O.
    pub async fn capture_after_async(
        &self,
        run_id: &str,
        path: &TrustedPath,
    ) -> Result<(), String> {
        let plan = self.plan_after(run_id, path)?;
        let read = if plan.existed {
            let canonical = plan.canonical.clone();
            let cap = MAX_CAPTURE_FILE_CONTENT_BYTES;
            let task = tokio::task::spawn_blocking(move || stream_read_capped(&canonical, cap))
                .await
                .map_err(|e| format!("checkpoint read task failed: {e}"))?;
            Some(task)
        } else {
            None
        };
        self.commit_after(run_id, plan, read)?;
        if self.store.is_some() {
            self.flush_live_to_store(run_id)?;
        }
        Ok(())
    }

    /// Before-capture decision, computed under a brief lock. The file read
    /// itself happens outside the lock (see the callers).
    fn plan_before(&self, run_id: &str, path: &TrustedPath) -> Result<BeforePlan, String> {
        let mut map = self.live.lock().map_err(|e| e.to_string())?;
        let live = map
            .get_mut(run_id)
            .ok_or_else(|| format!("no checkpoint for run {run_id}"))?;
        let key = path.project_relative.to_string_lossy().into_owned();
        if live.files.contains_key(&key) {
            return Ok(BeforePlan::AlreadyCaptured);
        }
        if !path.canonical.starts_with(&live.project_root) {
            return Err(format!(
                "checkpoint path {} escapes project root {}",
                path.canonical.display(),
                live.project_root.display()
            ));
        }
        let (redacted, redaction_reason) = if live.files.len() >= MAX_CAPTURE_FILE_COUNT {
            (true, Some("too_many_files".to_string()))
        } else if is_sensitive_checkpoint_path(&path.canonical) {
            (true, Some("sensitive_path".to_string()))
        } else {
            (false, None)
        };
        let existed = path.canonical.exists();
        Ok(BeforePlan::Capture {
            key,
            canonical: path.canonical.clone(),
            existed,
            redacted,
            redaction_reason,
        })
    }

    /// Insert the before-image after the (optional) read. Quota decisions that
    /// depend on the file size are made here, once the size is known.
    fn commit_before(
        &self,
        run_id: &str,
        plan: BeforePlan,
        read: Option<Result<StreamedRead, String>>,
    ) -> Result<(), String> {
        let BeforePlan::Capture {
            key,
            existed,
            mut redacted,
            mut redaction_reason,
            ..
        } = plan
        else {
            return Ok(());
        };
        let read = if existed {
            Some(read.unwrap_or_else(|| Err("checkpoint read result missing".into()))?)
        } else {
            None
        };
        let mut map = self.live.lock().map_err(|e| e.to_string())?;
        let live = map
            .get_mut(run_id)
            .ok_or_else(|| format!("no checkpoint for run {run_id}"))?;
        if let Some(r) = &read {
            // Per-file cap: the streaming read dropped content beyond the cap.
            if r.over_cap {
                redacted = true;
                redaction_reason = Some("too_large".to_string());
            }
            if live.captured_bytes.saturating_add(r.size) > MAX_CAPTURE_TOTAL_BYTES {
                redacted = true;
                redaction_reason = Some("over_byte_quota".to_string());
            }
        }
        let (before_hash, before_content, captured) = match &read {
            Some(r) if !redacted => (Some(r.hash.clone()), r.content.clone(), r.size),
            // Redacted files persist no content — only the small hash.
            Some(r) => (Some(r.hash.clone()), None, 0),
            None => (None, None, 0),
        };
        live.captured_bytes += captured;
        live.files.insert(
            key.clone(),
            FileSnapshot {
                path: key,
                before_hash,
                after_hash: None,
                before_content,
                after_content: None,
                existed_before: existed,
                redacted,
                redaction_reason,
            },
        );
        Ok(())
    }

    /// After-capture decision, computed under a brief lock (see the callers).
    fn plan_after(&self, run_id: &str, path: &TrustedPath) -> Result<AfterPlan, String> {
        let mut map = self.live.lock().map_err(|e| e.to_string())?;
        let live = map
            .get_mut(run_id)
            .ok_or_else(|| format!("no checkpoint for run {run_id}"))?;
        if !path.canonical.starts_with(&live.project_root) {
            return Err(format!(
                "checkpoint path {} escapes project root {}",
                path.canonical.display(),
                live.project_root.display()
            ));
        }
        let key = path.project_relative.to_string_lossy().into_owned();
        // Redaction is sticky for after-images: a file that was already
        // captured as redacted (sensitive or over quota) stays redacted.
        let redacted = live
            .files
            .get(&key)
            .map(|f| f.redacted)
            .unwrap_or_else(|| is_sensitive_checkpoint_path(&path.canonical));
        let redaction_reason = if redacted {
            Some(
                live.files
                    .get(&key)
                    .and_then(|f| f.redaction_reason.clone())
                    .unwrap_or_else(|| "sensitive_path".to_string()),
            )
        } else {
            None
        };
        let existed = path.canonical.exists();
        Ok(AfterPlan {
            key,
            canonical: path.canonical.clone(),
            existed,
            redacted,
            redaction_reason,
        })
    }

    /// Insert/update the after-image after the (optional) read.
    fn commit_after(
        &self,
        run_id: &str,
        plan: AfterPlan,
        read: Option<Result<StreamedRead, String>>,
    ) -> Result<(), String> {
        let AfterPlan {
            key,
            existed,
            mut redacted,
            mut redaction_reason,
            ..
        } = plan;
        let read = if existed {
            Some(read.unwrap_or_else(|| Err("checkpoint read result missing".into()))?)
        } else {
            None
        };
        let mut map = self.live.lock().map_err(|e| e.to_string())?;
        let live = map
            .get_mut(run_id)
            .ok_or_else(|| format!("no checkpoint for run {run_id}"))?;
        if let Some(r) = &read {
            // Per-file cap: the streaming read dropped content beyond the cap.
            if r.over_cap {
                redacted = true;
                redaction_reason = Some("too_large".to_string());
            }
            if live.captured_bytes.saturating_add(r.size) > MAX_CAPTURE_TOTAL_BYTES {
                redacted = true;
                redaction_reason = Some("over_byte_quota".to_string());
            }
        }
        let (after_hash, after_content, captured) = match &read {
            Some(r) if !redacted => (Some(r.hash.clone()), r.content.clone(), r.size),
            // Redacted files persist no content — only the small hash.
            Some(r) => (Some(r.hash.clone()), None, 0),
            None => (None, None, 0),
        };
        let entry = live
            .files
            .entry(key.clone())
            .or_insert_with(|| FileSnapshot {
                path: key,
                before_hash: None,
                after_hash: None,
                before_content: None,
                after_content: None,
                existed_before: false,
                redacted,
                redaction_reason: redaction_reason.clone(),
            });
        // A previously-captured before-image is not re-accounted.
        if entry.before_hash.is_none() {
            live.captured_bytes += captured;
        }
        entry.after_hash = after_hash;
        entry.after_content = after_content;
        entry.redacted = redacted;
        entry.redaction_reason = redaction_reason;
        Ok(())
    }

    pub fn finalize_run(&self, run_id: &str) -> Result<CheckpointRecord, String> {
        let mut map = self.live.lock().map_err(|e| e.to_string())?;
        let live = map
            .remove(run_id)
            .ok_or_else(|| format!("no checkpoint for run {run_id}"))?;
        let files: Vec<FileSnapshot> = live.files.into_values().collect();
        let record = CheckpointRecord {
            id: live.id.clone(),
            run_id: live.run_id.clone(),
            conversation_id: live.conversation_id.clone(),
            sequence: 0,
            label: Some("run_complete".into()),
            files: files.clone(),
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        if let Some(store) = &self.store {
            let conn = store.conn()?;
            // Persist full file snapshots so rewind works after process restart.
            let snap = serde_json::to_string(&serde_json::json!({ "files": files }))
                .map_err(|e| format!("serialize checkpoint snapshot: {e}"))?;
            let updated = conn
                .execute(
                    "UPDATE checkpoint SET snapshot_json = ?1, label = 'run_complete' WHERE id = ?2",
                    params![snap, live.id],
                )
                .map_err(|e| e.to_string())?;
            if updated == 0 {
                conn.execute(
                    "INSERT INTO checkpoint (id, run_id, conversation_id, sequence, label, snapshot_json)
                     VALUES (?1, ?2, ?3, 0, 'run_complete', ?4)",
                    params![live.id, live.run_id, live.conversation_id, snap],
                )
                .map_err(|e| e.to_string())?;
            }
            // Retention: keep last 50 per conversation
            conn.execute(
                "DELETE FROM checkpoint WHERE conversation_id = ?1 AND id NOT IN (
                    SELECT id FROM checkpoint WHERE conversation_id = ?1
                    ORDER BY created_at DESC LIMIT 50
                 )",
                params![live.conversation_id],
            )
            .map_err(|e| e.to_string())?;
        }
        Ok(record)
    }

    /// Flush live before/after images to SQLite without removing the live map.
    pub fn flush_live_to_store(&self, run_id: &str) -> Result<(), String> {
        let (checkpoint_id, snap) = {
            let map = self.live.lock().map_err(|e| e.to_string())?;
            let live = map
                .get(run_id)
                .ok_or_else(|| format!("no checkpoint for run {run_id}"))?;
            let files: Vec<FileSnapshot> = live.files.values().cloned().collect();
            let snap = serde_json::to_string(&serde_json::json!({ "files": files }))
                .map_err(|e| format!("serialize checkpoint snapshot: {e}"))?;
            (live.id.clone(), snap)
        };
        if let Some(actor) = &self.actor {
            // TASK-006: the durable flush runs on the storage actor's single
            // writer thread; the calling (async) thread never locks the Mutex.
            let run_id = run_id.to_string();
            actor
                .submit(true, move |conn| {
                    let changed = conn
                        .execute(
                            "UPDATE checkpoint SET snapshot_json = ?1 WHERE id = ?2",
                            params![snap, checkpoint_id],
                        )
                        .map_err(|e| e.to_string())?;
                    if changed != 1 {
                        return Err(format!("checkpoint row missing for run {run_id}"));
                    }
                    Ok(serde_json::json!(null))
                })
                .map(|_| ())
        } else {
            let store = self
                .store
                .as_ref()
                .ok_or_else(|| "checkpoint store unavailable".to_string())?;
            let conn = store.conn()?;
            let changed = conn
                .execute(
                    "UPDATE checkpoint SET snapshot_json = ?1 WHERE id = ?2",
                    params![snap, checkpoint_id],
                )
                .map_err(|e| e.to_string())?;
            if changed != 1 {
                return Err(format!("checkpoint row missing for run {run_id}"));
            }
            Ok(())
        }
    }

    pub fn load_checkpoint(&self, checkpoint_id: &str) -> Result<CheckpointRecord, String> {
        // live first
        {
            let map = self.live.lock().map_err(|e| e.to_string())?;
            for live in map.values() {
                if live.id == checkpoint_id {
                    return Ok(CheckpointRecord {
                        id: live.id.clone(),
                        run_id: live.run_id.clone(),
                        conversation_id: live.conversation_id.clone(),
                        sequence: 0,
                        label: None,
                        files: live.files.values().cloned().collect(),
                        created_at: chrono::Utc::now().to_rfc3339(),
                    });
                }
            }
        }
        let store = self
            .store
            .as_ref()
            .ok_or_else(|| "checkpoint store unavailable".to_string())?;
        let conn = store.conn()?;
        let (id, run_id, conversation_id, sequence, label, snap, created_at): (
            String,
            String,
            String,
            i64,
            Option<String>,
            String,
            String,
        ) = conn
            .query_row(
                "SELECT id, run_id, conversation_id, sequence, label, snapshot_json, created_at
                 FROM checkpoint WHERE id = ?1",
                params![checkpoint_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                    ))
                },
            )
            .map_err(|e| format!("checkpoint not found: {e}"))?;
        let files = parse_files_json(&snap)?;
        Ok(CheckpointRecord {
            id,
            run_id,
            conversation_id,
            sequence,
            label,
            files,
            created_at,
        })
    }

    pub fn rewind_preview(
        &self,
        run_id: &str,
        project_root: &Path,
        paths: Option<&[String]>,
    ) -> Result<RewindPreview, String> {
        let cp = self.checkpoint_for_run(run_id)?;
        build_rewind_preview(&cp, project_root, paths)
    }

    /// Async variant of [`Self::rewind_preview`]: current-file hashing runs on
    /// the blocking pool.
    pub async fn rewind_preview_async(
        &self,
        run_id: &str,
        project_root: &Path,
        paths: Option<&[String]>,
    ) -> Result<RewindPreview, String> {
        let cp = self.checkpoint_for_run(run_id)?;
        let project_root = project_root.to_path_buf();
        let paths = paths.map(|p| p.to_vec());
        tokio::task::spawn_blocking(move || {
            build_rewind_preview(&cp, &project_root, paths.as_deref())
        })
        .await
        .map_err(|e| format!("checkpoint preview task failed: {e}"))?
    }

    /// P0 fail-closed validation (task-07): checkpoint_id must exist and match
    /// the request run, and the project root must match the live checkpoint
    /// root when known. Any mismatch restores 0 files.
    fn validate_rewind(
        &self,
        run_id: &str,
        checkpoint_id: &str,
        project_root: &Path,
    ) -> Result<(), String> {
        if checkpoint_id.trim().is_empty() {
            return Err("checkpoint_id required".into());
        }
        let cp = self.load_checkpoint(checkpoint_id).map_err(|e| {
            format!("workspace.restore refused: checkpoint not found ({e}); restored=0")
        })?;
        if cp.run_id != run_id {
            return Err(format!(
                "workspace.restore refused: checkpoint.run_id={} != request run_id={}; restored=0",
                cp.run_id, run_id
            ));
        }
        // When live checkpoint exists, enforce project root match (canonical).
        {
            let map = self.live.lock().map_err(|e| e.to_string())?;
            if let Some(live) = map.get(run_id) {
                let expected = live
                    .project_root
                    .canonicalize()
                    .unwrap_or_else(|_| live.project_root.clone());
                let got = project_root
                    .canonicalize()
                    .unwrap_or_else(|_| project_root.to_path_buf());
                if expected != got {
                    return Err(format!(
                        "workspace.restore refused: project_identity mismatch \
                         (checkpoint root {:?}, request {:?}); restored=0",
                        expected, got
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn rewind(
        &self,
        run_id: &str,
        checkpoint_id: &str,
        project_root: &Path,
        paths: Option<&[String]>,
        conflict_policy: &str,
    ) -> Result<Vec<String>, String> {
        if conflict_policy != "fail" {
            return Err("only conflict_policy=fail is supported".into());
        }
        self.validate_rewind(run_id, checkpoint_id, project_root)?;

        let preview = self.rewind_preview(run_id, project_root, paths)?;
        if preview.checkpoint_id != checkpoint_id {
            return Err(format!(
                "workspace.restore refused: checkpoint_id mismatch \
                 (preview={}, request={}); restored=0",
                preview.checkpoint_id, checkpoint_id
            ));
        }
        if !preview.conflicts.is_empty() {
            return Err(format!(
                "workspace.restore refused: {} conflict(s); first={}; restored=0",
                preview.conflicts.len(),
                preview.conflicts[0].path
            ));
        }

        let cp = self.load_checkpoint(checkpoint_id).map_err(|e| {
            format!("workspace.restore refused: checkpoint not found ({e}); restored=0")
        })?;
        apply_rewind_files(&cp.files, project_root, paths)
    }

    /// Async variant of [`Self::rewind`] (workspace restore): file staging and
    /// application run on the blocking pool.
    pub async fn workspace_restore_async(
        &self,
        run_id: &str,
        checkpoint_id: &str,
        project_root: &Path,
        paths: Option<&[String]>,
        conflict_policy: &str,
    ) -> Result<Vec<String>, String> {
        if conflict_policy != "fail" {
            return Err("only conflict_policy=fail is supported".into());
        }
        self.validate_rewind(run_id, checkpoint_id, project_root)?;

        let preview = self
            .rewind_preview_async(run_id, project_root, paths)
            .await?;
        if preview.checkpoint_id != checkpoint_id {
            return Err(format!(
                "workspace.restore refused: checkpoint_id mismatch \
                 (preview={}, request={}); restored=0",
                preview.checkpoint_id, checkpoint_id
            ));
        }
        if !preview.conflicts.is_empty() {
            return Err(format!(
                "workspace.restore refused: {} conflict(s); first={}; restored=0",
                preview.conflicts.len(),
                preview.conflicts[0].path
            ));
        }

        let cp = self.load_checkpoint(checkpoint_id).map_err(|e| {
            format!("workspace.restore refused: checkpoint not found ({e}); restored=0")
        })?;
        let project_root = project_root.to_path_buf();
        let paths = paths.map(|p| p.to_vec());
        tokio::task::spawn_blocking(move || {
            apply_rewind_files(&cp.files, &project_root, paths.as_deref())
        })
        .await
        .map_err(|e| format!("workspace.restore task failed: {e}"))?
    }

    /// Alias with task-07 naming — workspace restore only (not conversation rewind).
    pub fn workspace_restore(
        &self,
        run_id: &str,
        checkpoint_id: &str,
        project_root: &Path,
        paths: Option<&[String]>,
        conflict_policy: &str,
    ) -> Result<Vec<String>, String> {
        self.rewind(run_id, checkpoint_id, project_root, paths, conflict_policy)
    }

    fn checkpoint_for_run(&self, run_id: &str) -> Result<CheckpointRecord, String> {
        {
            let map = self.live.lock().map_err(|e| e.to_string())?;
            if let Some(live) = map.get(run_id) {
                return Ok(CheckpointRecord {
                    id: live.id.clone(),
                    run_id: live.run_id.clone(),
                    conversation_id: live.conversation_id.clone(),
                    sequence: 0,
                    label: None,
                    files: live.files.values().cloned().collect(),
                    created_at: chrono::Utc::now().to_rfc3339(),
                });
            }
        }
        let store = self
            .store
            .as_ref()
            .ok_or_else(|| "no checkpoint for run".to_string())?;
        // Resolve id then drop the MutexGuard before load_checkpoint (which also
        // needs store.conn()) — nested conn() on std::sync::Mutex deadlocks.
        let id: String = {
            let conn = store.conn()?;
            conn.query_row(
                "SELECT id FROM checkpoint WHERE run_id = ?1 ORDER BY created_at DESC LIMIT 1",
                params![run_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("no checkpoint for run {run_id}"))?
        };
        self.load_checkpoint(&id)
    }

    /// Public accessor for live/persisted checkpoint of a run (tool capture path).
    pub fn checkpoint_for_run_public(&self, run_id: &str) -> Result<CheckpointRecord, String> {
        self.checkpoint_for_run(run_id)
    }
}

impl Default for CheckpointManager {
    fn default() -> Self {
        Self::new()
    }
}

pub fn global_checkpoint_manager() -> &'static CheckpointManager {
    #[cfg(test)]
    {
        *test_checkpoint_global_lock()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }
    #[cfg(not(test))]
    {
        use std::sync::OnceLock;
        static M: OnceLock<CheckpointManager> = OnceLock::new();
        M.get_or_init(|| {
            let db = crate::default_assistant_db_path();
            let art = std::env::var("NATIVES_RUNTIME_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|_| {
                    std::env::var_os("HOME")
                        .map(|h| PathBuf::from(h).join(".natives").join("runtime"))
                        .unwrap_or_else(std::env::temp_dir)
                })
                .join("artifacts");
            match DataStore::new(&db, &art) {
                Ok(store) => CheckpointManager::with_store(Arc::new(store)),
                Err(_) => CheckpointManager::new(),
            }
        })
    }
}

/// Test-only process-global checkpoint manager. Defaults to a memory-only
/// manager (never `~/.natives`); a test can install a store-backed one via
/// [`install_checkpoint_global_for_test`].
#[cfg(test)]
fn test_checkpoint_global_lock() -> &'static std::sync::Mutex<&'static CheckpointManager> {
    use std::sync::{Mutex, OnceLock};
    static M: OnceLock<Mutex<&'static CheckpointManager>> = OnceLock::new();
    M.get_or_init(|| Mutex::new(Box::leak(Box::new(CheckpointManager::new()))))
}

/// Test-only: replace the process-global checkpoint manager. Call while
/// holding [`DataStore::env_test_lock`] for determinism under
/// `--test-threads=2`.
#[cfg(test)]
pub fn install_checkpoint_global_for_test(mgr: CheckpointManager) -> &'static CheckpointManager {
    let m: &'static CheckpointManager = Box::leak(Box::new(mgr));
    let lock = test_checkpoint_global_lock();
    *lock.lock().unwrap_or_else(|e| e.into_inner()) = m;
    m
}

/// Before-capture plan computed under a brief lock.
enum BeforePlan {
    /// The path was already captured for this run.
    AlreadyCaptured,
    Capture {
        key: String,
        canonical: PathBuf,
        existed: bool,
        redacted: bool,
        redaction_reason: Option<String>,
    },
}

/// After-capture plan computed under a brief lock.
struct AfterPlan {
    key: String,
    canonical: PathBuf,
    existed: bool,
    redacted: bool,
    redaction_reason: Option<String>,
}

/// Rough context usage: chars/4 heuristic.
pub fn estimate_context_usage(
    system_chars: usize,
    conversation_chars: usize,
    tool_chars: usize,
    max_tokens: u64,
) -> Value {
    let used = ((system_chars + conversation_chars + tool_chars) / 4) as u64;
    serde_json::json!({
        "used_tokens": used,
        "max_tokens": max_tokens,
        "breakdown": {
            "system": system_chars / 4,
            "conversation": conversation_chars / 4,
            "tool_output": tool_chars / 4,
        }
    })
}

#[cfg(test)]
#[path = "checkpoint_tests.rs"]
mod tests;
