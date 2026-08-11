//! Checkpoint rewind helpers (W9 split from checkpoint.rs): preview building,
//! atomic workspace restore with rollback, and snapshot file parsing.

use super::checkpoint_stream::stream_read_capped;
use super::{
    CheckpointRecord, FileSnapshot, RewindConflict, RewindFilePreview, RewindPreview,
    MAX_CAPTURE_FILE_CONTENT_BYTES,
};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Paths whose contents must never be captured in a checkpoint. Only redacted
/// metadata + hash are stored; rewind reports these files as non-restorable.
///
/// Covers `.env` files, credential/key/secret material, private-key formats,
/// and database files. Conservative on purpose: over-matching redacts content
/// that simply will not be restorable; under-matching would persist secrets.
pub(super) fn is_sensitive_checkpoint_path(path: &Path) -> bool {
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
pub(super) fn build_rewind_preview(
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
pub(super) fn apply_rewind_files(
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
                    let tmp =
                        abs.with_extension(format!("natives-restore-tmp-{}", uuid::Uuid::new_v4()));
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

pub(super) fn parse_files_json(snap: &str) -> Result<Vec<FileSnapshot>, String> {
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
