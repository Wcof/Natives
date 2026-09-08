//! Package staging filesystem helpers (ADR-0025 D8/D16/D17).
//!
//! The Core Host never downloads: the browser fetches the `.nap`, enforces
//! the 5 MiB wire gate + artifact hash + gzip, and posts the decompressed
//! payload as base64. This module only decides WHERE staged bytes live
//! (Core-decided paths, never catalog-provided — ADR-0025 D9), writes
//! them, hashes them, and health-probes runtime binaries.

use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::app_store::types::AppError;

/// Default install root: `~/.natives/apps` (ADR-0025 D16).
pub fn default_app_root() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".natives").join("apps")
}

/// Reject anything that could escape the app root: separators, `.`/`..`,
/// absolute or `~`-relative prefixes (path-security CI gate, ADR-0025 §95).
pub(crate) fn validate_identifier(id: &str, kind: &str) -> Result<(), AppError> {
    if id.is_empty()
        || id.len() > 128
        || id.contains('/')
        || id.contains('\\')
        || id.contains('\0')
        || id == "."
        || id == ".."
        || id.starts_with('~')
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'+'))
        || id.starts_with('/')
        || (id.len() >= 2 && id.as_bytes()[1] == b':')
    {
        return Err(AppError::InvalidState(format!("invalid {kind}: {id:?}")));
    }
    Ok(())
}

/// Reject linked components before operating on a Core-owned app path.
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
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), AppError> {
    let parent = path
        .parent()
        .ok_or_else(|| AppError::InvalidState("missing file parent".into()))?;
    std::fs::create_dir_all(parent)?;
    let temp = path.with_extension(format!("{}.tmp", crate::workspace_store::schema::uuid_v4()));
    let result = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp, path)?;
        #[cfg(unix)]
        std::fs::File::open(parent)?.sync_all()?;
        Ok::<(), AppError>(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temp);
    }
    result
}

/// Stage directory for one install transaction:
/// `~/.natives/apps/<app_id>/staging/<install_id>/`.
pub fn staging_dir(app_root: &Path, app_id: &str, install_id: &str) -> Result<PathBuf, AppError> {
    validate_identifier(app_id, "app_id")?;
    validate_identifier(install_id, "install_id")?;
    let path = app_root.join(app_id).join("staging").join(install_id);
    validate_app_path(app_root, &path)?;
    Ok(path)
}

/// Staged payload file: `<staging_dir>/<package_id>.payload`.
pub fn staged_payload_path(staging: &Path, package_id: &str) -> Result<PathBuf, AppError> {
    validate_identifier(package_id, "package_id")?;
    let path = staging.join(format!("{package_id}.payload"));
    validate_app_path(staging, &path)?;
    Ok(path)
}

/// Lock files stay outside the removable app tree, so their inode cannot
/// change during uninstall. OS ownership ends automatically on process exit.
pub(crate) fn acquire_app_lock(
    root: &Path,
    app_id: &str,
    runtime: bool,
) -> Result<std::fs::File, AppError> {
    validate_identifier(app_id, "app id")?;
    let suffix = if runtime { "runtime" } else { "install" };
    let id = app_id.strip_prefix("com.natives.app.").unwrap_or(app_id);
    let path = root.join(".locks").join(format!("{id}.{suffix}.lock"));
    validate_app_path(root, &path)?;
    std::fs::create_dir_all(root.join(".locks"))?;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)?;
    let deadline = Instant::now() + Duration::from_secs(if runtime { 2 } else { 0 });
    loop {
        match file.try_lock() {
            Ok(()) => break,
            Err(std::fs::TryLockError::WouldBlock) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(std::fs::TryLockError::WouldBlock) => {
                return Err(AppError::Conflict(
                    "APP_BUSY: app is in use by another operation or runtime".into(),
                ))
            }
            Err(std::fs::TryLockError::Error(error)) => return Err(error.into()),
        }
    }
    Ok(file)
}

