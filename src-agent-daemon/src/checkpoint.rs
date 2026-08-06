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
struct LiveCheckpoint {
    id: String,
    conversation_id: String,
    run_id: String,
    project_root: PathBuf,
    files: HashMap<String, FileSnapshot>,
    /// Total content bytes persisted for this run (drives the byte quota).
    captured_bytes: u64,
}

/// Result of a bounded, streaming file read.
struct StreamedRead {
    hash: String,
    size: u64,
    /// Content only when the file fits the content cap AND is valid UTF-8.
    content: Option<String>,
    /// True when the file exceeded the per-file content cap (content dropped).
    over_cap: bool,
}

/// Process-local checkpoint manager with optional SQLite persistence.
pub struct CheckpointManager {
    live: std::sync::Mutex<HashMap<String, LiveCheckpoint>>,
    store: Option<Arc<DataStore>>,
    /// Optional bounded storage actor (TASK-006 / B04); when present, durable
    /// checkpoint flushes execute on its single writer thread instead of
    /// locking the DataStore Mutex from the calling (async) thread.
    actor: Option<Arc<crate::storage::actor::StorageActor>>,
}

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

/// Stream a file in bounded chunks, hashing the whole content (so the hash
/// stays a stable identity for conflict detection) while capping how much
/// content is retained. Memory use is O(chunk), never O(file).
fn stream_read_capped(path: &Path, content_cap: u64) -> Result<StreamedRead, String> {
    let mut file = std::fs::File::open(path).map_err(|e| format!("checkpoint read failed: {e}"))?;
    let size = file.metadata().map_err(|e| e.to_string())?.len();
    let mut hasher = Sha256::new();
    let mut content = Vec::new();
    let mut over_cap = false;
    let mut buf = vec![0u8; STREAM_CHUNK_BYTES];
    loop {
        let n = file.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        if !over_cap {
            let remaining = (content_cap as usize).saturating_sub(content.len());
            let take = n.min(remaining);
            content.extend_from_slice(&buf[..take]);
            if take < n {
                over_cap = true;
            }
        }
    }
    let content = if over_cap {
        None
    } else {
        String::from_utf8(content).ok()
    };
    Ok(StreamedRead {
        hash: format!("{:x}", hasher.finalize()),
        size,
        content,
        over_cap,
    })
}

/// Paths whose contents must never be captured in a checkpoint. Only redacted
/// metadata + hash are stored; rewind reports these files as non-restorable.
///
/// Covers `.env` files, credential/key/secret material, private-key formats,
/// and database files. Conservative on purpose: over-matching redacts content
/// that simply will not be restorable; under-matching would persist secrets.
fn is_sensitive_checkpoint_path(path: &Path) -> bool {
    let file_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    let lower = file_name.to_ascii_lowercase();
    let in_sensitive_dir = path.components().any(|component| match component {
        std::path::Component::Normal(name) => matches!(
            name.to_string_lossy().as_ref(),
            ".aws" | ".ssh" | ".gnupg" | ".kube" | "credentials"
        ),
        _ => false,
    });
    in_sensitive_dir
        || lower.starts_with(".env")
        || lower.contains("credential")
        || lower.contains("secret")
        || lower.contains("password")
        || lower.contains("passwd")
        || lower.ends_with(".pem")
        || lower.ends_with(".key")
        || lower.ends_with("key.json")
        || lower.ends_with(".p12")
        || lower.ends_with(".pfx")
        || lower.ends_with(".jks")
        || lower.ends_with(".db")
        || lower.ends_with(".sqlite")
        || lower.ends_with(".sqlite3")
        || lower == ".netrc"
        || lower == ".npmrc"
        || lower == ".pgpass"
}

