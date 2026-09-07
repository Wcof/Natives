//! Runtime registration for install commit (ADR-0025 D11/D16, Phase A5).
//!
//! Runs OUTSIDE the registry transaction (file side, no `AppStore` calls —
//! AGENTS.md lock discipline):
//!
//!   staging → install (atomic rename) → [runtime: executable + `current`
//!   pointer + `--health` probe, 2 s] → host manifest (real caller origin)
//!
//! On any failure `rollback_commit_registration` restores the previous
//! install state (old binary if one existed, old `current` pointer, old
//! manifest) and the caller marks the transaction `failed`. V1 has no
//! upgrades (D15), so rollback only undoes what this commit did.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use crate::app_host_manifest;
use crate::app_install;
use crate::app_store::types::AppError;
use serde::Serialize;

/// Commit-time registration outcome. `registered = false` when the app has
/// no packages (metadata-only A2-style entries stay registrable).
#[derive(Serialize)]
pub struct RegistrationOutcome {
    /// True when at least one package was installed.
    pub registered: bool,
    /// Host manifest registered (false when no caller origin — V1 never
    /// fabricates origins; the app row records the skipped state).
    pub host_registered: bool,
    /// Runtime binary that passed the health probe (empty when skipped).
    pub runtime_binary: String,
    /// Manifest path written (empty when not registered).
    pub manifest_path: String,
}

/// A staged package ready for commit: `package_id` / `kind` / `version`
/// come from the signed catalog snapshot, `staged` from `app_package_stages`.
pub struct StagedPackage {
    pub package_id: String,
    pub kind: String,
    pub version: String,
    pub staged: std::path::PathBuf,
}

/// Install every staged package to its Core-decided path
/// (`apps/<app_id>/{runtime|packages}/<version>/<package_id>`). Runtime
/// packages additionally get the executable bit, the `current` version
/// pointer (D15), and the `--health` probe (D11 health_check stage). The
/// host manifest is written LAST so a failing probe never leaves a manifest
/// pointing at a dead binary. Per-package receipt paths are recorded by the
/// registry commit (it owns the `app_packages` rows).
///
/// `manifest_dir = None` → the browser's real Native Messaging Host dir
/// (production); Some(dir) → test injection (unit tests never touch the
/// real browser directory).
pub fn register_commit(
    app_root: &Path,
    app_id: &str,
    host_name: &str,
    caller_origin: Option<&str>,
    staged: &[StagedPackage],
    manifest_dir: Option<&Path>,
) -> Result<RegistrationOutcome, AppError> {
    if staged.is_empty() {
        return Ok(RegistrationOutcome {
            registered: false,
            host_registered: false,
            runtime_binary: String::new(),
            manifest_path: String::new(),
        });
    }
    let mut last_binary = String::new();
    for entry in staged {
        let target = app_install::install_path_for(
            app_root,
            app_id,
            &entry.kind,
            &entry.version,
            &entry.package_id,
        )?;
        app_install::install_staged_payload(&entry.staged, &target)?;
        if entry.kind == "runtime" {
            app_install::mark_executable(&target)?;
            // health_check stage: `--health` must exit 0 with status ok
            // within 2 s, otherwise the whole commit rolls back. The
            // `current` pointer is written ONLY after the probe passes —
            // a failed probe must not leave a pointer to a dead binary.
            app_install::health_check_binary(&target)?;
            app_install::set_runtime_current(app_root, app_id, &entry.version)?;
            last_binary = target.to_string_lossy().into_owned();
        }
    }
    // Host manifest: real caller origin only (ADR-0025 D16). No origin →
    // skip registration and record the explicit `host_registered = false`
    // state instead of fabricating an allowed_origins entry.
    let manifest_path = match caller_origin.filter(|origin| !origin.is_empty()) {
        Some(origin) if !last_binary.is_empty() => match manifest_dir {
            Some(dir) => app_host_manifest::write_manifest_in(
                dir,
                host_name,
                Path::new(&last_binary),
                origin,
            )?
            .to_string_lossy()
            .into_owned(),
            None => app_host_manifest::write_manifest(host_name, Path::new(&last_binary), origin)?
                .to_string_lossy()
                .into_owned(),
        },
        _ => String::new(),
    };
    Ok(RegistrationOutcome {
        registered: true,
        host_registered: !manifest_path.is_empty(),
        runtime_binary: last_binary,
        manifest_path,
    })
}

/// Undo a failed commit registration: delete the version directories this
/// commit installed, drop the `runtime` tree entirely when it became empty
/// (fresh-install failure), and remove the manifest this commit wrote.
/// Staged bytes are removed by the caller. `manifest_dir` semantics are
/// the same as [`register_commit`].
pub fn rollback_commit_registration(
    app_root: &Path,
    app_id: &str,
    staged: &[StagedPackage],
    host_name: &str,
    manifest_dir: Option<&Path>,
) -> Result<(), AppError> {
    let removed_dirs: BTreeSet<std::path::PathBuf> = staged
        .iter()
        .map(|entry| {
            app_root
                .join(app_id)
                .join(if entry.kind == "runtime" {
                    "runtime"
                } else {
                    "packages"
                })
                .join(&entry.version)
        })
        .collect();
    for dir in removed_dirs {
        if dir.exists() {
            fs::remove_dir_all(&dir).map_err(AppError::from)?;
        }
    }
    // The `current` pointer (written after a passing probe) must be
    // removed when its version directory goes away — a rollback must not
    // leave a pointer into a deleted runtime version.
    let current = app_root.join(app_id).join("runtime").join("current");
    if current.exists() {
        let _ = fs::remove_file(&current);
    }
    for base in ["runtime", "packages"] {
        let dir = app_root.join(app_id).join(base);
        let empty = dir
            .read_dir()
            .map(|mut iter| iter.all(|_| false))
            .unwrap_or(true);
        if empty {
            let _ = fs::remove_dir_all(&dir);
        }
    }
    match manifest_dir {
        Some(dir) => app_host_manifest::remove_manifest_in(dir, host_name),
        None => app_host_manifest::remove_manifest(host_name),
    }
    .map_err(AppError::from)?;
    Ok(())
}