pub(crate) fn remove_file_if_present(path: &Path) -> Result<(), AppError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

/// Core-decided final install path from `kind` (ADR-0025 D9: the catalog
/// describes the target, the Core decides the path). V1 kinds:
/// - `runtime` → `apps/<app_id>/runtime/<version>/<package_id>` (binary)
/// - `data`    → `apps/<app_id>/packages/<version>/<package_id>`
pub fn install_path_for(
    app_root: &Path,
    app_id: &str,
    kind: &str,
    version: &str,
    package_id: &str,
) -> Result<PathBuf, AppError> {
    validate_identifier(app_id, "app_id")?;
    validate_identifier(kind, "package kind")?;
    validate_identifier(version, "version")?;
    validate_identifier(package_id, "package_id")?;
    let path = match kind {
        "runtime" => app_root
            .join(app_id)
            .join("runtime")
            .join(version)
            .join(package_id),
        "data" => app_root
            .join(app_id)
            .join("packages")
            .join(version)
            .join(package_id),
        other => {
            return Err(AppError::InvalidState(format!(
                "unknown package kind: {other}"
            )))
        }
    };
    validate_app_path(app_root, &path)?;
    Ok(path)
}

/// Write staged bytes and return (size, sha256 hex). Written to a temp
/// name then renamed, so a crash never leaves a partial payload that a
/// later stage check could hash.
pub fn write_staged_payload(path: &Path, bytes: &[u8]) -> Result<(u64, String), AppError> {
    atomic_write(path, bytes)?;
    let hash = hex_sha256(bytes);
    Ok((bytes.len() as u64, hash))
}

/// Remove a staging directory tree (rollback / abort). Idempotent.
pub fn remove_staging_dir(dir: &Path) -> Result<(), AppError> {
    if dir.exists() {
        std::fs::remove_dir_all(dir)?;
    }
    Ok(())
}

/// Delete the install-owned trees of an app on uninstall: `runtime/`,
/// `packages/`, `staging/`. Personal data (`data/`, `cache/`, `imports/`)
/// is preserved (ADR-0025 D32).
pub fn remove_app_install_dirs(app_root: &Path, app_id: &str) -> Result<(), AppError> {
    validate_identifier(app_id, "app_id")?;
    let base = app_root.join(app_id);
    for name in ["runtime", "packages", "staging"] {
        validate_app_path(app_root, &base.join(name))?;
    }
    for name in ["runtime", "packages", "staging"] {
        let dir = base.join(name);
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
    }
    Ok(())
}

pub fn hex_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// Constant-length lowercase-hex comparison (declared vs actual).
pub fn sha256_matches(declared: &str, actual: &str) -> bool {
    declared.len() == 64 && declared.eq_ignore_ascii_case(actual)
}

/// Atomically move a staged payload into its final install location
/// (same-volume rename; cross-device fails with an explicit IO error
/// rather than a silent partial copy).
pub fn install_staged_payload(staged: &Path, target: &Path) -> Result<u64, AppError> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::rename(staged, target)?;
    let size = std::fs::metadata(target)?.len();
    Ok(size)
}

/// Mark a runtime binary executable (V1 targets macOS/Linux; no-op on
/// Windows where the extension bit is meaningless for .exe).
pub fn mark_executable(path: &Path) -> Result<(), AppError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = std::fs::metadata(path)?;
        let mut perms = meta.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms)?;
    }
    let _ = path;
    Ok(())
}