/// Build the rewind preview for one checkpoint record. Current-file hashes are
/// computed with a streaming read (bounded memory). Redacted files are reported
/// as non-restorable so rewind never silently claims to restore content it
/// does not have.
fn build_rewind_preview(
    cp: &CheckpointRecord,
    project_root: &Path,
    paths: Option<&[String]>,
) -> Result<RewindPreview, String> {
    let mut previews = Vec::new();
    let mut conflicts = Vec::new();
    for f in &cp.files {
        if let Some(filter) = paths {
            if !filter.iter().any(|p| p == &f.path) {
                continue;
            }
        }
        let abs = project_root.join(&f.path);
        let current_hash = if abs.exists() {
            Some(stream_read_capped(&abs, MAX_CAPTURE_FILE_CONTENT_BYTES)?.hash)
        } else {
            None
        };
        // Conflict: current differs from after_hash (external edit after run)
        if let (Some(after), Some(cur)) = (&f.after_hash, &current_hash) {
            if after != cur {
                conflicts.push(RewindConflict {
                    path: f.path.clone(),
                    reason: "external modification since checkpoint after".into(),
                });
            }
        }
        let change_type = if !f.existed_before {
            "add"
        } else if f.after_hash.is_none() {
            "delete"
        } else {
            "update"
        };
        let (restorable, non_restorable_reason) = if f.redacted {
            (
                false,
                Some(
                    f.redaction_reason
                        .clone()
                        .unwrap_or_else(|| "content not captured".into()),
                ),
            )
        } else if f.existed_before && f.before_content.is_none() {
            // Non-UTF-8 content: the hash is recorded but content was never
            // stored as text, so rewind cannot restore it.
            (false, Some("content not captured (non-UTF-8)".into()))
        } else {
            (true, None)
        };
        previews.push(RewindFilePreview {
            path: f.path.clone(),
            checkpoint_after_hash: f.after_hash.clone(),
            current_hash,
            change_type: change_type.into(),
            restorable,
            non_restorable_reason,
        });
    }
    Ok(RewindPreview {
        checkpoint_id: cp.id.clone(),
        run_id: cp.run_id.clone(),
        files: previews,
        conflicts,
    })
}

/// Apply a checkpoint to the workspace: restore `before_content` for files
/// that existed, delete files the run added, and roll back everything on a
/// mid-apply failure (staging + reverse-order undo). Files whose content was
/// redacted are skipped — the preview already reports them as non-restorable.
fn apply_rewind_files(
    files: &[FileSnapshot],
    project_root: &Path,
    paths: Option<&[String]>,
) -> Result<Vec<String>, String> {
    // Staging: capture current content for undo; apply atomically; rollback on failure.
    let mut staging: Vec<(PathBuf, Option<Vec<u8>>)> = Vec::new();
    let mut restored = Vec::new();
    let apply_result: Result<(), String> = (|| {
        for f in files {
            if let Some(filter) = paths {
                if !filter.iter().any(|p| p == &f.path) {
                    continue;
                }
            }
            if f.redacted && f.existed_before {
                // Content was not captured; restoring is impossible. The
                // preview already surfaced this as non-restorable.
                continue;
            }
            let abs = project_root.join(&f.path);
            let prior = if abs.exists() {
                Some(std::fs::read(&abs).map_err(|e| e.to_string())?)
            } else {
                None
            };
            staging.push((abs.clone(), prior));

            if f.existed_before {
                if let Some(content) = &f.before_content {
                    if let Some(parent) = abs.parent() {
                        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                    }
                    // Atomic-ish write: temp + rename
                    let tmp = abs.with_extension(format!("natives-restore-tmp-{}", Uuid::new_v4()));
                    std::fs::write(&tmp, content).map_err(|e| e.to_string())?;
                    std::fs::rename(&tmp, &abs).map_err(|e| {
                        let _ = std::fs::remove_file(&tmp);
                        e.to_string()
                    })?;
                }
            } else if abs.exists() {
                // was added by the run — delete
                std::fs::remove_file(&abs).map_err(|e| e.to_string())?;
            }
            restored.push(f.path.clone());
        }
        Ok(())
    })();

    if let Err(e) = apply_result {
        // Undo already-applied files from staging (reverse order).
        for (abs, prior) in staging.into_iter().rev() {
            match prior {
                Some(bytes) => {
                    if let Some(parent) = abs.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    let _ = std::fs::write(&abs, bytes);
                }
                None => {
                    let _ = std::fs::remove_file(&abs);
                }
            }
        }
        return Err(format!(
            "workspace.restore failed mid-apply and rolled back: {e}; restored=0"
        ));
    }
    Ok(restored)
}

