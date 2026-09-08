//! Filesystem half of an App install transaction (ADR-0025 D15).
//! The caller persists Rollback before touching version directories.

use crate::app_store::types::{AppError, InstallRequest};
use crate::{app_host_manifest, app_install};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Serialize, Deserialize)]
pub(crate) struct Rollback {
    pub current: Option<Vec<u8>>,
    pub manifest: Option<Vec<u8>>,
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, AppError> {
    match fs::read(path) {
        Ok(bytes) if bytes.len() <= 16 * 1024 => Ok(Some(bytes)),
        Ok(_) => Err(AppError::InvalidState("registration file too large".into())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn snapshot(
    root: &Path,
    request: &InstallRequest,
    manifests: &Path,
) -> Result<Rollback, AppError> {
    let current = app_install::runtime_current_path(root, &request.app.app_id);
    app_install::validate_app_path(root, &current)?;
    let manifest = app_host_manifest::manifest_path_in(manifests, request.host_name()?)?;
    for package in &request.packages {
        let target = app_install::install_path_for(
            root,
            &request.app.app_id,
            &package.kind,
            &package.version,
            &package.package_id,
        )?;
        if target.parent().is_some_and(Path::exists) {
            return Err(AppError::Conflict(
                "target version already exists; recover or uninstall first".into(),
            ));
        }
    }
    Ok(Rollback {
        current: read_optional(&current)?,
        manifest: read_optional(&manifest)?,
    })
}

/// Move a complete verified package set, then probe it before changing any
/// registration. A failed probe cannot redirect Chrome to the new runtime.
pub(crate) fn prepare(
    root: &Path,
    request: &InstallRequest,
    install_id: &str,
) -> Result<std::path::PathBuf, AppError> {
    let staging = app_install::staging_dir(root, &request.app.app_id, install_id)?;
    let mut runtime = None;
    for package in &request.packages {
        let staged = app_install::staged_payload_path(&staging, &package.package_id)?;
        let bytes = fs::read(&staged)?;
        if bytes.len() as i64 != package.payload_size
            || !app_install::sha256_matches(
                &package.payload_sha256,
                &app_install::hex_sha256(&bytes),
            )
        {
            return Err(AppError::InvalidState(
                "staged package changed before commit".into(),
            ));
        }
        let target = app_install::install_path_for(
            root,
            &request.app.app_id,
            &package.kind,
            &package.version,
            &package.package_id,
        )?;
        app_install::install_staged_payload(&staged, &target)?;
        if package.kind == "runtime" {
            app_install::mark_executable(&target)?;
            app_install::health_check_binary(&target, &request.app.version)?;
            runtime = Some(target);
        }
    }
    runtime.ok_or_else(|| AppError::InvalidState("runtime package missing".into()))
}

pub(crate) fn activate(
    root: &Path,
    request: &InstallRequest,
    runtime: &Path,
    origin: &str,
    manifests: &Path,
) -> Result<(), AppError> {
    app_host_manifest::write_manifest_in(manifests, request.host_name()?, runtime, origin)?;
    app_install::set_runtime_current(root, &request.app.app_id, &request.app.version)
}

fn restore(path: &Path, bytes: &Option<Vec<u8>>) -> Result<(), AppError> {
    match bytes {
        Some(bytes) => app_install::atomic_write(path, bytes),
        None => app_install::remove_file_if_present(path),
    }
}

pub(crate) fn rollback(
    root: &Path,
    request: &InstallRequest,
    saved: &Rollback,
    manifests: &Path,
) -> Result<(), AppError> {
    let current = app_install::runtime_current_path(root, &request.app.app_id);
    app_install::validate_app_path(root, &current)?;
    let manifest = app_host_manifest::manifest_path_in(manifests, request.host_name()?)?;
    restore(&manifest, &saved.manifest)?;
    restore(&current, &saved.current)?;
    remove_version(root, &request.app.app_id, &request.app.version)?;
    Ok(())
}

pub(crate) fn remove_version(root: &Path, app_id: &str, version: &str) -> Result<(), AppError> {
    app_install::validate_identifier(version, "version")?;
    for base in ["runtime", "packages"] {
        let dir = root.join(app_id).join(base).join(version);
        app_install::validate_app_path(root, &dir)?;
        app_install::remove_staging_dir(&dir)?;
        let parent = root.join(app_id).join(base);
        if parent.is_dir() && fs::read_dir(&parent)?.next().is_none() {
            fs::remove_dir(parent)?;
        }
    }
    Ok(())
}
