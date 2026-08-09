//! Trash and bulk batch operations (multi-entry trash / move / copy into a
//! destination directory). Individual failures are reported, not fatal.

use super::*;
use crate::{Error, Result};

/// Trash entry (macOS/Linux/Windows via trash crate)
pub fn trash_entry(file_path: &str) -> Result<()> {
    let path = expand_tilde(file_path);
    if !path.exists() {
        return Err(Error::NotFound(file_path.to_string()));
    }
    // Enforce allowlist on the resolved target so trashing can't reach
    // blocklisted/out-of-scope paths.
    let canon = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
    validate_path(&canon)?;

    // Use trash crate
    ::trash::delete(&path).map_err(|e| Error::Internal(format!("trash failed: {e}")))
}

/// Batch trash. Continues on individual failures and reports them.
pub fn trash_entries(paths: &[String]) -> Result<serde_json::Value> {
    let mut ok: Vec<String> = Vec::new();
    let mut errors: Vec<serde_json::Value> = Vec::new();
    for p in paths {
        match trash_entry(p) {
            Ok(()) => ok.push(p.clone()),
            Err(e) => errors.push(serde_json::json!({ "path": p, "error": e.to_string() })),
        }
    }
    Ok(serde_json::json!({
        "ok": errors.is_empty(),
        "trashed": ok,
        "errors": errors,
        "count": ok.len(),
    }))
}

/// Batch move into a destination directory. Auto-dedupe, cross-volume safe.
pub fn move_entries(paths: &[String], dest_dir: &str) -> Result<serde_json::Value> {
    let dest = expand_tilde(dest_dir);
    if !dest.exists() {
        std::fs::create_dir_all(&dest).map_err(Error::Io)?;
    }
    validate_path(&dest)?;
    if !dest.is_dir() {
        return Err(Error::InvalidInput("destination is not a directory".into()));
    }

    let mut moved: Vec<String> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    let mut errors: Vec<serde_json::Value> = Vec::new();
    let dest_canon = std::fs::canonicalize(&dest).unwrap_or_else(|_| dest.clone());
    for p in paths {
        let src = expand_tilde(p);
        // Skip no-op: already directly inside dest
        if let Some(parent) = src.parent() {
            let parent_canon =
                std::fs::canonicalize(parent).unwrap_or_else(|_| parent.to_path_buf());
            if parent_canon == dest_canon {
                skipped.push(p.clone());
                continue;
            }
        }
        // Prevent moving a directory into itself / its descendant
        if src.is_dir() {
            let src_canon = std::fs::canonicalize(&src).unwrap_or_else(|_| src.clone());
            if dest_canon.starts_with(&src_canon) {
                errors.push(
                    serde_json::json!({ "path": p, "error": "cannot move a folder into itself" }),
                );
                continue;
            }
        }
        match move_entry(p, dest.to_string_lossy().as_ref()) {
            Ok(path) => moved.push(path),
            Err(e) => errors.push(serde_json::json!({ "path": p, "error": e.to_string() })),
        }
    }
    Ok(serde_json::json!({
        "ok": errors.is_empty(),
        "moved": moved,
        "skipped": skipped,
        "errors": errors,
        "count": moved.len(),
    }))
}

/// Batch copy into a destination directory. Auto-dedupe. Source preserved.
pub fn copy_entries(paths: &[String], dest_dir: &str) -> Result<serde_json::Value> {
    let dest = expand_tilde(dest_dir);
    if !dest.exists() {
        std::fs::create_dir_all(&dest).map_err(Error::Io)?;
    }
    validate_path(&dest)?;
    if !dest.is_dir() {
        return Err(Error::InvalidInput("destination is not a directory".into()));
    }

    let mut copied: Vec<String> = Vec::new();
    let mut errors: Vec<serde_json::Value> = Vec::new();
    for p in paths {
        match copy_entry(p, dest.to_string_lossy().as_ref()) {
            Ok(path) => copied.push(path),
            Err(e) => errors.push(serde_json::json!({ "path": p, "error": e.to_string() })),
        }
    }
    Ok(serde_json::json!({
        "ok": errors.is_empty(),
        "copied": copied,
        "errors": errors,
        "count": copied.len(),
    }))
}