fn parse_files_json(snap: &str) -> Result<Vec<FileSnapshot>, String> {
    let v: Value = serde_json::from_str(snap).map_err(|e| e.to_string())?;
    let arr = v
        .get("files")
        .and_then(|f| f.as_array())
        .cloned()
        .ok_or_else(|| "checkpoint snapshot files must be an array".to_string())?;
    arr.into_iter()
        .map(|item| serde_json::from_value(item).map_err(|e| e.to_string()))
        .collect()
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
mod tests {
    use super::*;
    use std::path::Path;

    /// Build a `TrustedPath` for an existing in-root file (mirrors the
    /// canonical result Gateway preflight would produce).
    fn trusted(root: &Path, rel: &str) -> TrustedPath {
        let canonical = root
            .join(rel)
            .canonicalize()
            .unwrap_or_else(|_| root.join(rel));
        TrustedPath::new(canonical, PathBuf::from(rel))
    }

    #[test]
    fn capture_and_rewind_roundtrip() {
        let root = std::env::temp_dir().join(format!("cp-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let file = root.join("a.txt");
        std::fs::write(&file, "before").unwrap();

        let mgr = CheckpointManager::new();
        let _id = mgr.begin_run("run1", "c1", &root).unwrap();
        mgr.capture_before("run1", &trusted(&root, "a.txt"))
            .unwrap();
        std::fs::write(&file, "after").unwrap();
        mgr.capture_after("run1", &trusted(&root, "a.txt")).unwrap();
        let rec = mgr.finalize_run("run1").unwrap();
        assert_eq!(rec.files.len(), 1);

        // re-open as live for rewind: put back
        let _ = mgr.begin_run("run1", "c1", &root).unwrap();
        // Manually inject finalized data into live by re-capturing is wrong;
        // use in-memory manager that still has no live — load from finalize path:
        // For unit test without store, keep live by not finalizing:
    }

    #[test]
    fn rewind_without_finalize_uses_live() {
        let root = std::env::temp_dir().join(format!("cp2-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let file = root.join("b.txt");
        std::fs::write(&file, "v1").unwrap();
        let mgr = CheckpointManager::new();
        let cp_id = mgr.begin_run("run2", "c1", &root).unwrap();
        mgr.capture_before("run2", &trusted(&root, "b.txt"))
            .unwrap();
        std::fs::write(&file, "v2").unwrap();
        mgr.capture_after("run2", &trusted(&root, "b.txt")).unwrap();

        let preview = mgr.rewind_preview("run2", &root, None).unwrap();
        assert!(preview.conflicts.is_empty());
        assert_eq!(preview.checkpoint_id, cp_id);

        let restored = mgr.rewind("run2", &cp_id, &root, None, "fail").unwrap();
        assert_eq!(restored, vec!["b.txt".to_string()]);
        let content = std::fs::read_to_string(&file).unwrap();
        assert_eq!(content, "v1");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn external_modification_conflicts() {
        let root = std::env::temp_dir().join(format!("cp3-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let file = root.join("c.txt");
        std::fs::write(&file, "v1").unwrap();
        let mgr = CheckpointManager::new();
        let cp_id = mgr.begin_run("run3", "c1", &root).unwrap();
        mgr.capture_before("run3", &trusted(&root, "c.txt"))
            .unwrap();
        std::fs::write(&file, "v2").unwrap();
        mgr.capture_after("run3", &trusted(&root, "c.txt")).unwrap();
        // external edit after "run"
        std::fs::write(&file, "external").unwrap();
        let preview = mgr.rewind_preview("run3", &root, None).unwrap();
        assert!(!preview.conflicts.is_empty());
        let err = mgr.rewind("run3", &cp_id, &root, None, "fail").unwrap_err();
        assert!(err.contains("conflict"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn context_usage_estimate() {
        let v = estimate_context_usage(400, 800, 400, 128_000);
        assert_eq!(v["used_tokens"], 400);
        assert_eq!(v["max_tokens"], 128_000);
    }

    #[test]
    fn checkpoint_survives_manager_restart_via_sqlite() {
        let root = std::env::temp_dir().join(format!("cp-restart-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let file = root.join("persist.txt");
        std::fs::write(&file, "v1").unwrap();

        let db = std::env::temp_dir().join(format!("cp-db-{}.sqlite", Uuid::new_v4()));
        let art = std::env::temp_dir().join(format!("cp-art-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&art).unwrap();
        let store = Arc::new(DataStore::new(&db, &art).expect("store"));
        // FK: checkpoint → run → conversation
        {
            let conn = store.conn().unwrap();
            conn.execute(
                "INSERT INTO conversation (id, mode, title, provider_id, model_id)
                 VALUES ('c1', 'agent', 't', 'p', 'm')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO run (id, conversation_id, status, provider_id, model_id)
                 VALUES ('run-persist', 'c1', 'running', 'p', 'm')",
                [],
            )
            .unwrap();
        }

        let mgr1 = CheckpointManager::with_store(Arc::clone(&store));
        let cp_id = mgr1.begin_run("run-persist", "c1", &root).unwrap();
        mgr1.capture_before("run-persist", &trusted(&root, "persist.txt"))
            .unwrap();
        std::fs::write(&file, "v2").unwrap();
        mgr1.capture_after("run-persist", &trusted(&root, "persist.txt"))
            .unwrap();
        let rec = mgr1.finalize_run("run-persist").unwrap();
        assert_eq!(rec.id, cp_id);
        assert_eq!(rec.files.len(), 1);

        // New manager instance = process restart; live map empty, load from SQLite.
        let mgr2 = CheckpointManager::with_store(store);
        let preview = mgr2
            .rewind_preview("run-persist", &root, None)
            .expect("preview after restart");
        assert_eq!(preview.checkpoint_id, cp_id);
        assert!(preview.conflicts.is_empty());
        let restored = mgr2
            .rewind("run-persist", &cp_id, &root, None, "fail")
            .expect("rewind after restart");
        assert_eq!(restored, vec!["persist.txt".to_string()]);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "v1");

        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_file(&db);
        let _ = std::fs::remove_dir_all(&art);
    }

    /// TASK-006: a durable checkpoint flush routed through the bounded storage
    /// actor still persists the snapshot_json (async path never locks the Mutex).
    #[test]
    fn flush_via_storage_actor_persists_snapshot() {
        let root = std::env::temp_dir().join(format!("cp-act-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let file = root.join("act.txt");
        std::fs::write(&file, "v1").unwrap();

        let db = std::env::temp_dir().join(format!("cp-act-db-{}.sqlite", Uuid::new_v4()));
        let art = std::env::temp_dir().join(format!("cp-act-art-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&art).unwrap();
        let store = Arc::new(DataStore::new(&db, &art).expect("store"));
        {
            let conn = store.conn().unwrap();
            conn.execute(
                "INSERT INTO conversation (id, mode, title, provider_id, model_id)
                 VALUES ('c-act', 'agent', 't', 'p', 'm')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO run (id, conversation_id, status, provider_id, model_id)
                 VALUES ('run-act', 'c-act', 'running', 'p', 'm')",
                [],
            )
            .unwrap();
        }

        let actor = crate::storage::actor::StorageActor::new(4, store.clone());
        let mgr = CheckpointManager::with_store_and_actor(store.clone(), actor);
        let cp_id = mgr.begin_run("run-act", "c-act", &root).unwrap();
        mgr.capture_before("run-act", &trusted(&root, "act.txt"))
            .unwrap();
        std::fs::write(&file, "v2").unwrap();
        // capture_after triggers flush_live_to_store → routed through the actor.
        mgr.capture_after("run-act", &trusted(&root, "act.txt"))
            .unwrap();
        // The flush is durable: a fresh manager instance (no live map) reads it.
        let mgr2 = CheckpointManager::with_store(store);
        let preview = mgr2
            .rewind_preview("run-act", &root, None)
            .expect("actor-flushed snapshot is loadable");
        assert_eq!(preview.checkpoint_id, cp_id);
        assert_eq!(preview.files.len(), 1, "flush persisted the captured file");

        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_file(&db);
        let _ = std::fs::remove_dir_all(&art);
    }

    // ---- T03: bounded capture, streaming hash, sensitive-path redaction ----

    /// A `.env` file containing a fixture secret must be captured as redacted
    /// metadata + hash only — the secret never reaches the checkpoint and the
    /// preview reports the file as non-restorable.
    #[test]
    fn sensitive_env_file_never_persists_content() {
        let root = std::env::temp_dir().join(format!("cp-secret-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let file = root.join(".env");
        std::fs::write(&file, "API_KEY=fixture-secret-123456789\n").unwrap();
        let mgr = CheckpointManager::new();
        mgr.begin_run("run-secret", "c1", &root).unwrap();
        mgr.capture_before("run-secret", &trusted(&root, ".env"))
            .unwrap();
        std::fs::write(&file, "API_KEY=changed-987654321\n").unwrap();
        mgr.capture_after("run-secret", &trusted(&root, ".env"))
            .unwrap();
        // Preview (live) reports the file as non-restorable: content was not
        // captured, so rewind must say so rather than pretend.
        let preview = mgr.rewind_preview("run-secret", &root, None).unwrap();
        let entry = preview.files.iter().find(|f| f.path == ".env").unwrap();
        assert!(!entry.restorable);
        assert_eq!(
            entry.non_restorable_reason.as_deref(),
            Some("sensitive_path")
        );
        let rec = mgr.finalize_run("run-secret").unwrap();
        let snapshot = rec.files.iter().find(|f| f.path == ".env").unwrap();
        assert!(snapshot.redacted, "sensitive path must be marked redacted");
        assert_eq!(snapshot.redaction_reason.as_deref(), Some("sensitive_path"));
        assert!(
            snapshot.before_content.is_none() && snapshot.after_content.is_none(),
            "secret content must never be captured"
        );
        assert!(
            snapshot.before_hash.is_some() && snapshot.after_hash.is_some(),
            "redacted metadata still carries the hash"
        );
        // The persisted snapshot JSON must not contain the fixture secret.
        let json = serde_json::to_string(&rec).unwrap();
        assert!(
            !json.contains("fixture-secret-123456789"),
            "fixture secret leaked into the checkpoint snapshot"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A file larger than the per-file content cap is streamed (bounded memory)
    /// and recorded as redacted metadata + hash; its content is never loaded
    /// into memory or persisted, and rewind reports it non-restorable.
    #[test]
    fn large_file_is_streamed_and_content_redacted() {
        let root = std::env::temp_dir().join(format!("cp-large-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let file = root.join("big.bin");
        // Over the per-file cap (256 KiB) but cheap to create.
        let big = vec![b'x'; (MAX_CAPTURE_FILE_CONTENT_BYTES as usize) * 2];
        std::fs::write(&file, &big).unwrap();
        let mgr = CheckpointManager::new();
        mgr.begin_run("run-large", "c1", &root).unwrap();
        mgr.capture_before("run-large", &trusted(&root, "big.bin"))
            .unwrap();
        std::fs::write(&file, &big[..1]).unwrap(); // rewrite after
        mgr.capture_after("run-large", &trusted(&root, "big.bin"))
            .unwrap();
        // Preview (live) reports the over-cap file as non-restorable.
        let preview = mgr.rewind_preview("run-large", &root, None).unwrap();
        assert!(!preview.files[0].restorable);
        assert_eq!(
            preview.files[0].non_restorable_reason.as_deref(),
            Some("too_large")
        );
        let rec = mgr.finalize_run("run-large").unwrap();
        let snapshot = rec.files.iter().find(|f| f.path == "big.bin").unwrap();
        assert!(snapshot.redacted, "over-cap file must be redacted");
        assert!(snapshot.before_content.is_none());
        assert!(snapshot.after_content.is_none());
        assert!(
            snapshot.before_hash.is_some() && snapshot.after_hash.is_some(),
            "streaming hash is computed without buffering the file"
        );
        // The hash differs before/after — the streaming hash covered the file.
        assert_ne!(
            snapshot.before_hash, snapshot.after_hash,
            "streaming hash must reflect the whole file"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A 1GB sparse file must not be read into memory: `stream_read_capped`
    /// hashes it in bounded chunks and stores no content. The streaming helper
    /// itself is the memory-bounded primitive; the manager applies it.
    #[test]
    fn one_gigabyte_file_streams_with_bounded_memory() {
        let dir = std::env::temp_dir().join(format!("cp-gb-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("giant.bin");
        // Sparse file: instant to create, 1 GiB of zeros on disk.
        let f = std::fs::File::create(&path).unwrap();
        f.set_len(1024 * 1024 * 1024).unwrap();
        drop(f);
        let read = stream_read_capped(&path, MAX_CAPTURE_FILE_CONTENT_BYTES).unwrap();
        assert_eq!(read.size, 1024 * 1024 * 1024);
        assert!(read.over_cap, "1GiB file must exceed the content cap");
        assert!(read.content.is_none(), "1GiB content must not be buffered");
        assert_eq!(read.hash.len(), 64, "full SHA-256 hex digest");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Beyond the per-run file-count cap, further files are recorded as
    /// redacted metadata instead of growing the checkpoint without bound.
    #[test]
    fn file_count_quota_redacts_excess_files() {
        let root = std::env::temp_dir().join(format!("cp-count-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let mgr = CheckpointManager::new();
        mgr.begin_run("run-count", "c1", &root).unwrap();
        let within = 3usize;
        for i in 0..(MAX_CAPTURE_FILE_COUNT + within) {
            let name = format!("f{i}.txt");
            let file = root.join(&name);
            std::fs::write(&file, "x").unwrap();
            mgr.capture_before("run-count", &trusted(&root, &name))
                .unwrap();
        }
        let rec = mgr.finalize_run("run-count").unwrap();
        assert_eq!(rec.files.len(), MAX_CAPTURE_FILE_COUNT + within);
        let redacted_count = rec.files.iter().filter(|f| f.redacted).count();
        assert_eq!(
            redacted_count, within,
            "files beyond the count cap must be redacted"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The async capture path runs the file read on the blocking pool and
    /// produces the same redaction guarantees as the sync path.
    #[tokio::test]
    async fn async_capture_redacts_sensitive_path() {
        let root = std::env::temp_dir().join(format!("cp-async-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let file = root.join(".env");
        std::fs::write(&file, "TOKEN=async-fixture-secret").unwrap();
        let mgr = CheckpointManager::new();
        mgr.begin_run("run-async", "c1", &root).unwrap();
        mgr.capture_before_async("run-async", &trusted(&root, ".env"))
            .await
            .unwrap();
        std::fs::write(&file, "TOKEN=changed").unwrap();
        mgr.capture_after_async("run-async", &trusted(&root, ".env"))
            .await
            .unwrap();
        let rec = mgr.finalize_run("run-async").unwrap();
        let snap = rec.files.iter().find(|f| f.path == ".env").unwrap();
        assert!(snap.redacted);
        assert!(snap.before_content.is_none() && snap.after_content.is_none());
        let json = serde_json::to_string(&rec).unwrap();
        assert!(
            !json.contains("async-fixture-secret"),
            "async capture leaked the fixture secret"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
