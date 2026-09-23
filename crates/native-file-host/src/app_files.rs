//! Safe filesystem helpers for the complete product's per-user module data.

use crate::app_store::types::AppError;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub fn default_app_root() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(crate::app_signing::natives_dir_name())
        .join("apps")
}

pub(crate) fn validate_identifier(id: &str, kind: &str) -> Result<(), AppError> {
    if id.is_empty()
        || id.len() > 128
        || id.contains('/')
        || id.contains('\\')
        || id.contains('\0')
        || id == "."
        || id == ".."
        || id.starts_with('~')
        || id.starts_with('/')
        || (id.len() >= 2 && id.as_bytes()[1] == b':')
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'+'))
    {
        return Err(AppError::InvalidState(format!("invalid {kind}: {id:?}")));
    }
    Ok(())
}

pub(crate) fn validate_app_path(root: &Path, path: &Path) -> Result<(), AppError> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| AppError::InvalidState("path outside app root".into()))?;
    let mut current = root.to_path_buf();
    let mut paths = vec![current.clone()];
    for component in relative.components() {
        let std::path::Component::Normal(name) = component else {
            return Err(AppError::InvalidState("invalid app path component".into()));
        };
        current.push(name);
        paths.push(current.clone());
    }
    for path in paths {
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(AppError::InvalidState(
                    "symlinks are not allowed in app paths".into(),
                ))
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

/// A single OS lock covers module data reset and preference changes.
pub(crate) fn acquire_app_lock(
    root: &Path,
    app_id: &str,
    runtime: bool,
) -> Result<std::fs::File, AppError> {
    validate_identifier(app_id, "app id")?;
    let suffix = if runtime { "runtime" } else { "operation" };
    let id = app_id.strip_prefix("com.natives.app.").unwrap_or(app_id);
    let path = root.join(".locks").join(format!("{id}.{suffix}.lock"));
    validate_app_path(root, &path)?;
    std::fs::create_dir_all(root.join(".locks"))?;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(&path)?;
    let deadline = Instant::now() + Duration::from_secs(if runtime { 2 } else { 0 });
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(std::fs::TryLockError::WouldBlock) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10))
            }
            Err(std::fs::TryLockError::WouldBlock) => {
                return Err(AppError::Conflict(format!(
                    "APP_BUSY: module is in use ({})",
                    path.display()
                )))
            }
            Err(std::fs::TryLockError::Error(error)) => return Err(error.into()),
        }
    }
}

pub fn remove_staging_dir(dir: &Path) -> Result<(), AppError> {
    if dir.exists() {
        std::fs::remove_dir_all(dir)?;
    }
    Ok(())
}