/// Active runtime version file: `apps/<app_id>/runtime/current` holds the
/// plain version string. A file (not a symlink) so the switch is a
/// portable atomic rename and the reader needs no OS-specific handling.
pub fn runtime_current_path(app_root: &Path, app_id: &str) -> PathBuf {
    app_root.join(app_id).join("runtime").join("current")
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn read_runtime_current(app_root: &Path, app_id: &str) -> Option<String> {
    let text = std::fs::read_to_string(runtime_current_path(app_root, app_id)).ok()?;
    let version = text.trim().to_string();
    if version.is_empty() {
        None
    } else {
        Some(version)
    }
}

/// Atomically point `current` at `version` (temp file + rename).
pub fn set_runtime_current(app_root: &Path, app_id: &str, version: &str) -> Result<(), AppError> {
    validate_identifier(app_id, "app_id")?;
    validate_identifier(version, "version")?;
    let path = runtime_current_path(app_root, app_id);
    validate_app_path(app_root, &path)?;
    atomic_write(&path, format!("{version}\n").as_bytes())
}

/// Spawn `<binary> --health` and require exit 0 + `{"status":"ok"}` on
/// stdout within 2 s. The health probe and runtime EOF exit are separate checks.
pub fn health_check_binary(path: &Path, version: &str) -> Result<(), AppError> {
    use std::io::Read;
    // macOS serializes inspection of newly created executables. Keep test
    // fixtures from consuming each other's production probe deadline.
    #[cfg(test)]
    static PROBES: std::sync::Mutex<()> = std::sync::Mutex::new(());
    #[cfg(test)]
    let _probe = PROBES
        .lock()
        .map_err(|_| AppError::InvalidState("test probe lock poisoned".into()))?;
    let mut child = std::process::Command::new(path)
        .arg("--health")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(AppError::InvalidState(
                "health probe output unavailable".into(),
            ));
        }
    };
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut out = Vec::new();
        let result = stdout.take(4097).read_to_end(&mut out).map(|_| out);
        let _ = tx.send(result);
    });
    let deadline = Instant::now() + Duration::from_secs(2);
    let result = (|| {
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if Instant::now() >= deadline {
                return Err(AppError::InvalidState("health check timed out".into()));
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        if !status.success() {
            return Err(AppError::InvalidState("health check failed".into()));
        }
        let bytes = rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|_| AppError::InvalidState("health check output timed out".into()))??;
        if bytes.len() > 4096 {
            return Err(AppError::InvalidState(
                "health check output too large".into(),
            ));
        }
        let value: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|_| AppError::InvalidState("invalid health check response".into()))?;
        if value["status"] != "ok" || value["version"] != version {
            return Err(AppError::InvalidState(
                "health check status or version mismatch".into(),
            ));
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = child.kill();
    }
    let _ = child.wait();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "natives-app-install-test-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn identifiers_reject_traversal_and_prefixes() {
        let root = PathBuf::from("/tmp/natives-app-root");
        for bad in [
            "../evil", "", ".", "..", "/Users/x", "C:\\x", "~/x", "a/b", "a\\b",
        ] {
            assert!(
                staging_dir(&root, bad, "uuid-1").is_err(),
                "app_id {bad:?} must be rejected"
            );
        }
        assert!(staging_dir(&root, "com.natives.app.demo", "../evil").is_err());
        assert!(staging_dir(&root, "com.natives.app.demo", "uuid-1").is_ok());
        for bad in ["../x", "", "a/b", "C:\\x"] {
            assert!(staged_payload_path(&root, bad).is_err(), "package {bad:?}");
        }
    }

    #[test]
    fn install_paths_are_core_decided() {
        let root = PathBuf::from("/tmp/natives-app-root");
        let runtime =
            install_path_for(&root, "fund", "runtime", "1.0.0", "fund-host").expect("runtime");
        assert!(runtime
            .to_string_lossy()
            .ends_with("/fund/runtime/1.0.0/fund-host"));
        let data = install_path_for(&root, "fund", "data", "1", "map").expect("data");
        assert!(data.to_string_lossy().ends_with("/fund/packages/1/map"));
        assert!(install_path_for(&root, "fund", "evil", "1", "x").is_err());
        assert!(install_path_for(&root, "../fund", "runtime", "1", "x").is_err());
    }

    #[test]
    fn write_and_hash_payload() {
        let dir = temp_root("hash");
        let staged = dir.join("staging/uuid/one.payload");
        let (size, hash) = write_staged_payload(&staged, b"hello nap").expect("write");
        assert_eq!(size, 9);
        assert_eq!(hash, hex_sha256(b"hello nap"));
        assert_eq!(hash.len(), 64);
        // on-disk bytes match; a single tampered byte does not
        let on_disk = std::fs::read(&staged).expect("read back");
        assert!(sha256_matches(&hex_sha256(&on_disk), &hash));
        assert!(!sha256_matches(&hash, &hex_sha256(b"hello naP")));
        assert!(!sha256_matches("short", &hash));
        remove_staging_dir(&dir).expect("cleanup");
    }

    #[test]
    fn install_move_is_atomic_rename() {
        let dir = temp_root("move");
        let staged = dir.join("staging/uuid/one.payload");
        write_staged_payload(&staged, b"payload-bytes").expect("write");
        let target = dir.join("fund/packages/1/one");
        let size = install_staged_payload(&staged, &target).expect("move");
        assert_eq!(size, 13);
        assert!(!staged.exists(), "source removed after atomic rename");
        assert!(target.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn current_version_switch_is_persisted() {
        let dir = temp_root("current");
        let root = &dir;
        assert_eq!(read_runtime_current(root, "fund"), None);
        set_runtime_current(root, "fund", "1.0.0").expect("set 1.0.0");
        assert_eq!(read_runtime_current(root, "fund").as_deref(), Some("1.0.0"));
        set_runtime_current(root, "fund", "1.1.0").expect("set 1.1.0");
        assert_eq!(read_runtime_current(root, "fund").as_deref(), Some("1.1.0"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn uninstall_dirs_preserve_personal_data() {
        let dir = temp_root("uninstall");
        let app = dir.join("fund");
        for sub in [
            "runtime/1.0.0",
            "packages/1",
            "staging/uuid",
            "data",
            "cache",
            "imports",
        ] {
            std::fs::create_dir_all(app.join(sub)).expect("mkdir");
        }
        std::fs::write(app.join("data/fund.db"), b"personal").expect("db");
        remove_app_install_dirs(&dir, "fund").expect("remove");
        assert!(!app.join("runtime").exists());
        assert!(!app.join("packages").exists());
        assert!(!app.join("staging").exists());
        assert_eq!(
            std::fs::read(app.join("data/fund.db")).expect("data preserved"),
            b"personal"
        );
        assert!(app.join("cache").exists());
        assert!(app.join("imports").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn app_directory_symlinks_cannot_escape_during_install_or_uninstall() {
        let dir = temp_root("symlink-escape");
        let root = dir.join("apps");
        let external = dir.join("external");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(external.join("runtime")).unwrap();
        std::fs::write(external.join("runtime/keep"), b"external data").unwrap();
        std::os::unix::fs::symlink(&external, root.join("fund")).unwrap();
        assert!(staging_dir(&root, "fund", "tx").is_err());
        assert!(install_path_for(&root, "fund", "runtime", "1.0.0", "host").is_err());
        assert!(remove_app_install_dirs(&root, "fund").is_err());
        assert_eq!(
            std::fs::read(external.join("runtime/keep")).unwrap(),
            b"external data"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn current_version_rejects_symlink_parent_and_invalid_app_id() {
        let dir = temp_root("current-escape");
        let external = dir.join("external");
        std::fs::create_dir_all(&external).unwrap();
        std::fs::create_dir_all(dir.join("fund")).unwrap();
        std::os::unix::fs::symlink(&external, dir.join("fund/runtime")).unwrap();
        assert!(set_runtime_current(&dir, "fund", "1.0.0").is_err());
        assert!(set_runtime_current(&dir, "../escaped", "1.0.0").is_err());
        assert!(!external.join("current").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
