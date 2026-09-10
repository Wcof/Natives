//! Filesystem version directory helpers (ADR-0026 D2).

use crate::app_install;
use crate::app_store::types::AppError;
use std::fs;
use std::path::Path;

pub(crate) fn remove_version(root: &Path, app_id: &str, version: &str) -> Result<(), AppError> {
    app_install::validate_identifier(version, "version")?;
    for base in ["runtime", "packages"] {
        let dir = root.join(app_id).join(base).join(version);
        app_install::validate_app_path(root, &dir)?;
        app_install::remove_staging_dir(&dir)?;
        let parent = root.join(app_id).join(base);
        if parent.is_dir() && fs::read_dir(&parent)?.next().is_none() {
            let _ = fs::remove_dir(parent);
        }
    }
    Ok(())
}

/// Keep only the listed version directories under `runtime/` and
/// `packages/`, removing anything older (contract §5 step 7: after a commit
/// exactly one previous version is retained; older ones and staging are
/// cleaned). Names come from our own validated install layout.
pub(crate) fn retain_versions(root: &Path, app_id: &str, keep: &[&str]) -> Result<(), AppError> {
    for base in ["runtime", "packages"] {
        let base_dir = root.join(app_id).join(base);
        let entries = match fs::read_dir(&base_dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries {
            let entry = entry?;
            let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
            if keep.contains(&name.as_str()) {
                continue;
            }
            remove_version(root, app_id, &name)?;
        }
    }
    Ok(())
}
