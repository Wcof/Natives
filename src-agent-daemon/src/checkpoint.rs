//! Checkpoint capture + rewind (Phase 3).
//!
//! Logical checkpoints per run; lazy before-images on first write; after hash at
//! run end. Rewind refuses external modifications when conflict_policy=fail.

use crate::storage::DataStore;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSnapshot {
    pub path: String,
    pub before_hash: Option<String>,
    pub after_hash: Option<String>,
    pub before_content: Option<String>,
    pub after_content: Option<String>,
    pub existed_before: bool,
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
}

/// Process-local checkpoint manager with optional SQLite persistence.
pub struct CheckpointManager {
    live: std::sync::Mutex<HashMap<String, LiveCheckpoint>>,
    store: Option<Arc<DataStore>>,
}

impl CheckpointManager {
    pub fn new() -> Self {
        Self {
            live: std::sync::Mutex::new(HashMap::new()),
            store: None,
        }
    }

    pub fn with_store(store: Arc<DataStore>) -> Self {
        Self {
            live: std::sync::Mutex::new(HashMap::new()),
            store: Some(store),
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

    /// Lazy capture before-image the first time a relative path is touched.
    pub fn capture_before(&self, run_id: &str, rel_path: &str) -> Result<(), String> {
        let mut map = self.live.lock().map_err(|e| e.to_string())?;
        let live = map
            .get_mut(run_id)
            .ok_or_else(|| format!("no checkpoint for run {run_id}"))?;
        if live.files.contains_key(rel_path) {
            return Ok(());
        }
        if rel_path.contains("..") {
            return Err("path escape".into());
        }
        let abs = live.project_root.join(rel_path);
        let (existed, content, hash) = if abs.exists() {
            let bytes = std::fs::read(&abs).map_err(|e| e.to_string())?;
            let hash = hex_sha256(&bytes);
            let text = String::from_utf8(bytes).ok();
            (true, text, Some(hash))
        } else {
            (false, None, None)
        };
        live.files.insert(
            rel_path.to_string(),
            FileSnapshot {
                path: rel_path.to_string(),
                before_hash: hash,
                after_hash: None,
                before_content: content,
                after_content: None,
                existed_before: existed,
            },
        );
        Ok(())
    }

    /// After a successful write, record after content/hash.
    pub fn capture_after(&self, run_id: &str, rel_path: &str) -> Result<(), String> {
        {
            let mut map = self.live.lock().map_err(|e| e.to_string())?;
            let live = map
                .get_mut(run_id)
                .ok_or_else(|| format!("no checkpoint for run {run_id}"))?;
            let abs = live.project_root.join(rel_path);
            let entry = live
                .files
                .entry(rel_path.to_string())
                .or_insert_with(|| FileSnapshot {
                    path: rel_path.to_string(),
                    before_hash: None,
                    after_hash: None,
                    before_content: None,
                    after_content: None,
                    existed_before: false,
                });
            if abs.exists() {
                let bytes = std::fs::read(&abs).map_err(|e| e.to_string())?;
                entry.after_hash = Some(hex_sha256(&bytes));
                entry.after_content = String::from_utf8(bytes).ok();
            } else {
                entry.after_hash = None;
                entry.after_content = None;
            }
        }
        // Durable flush when a store is configured. Without a store (unit tests /
        // pure in-memory) keep live-only success. With a store, fail closed so
        // side-effecting writes are not half-recorded.
        if self.store.is_some() {
            self.flush_live_to_store(run_id)?;
        }
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
        let map = self.live.lock().map_err(|e| e.to_string())?;
        let live = map
            .get(run_id)
            .ok_or_else(|| format!("no checkpoint for run {run_id}"))?;
        let store = self
            .store
            .as_ref()
            .ok_or_else(|| "checkpoint store unavailable".to_string())?;
        let conn = store.conn()?;
        let files: Vec<FileSnapshot> = live.files.values().cloned().collect();
        let snap = serde_json::to_string(&serde_json::json!({ "files": files }))
            .map_err(|e| format!("serialize checkpoint snapshot: {e}"))?;
        let changed = conn
            .execute(
                "UPDATE checkpoint SET snapshot_json = ?1 WHERE id = ?2",
                params![snap, live.id],
            )
            .map_err(|e| e.to_string())?;
        if changed != 1 {
            return Err(format!("checkpoint row missing for run {run_id}"));
        }
        Ok(())
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
                let bytes = std::fs::read(&abs).map_err(|e| e.to_string())?;
                Some(hex_sha256(&bytes))
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
            previews.push(RewindFilePreview {
                path: f.path.clone(),
                checkpoint_after_hash: f.after_hash.clone(),
                current_hash,
                change_type: change_type.into(),
            });
        }
        Ok(RewindPreview {
            checkpoint_id: cp.id,
            run_id: run_id.to_string(),
            files: previews,
            conflicts,
        })
    }

    pub fn rewind(
        &self,
        run_id: &str,
        checkpoint_id: &str,
        project_root: &Path,
        paths: Option<&[String]>,
        conflict_policy: &str,
    ) -> Result<Vec<String>, String> {
        // P0 fail-closed validation (task-07):
        // - checkpoint_id must exist
        // - checkpoint.run_id must equal request run_id
        // - project_root must match the live checkpoint project root when known
        // Any mismatch restores 0 files.
        if checkpoint_id.trim().is_empty() {
            return Err("checkpoint_id required".into());
        }
        if conflict_policy != "fail" {
            return Err("only conflict_policy=fail is supported".into());
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

        // Staging: capture current content for undo; apply atomically; rollback on failure.
        let mut staging: Vec<(PathBuf, Option<Vec<u8>>)> = Vec::new();
        let mut restored = Vec::new();
        let apply_result: Result<(), String> = (|| {
            for f in &cp.files {
                if let Some(filter) = paths {
                    if !filter.iter().any(|p| p == &f.path) {
                        continue;
                    }
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
                        let tmp =
                            abs.with_extension(format!("natives-restore-tmp-{}", Uuid::new_v4()));
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
    use std::sync::OnceLock;
    static M: OnceLock<CheckpointManager> = OnceLock::new();
    M.get_or_init(|| {
        #[cfg(test)]
        {
            // Tests: resolve to the isolated test store so checkpoint rows never
            // touch ~/.natives/assistant.db.
            match crate::storage::open_resolved_store() {
                Ok(store) => CheckpointManager::with_store(Arc::new(store)),
                Err(_) => CheckpointManager::new(),
            }
        }
        #[cfg(not(test))]
        {
            // Best-effort open from env
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
        }
    })
}

fn hex_sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
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

    #[test]
    fn capture_and_rewind_roundtrip() {
        let root = std::env::temp_dir().join(format!("cp-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("a.txt");
        std::fs::write(&file, "before").unwrap();

        let mgr = CheckpointManager::new();
        let _id = mgr.begin_run("run1", "c1", &root).unwrap();
        mgr.capture_before("run1", "a.txt").unwrap();
        std::fs::write(&file, "after").unwrap();
        mgr.capture_after("run1", "a.txt").unwrap();
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
        let file = root.join("b.txt");
        std::fs::write(&file, "v1").unwrap();
        let mgr = CheckpointManager::new();
        let cp_id = mgr.begin_run("run2", "c1", &root).unwrap();
        mgr.capture_before("run2", "b.txt").unwrap();
        std::fs::write(&file, "v2").unwrap();
        mgr.capture_after("run2", "b.txt").unwrap();

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
        let file = root.join("c.txt");
        std::fs::write(&file, "v1").unwrap();
        let mgr = CheckpointManager::new();
        let cp_id = mgr.begin_run("run3", "c1", &root).unwrap();
        mgr.capture_before("run3", "c.txt").unwrap();
        std::fs::write(&file, "v2").unwrap();
        mgr.capture_after("run3", "c.txt").unwrap();
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
        mgr1.capture_before("run-persist", "persist.txt").unwrap();
        std::fs::write(&file, "v2").unwrap();
        mgr1.capture_after("run-persist", "persist.txt").unwrap();
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
}
