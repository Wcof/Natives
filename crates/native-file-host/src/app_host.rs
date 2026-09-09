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
