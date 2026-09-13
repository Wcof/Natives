//! Commit-time activation for managed_local apps (ADR-0027; contract §2/§5):
//! runtimeHost naming, executable bit, bounded `--health` probe and Native
//! Host registration (macOS/Linux file, Windows HKCU). All paths are
//! Core-decided; the catalog and the page never supply disk paths, host
//! names or launch arguments.

use crate::app_store::types::AppError;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Health probe budget (contract §5 step 4): bounded, five seconds, always
/// reaped. `--inspect-data` (contract §4.1) shares the same wall-clock budget.
const HEALTH_TIMEOUT: Duration = Duration::from_secs(5);
/// Captured-output cap for bounded modes. `--inspect-data` must stay under
/// 64 KiB (contract §4.1); `--health` output is not consumed but still capped.
const BOUNDED_OUTPUT_CAP: usize = 64 * 1024;

/// Drain a child pipe to EOF on its own thread, keeping at most `cap` bytes.
/// Reading always continues past the cap (bytes beyond it are discarded) so a
/// chatty child can never fill the pipe buffer and deadlock the parent while it
/// waits on the wall-clock budget.
fn drain_capped<R: Read + Send + 'static>(
    mut reader: R,
    cap: usize,
) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut kept = Vec::new();
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if kept.len() < cap {
                        let take = n.min(cap - kept.len());
                        kept.extend_from_slice(&buf[..take]);
                    }
                }
            }
        }
        kept
    })
}

/// Run a bounded child (contract §4.1/§5 step 4): stdin closed, stdout/stderr
/// drained concurrently, a hard wall-clock budget, capped captured stdout, and
/// the child always reaped. Returns the exit status and the captured stdout
/// (up to `stdout_cap`). The error string is caller-prefixed with the mode.
pub(crate) fn run_bounded(
    mut command: Command,
    timeout: Duration,
    stdout_cap: usize,
) -> Result<(std::process::ExitStatus, Vec<u8>), String> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|error| format!("spawn: {error}"))?;
    // Drain both pipes on their own threads before waiting, so neither can
    // back-pressure the child into a hang while we hold the budget.
    let out_handle = child
        .stdout
        .take()
        .map(|pipe| drain_capped(pipe, stdout_cap));
    let err_handle = child.stderr.take().map(|pipe| drain_capped(pipe, 0));
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                if let Some(handle) = out_handle {
                    let _ = handle.join();
                }
                if let Some(handle) = err_handle {
                    let _ = handle.join();
                }
                return Err(format!("timed out after {:?}", started.elapsed()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                if let Some(handle) = out_handle {
                    let _ = handle.join();
                }
                if let Some(handle) = err_handle {
                    let _ = handle.join();
                }
                return Err(format!("{error}"));
            }
        }
    };
    let stdout = out_handle
        .map(|handle| handle.join().unwrap_or_default())
        .unwrap_or_default();
    if let Some(handle) = err_handle {
        let _ = handle.join();
    }
    Ok((status, stdout))
}

/// runtimeHost is computed by Core (contract §2): the contract-fixed prefix
/// `com.natives.app.a` plus the full lowercase SHA-256 hex of the app_id.
/// The `a` guards against app_ids beginning with a hyphen colliding with
/// Native Messaging host-name rules. Persisted on the receipt; never guessed
/// by the page or overridden by the catalog.
pub fn runtime_host_name(app_id: &str) -> String {
    let digest = Sha256::digest(app_id.as_bytes());
    let mut hex = String::with_capacity(64);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    format!("com.natives.app.a{hex}")
}

/// Executable payload lands with the exec bit; group/other keep read only.
pub fn make_executable(path: &Path) -> Result<(), AppError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms)?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

/// Bounded `--health` probe (contract §5 step 4/5): package integrity and
/// protocol surface only — no runtime lock, no business start, no production
/// DB/Keychain/network. Non-zero exit, timeout or failure to spawn rejects
/// the candidate; the child is always reaped and its pipes drained.
pub fn health_probe(executable: &Path) -> Result<(), AppError> {
    let mut command = Command::new(executable);
    command.arg("--health");
    let (status, _stdout) = run_bounded(command, HEALTH_TIMEOUT, BOUNDED_OUTPUT_CAP)
        .map_err(|error| AppError::InvalidState(format!("APP_HEALTH_CHECK_FAILED: {error}")))?;
    if !status.success() {
        return Err(AppError::InvalidState(format!(
            "APP_HEALTH_CHECK_FAILED: candidate exited non-zero: {status:?}"
        )));
    }
    Ok(())
}

/// Run the current, signed, installed binary's read-only `--inspect-data`
/// (contract §4.1): Core-executed while holding the install→runtime lock,
/// bounded to five seconds and 64 KiB, no migration/network/Keychain. Returns
/// the raw stdout for strict deserialization by the caller. Any failure to
/// spawn, a timeout, or a non-zero exit is surfaced as APP_INSPECT_UNAVAILABLE
/// so the rollback gate can default-deny rather than trust an absent answer.
pub fn run_inspect_data(executable: &Path, apps_root: &Path) -> Result<Vec<u8>, AppError> {
    let mut command = Command::new(executable);
    command
        .arg("--inspect-data")
        .env("NATIVES_APPS_ROOT", apps_root);
    let (status, stdout) = run_bounded(command, HEALTH_TIMEOUT, BOUNDED_OUTPUT_CAP)
        .map_err(|error| AppError::InvalidState(format!("APP_INSPECT_UNAVAILABLE: {error}")))?;
    if !status.success() {
        return Err(AppError::InvalidState(format!(
            "APP_INSPECT_UNAVAILABLE: candidate exited non-zero: {status:?}"
        )));
    }
    Ok(stdout)
}

/// Register the per-app Native Host so the page can `connectNative` the
/// managed executable (contract §5 step 4 of the app.html flow). macOS and
/// Linux use the Chrome JSON manifest file; Windows uses HKCU. The manifest
/// exposes ONLY this app's executable to the caller's extension origin.
pub fn register_runtime_host(
    app_id: &str,
    executable: &Path,
    origin: &str,
    manifest_dir: &Path,
) -> Result<String, AppError> {
    let host = runtime_host_name(app_id);
    let manifest = serde_json::json!({
        "name": host,
        "description": format!("Natives managed app {app_id}"),
        "path": executable.to_string_lossy(),
        "type": "stdio",
        "allowed_origins": [origin],
    });
    #[cfg(target_os = "windows")]
    {
        let _ = manifest_dir;
        windows_hkcu_register(&host, executable)?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::fs::create_dir_all(manifest_dir)?;
        let path = crate::app_host_manifest::manifest_path_in(manifest_dir, &host)?;
        // Same atomic-write discipline as the projection.
        let body = serde_json::to_vec_pretty(&manifest)
            .map_err(|_| AppError::InvalidState("manifest serialization failed".into()))?;
        let temp = path.with_extension("json.tmp");
        std::fs::write(&temp, &body)?;
        std::fs::rename(&temp, &path)?;
    }
    Ok(host)
}

/// Remove a per-app Native Host registration created by
/// [`register_runtime_host`]. Missing registration is not an error.
pub fn unregister_runtime_host(app_id: &str, manifest_dir: &Path) -> Result<(), AppError> {
    let host = runtime_host_name(app_id);
    #[cfg(target_os = "windows")]
    {
        let _ = manifest_dir;
        windows_hkcu_unregister(&host)?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        let path = crate::app_host_manifest::manifest_path_in(manifest_dir, &host)?;
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn windows_hkcu_register(host: &str, executable: &Path) -> Result<(), AppError> {
    use std::os::windows::process::CommandExt;
    let script = format!(
        "New-Item -Path 'HKCU:\\Software\\Google\\Chrome\\NativeMessagingHosts\\{host}' -Force | Out-Null; \
         Set-ItemProperty -Path 'HKCU:\\Software\\Google\\Chrome\\NativeMessagingHosts\\{host}' -Name '(Default)' -Value '{}'",
        executable.display()
    );
    let status = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(0x08000000)
        .status()
        .map_err(|error| AppError::InvalidState(format!("APP_REGISTER_FAILED: {error}")))?;
    if !status.success() {
        return Err(AppError::InvalidState(
            "APP_REGISTER_FAILED: HKCU registration rejected".into(),
        ));
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn windows_hkcu_unregister(host: &str) -> Result<(), AppError> {
    use std::os::windows::process::CommandExt;
    let script = format!(
        "Remove-Item -Path 'HKCU:\\Software\\Google\\Chrome\\NativeMessagingHosts\\{host}' -ErrorAction SilentlyContinue"
    );
    let _ = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(0x08000000)
        .status();
    Ok(())
}

/// Standard path for an app's activation.json projection (contract §3.1):
/// `~/.natives/apps/<appId>/activation.json`.
pub fn activation_path(apps_root: &Path, app_id: &str) -> PathBuf {
    apps_root.join(app_id).join("activation.json")
}

/// Atomically write activation.json projection (contract §3.1).
/// Permissions: 0700 for parent, 0600 for file; temp + fsync + rename.
pub fn write_activation_projection(
    apps_root: &Path,
    app_id: &str,
    projection: &serde_json::Value,
) -> Result<(), AppError> {
    let path = activation_path(apps_root, app_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(parent)?.permissions();
            perms.set_mode(0o700);
            let _ = std::fs::set_permissions(parent, perms);
        }
    }
    let body = serde_json::to_vec_pretty(projection)
        .map_err(|_| AppError::InvalidState("activation projection serialization failed".into()))?;
    let temp = path.with_extension("json.tmp");
    {
        use std::io::Write;
        let mut file = std::fs::File::create(&temp)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = file.metadata()?.permissions();
            perms.set_mode(0o600);
            let _ = file.set_permissions(perms);
        }
        file.write_all(&body)?;
        file.sync_all()?;
    }
    std::fs::rename(&temp, &path)?;
    Ok(())
}

/// Read activation.json projection if it exists.
pub fn read_activation_projection(
    apps_root: &Path,
    app_id: &str,
) -> Result<Option<serde_json::Value>, AppError> {
    let path = activation_path(apps_root, app_id);
    if !path.exists() {
        return Ok(None);
    }
    let bytes = std::fs::read(&path)?;
    let val: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::InvalidState(format!("invalid activation.json: {e}")))?;
    Ok(Some(val))
}

/// Next monotonic activation generation (contract §5.1 step 2): the current
/// projection's `generation` plus one; a missing projection or field starts at
/// 1. Core bumps this on every enable/disable/upgrade/rollback/recover so the
/// page/Host `activationGeneration` never resets or reuses a value. This is a
/// separate counter from the App Store install `revision`.
pub fn next_activation_generation(apps_root: &Path, app_id: &str) -> u64 {
    match read_activation_projection(apps_root, app_id) {
        Ok(Some(value)) => value
            .get("generation")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
            .saturating_add(1),
        _ => 1,
    }
}

/// Atomically update the `enabled` field and `activationState` in activation.json.
/// The activation generation is bumped monotonically on every change (contract
/// §5.1 step 2). A missing projection (data-only apps have none) is a no-op.
pub fn update_activation_enabled(
    apps_root: &Path,
    app_id: &str,
    enabled: bool,
) -> Result<(), AppError> {
    if let Some(mut val) = read_activation_projection(apps_root, app_id)? {
        let next_generation = val
            .get("generation")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
            .saturating_add(1);
        val["generation"] = serde_json::json!(next_generation);
        val["enabled"] = serde_json::Value::Bool(enabled);
        val["activationState"] = if enabled {
            serde_json::json!("ready")
        } else {
            serde_json::json!("disabled")
        };
        write_activation_projection(apps_root, app_id, &val)?;
    }
    Ok(())
}

/// Remove activation.json on uninstall / purge.
pub fn remove_activation_projection(apps_root: &Path, app_id: &str) -> Result<(), AppError> {
    let path = activation_path(apps_root, app_id);
    if path.exists() {
        let _ = std::fs::remove_file(path);
    }
    Ok(())
}

/// Find the installed runtime binary under `apps_root/<appId>/runtime/<version>/`.
pub fn find_runtime_binary(apps_root: &Path, app_id: &str, version: &str) -> Option<PathBuf> {
    let dir = apps_root.join(app_id).join("runtime").join(version);
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                return Some(path);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_host_name_is_stable_sha256() {
        let a = runtime_host_name("fund");
        let b = runtime_host_name("fund");
        assert_eq!(a, b);
        assert!(a.starts_with("com.natives.app.a"));
        assert_eq!(a.len(), "com.natives.app.a".len() + 64);
        // Different ids never collide in the visible prefix space.
        assert_ne!(a, runtime_host_name("other"));
    }

    #[test]
    fn health_probe_rejects_nonzero_and_missing() {
        // Missing binary must fail without hanging.
        assert!(health_probe(Path::new("/nonexistent/health-probe")).is_err());
        // /bin/false exits non-zero.
        if Path::new("/bin/false").exists() {
            assert!(health_probe(Path::new("/bin/false")).is_err());
        }
        // /bin/true exits zero within budget.
        if Path::new("/bin/true").exists() {
            assert!(health_probe(Path::new("/bin/true")).is_ok());
        }
    }

    #[test]
    fn activation_projection_lifecycle() {
        let temp_dir =
            std::env::temp_dir().join(format!("natives-act-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let app_id = "sample";
        let initial = serde_json::json!({
            "receiptVersion": 1,
            "appId": app_id,
            "runtimeHost": "com.natives.app.test",
            "activeVersion": "1.0.0",
            "generation": 1,
            "activationState": "ready",
            "enabled": true,
            "appProtocolVersion": 1,
            "allowedOrigins": ["chrome-extension://abcdefghijklmnop"],
        });

        // Write
        write_activation_projection(&temp_dir, app_id, &initial).expect("write projection");
        let read = read_activation_projection(&temp_dir, app_id)
            .expect("read projection")
            .expect("present");
        assert_eq!(read["enabled"], true);
        assert_eq!(read["activationState"], "ready");
        assert_eq!(read["generation"], 1);
        assert_eq!(next_activation_generation(&temp_dir, app_id), 2);

        // Update to disabled: generation must advance monotonically.
        update_activation_enabled(&temp_dir, app_id, false).expect("update disabled");
        let disabled_read = read_activation_projection(&temp_dir, app_id)
            .expect("read disabled")
            .expect("present");
        assert_eq!(disabled_read["enabled"], false);
        assert_eq!(disabled_read["activationState"], "disabled");
        assert_eq!(disabled_read["generation"], 2);

        // Update to enabled: never resets or reuses a generation.
        update_activation_enabled(&temp_dir, app_id, true).expect("update enabled");
        let enabled_read = read_activation_projection(&temp_dir, app_id)
            .expect("read enabled")
            .expect("present");
        assert_eq!(enabled_read["enabled"], true);
        assert_eq!(enabled_read["activationState"], "ready");
        assert_eq!(enabled_read["generation"], 3);
        assert_eq!(next_activation_generation(&temp_dir, app_id), 4);

        // Remove
        remove_activation_projection(&temp_dir, app_id).expect("remove projection");
        assert!(read_activation_projection(&temp_dir, app_id)
            .expect("read removed")
            .is_none());
        // A missing projection is a no-op for the enabled update and starts at 1.
        update_activation_enabled(&temp_dir, app_id, true).expect("no-op update");
        assert_eq!(next_activation_generation(&temp_dir, app_id), 1);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[cfg(unix)]
    #[test]
    fn run_bounded_drains_oversized_output_without_deadlock() {
        // 100 KiB of output against a 64 KiB cap: the drain keeps reading past
        // the cap so the child never blocks, and only the cap is retained.
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "dd if=/dev/zero bs=1024 count=100 2>/dev/null"]);
        let (status, stdout) =
            run_bounded(command, Duration::from_secs(5), BOUNDED_OUTPUT_CAP).expect("bounded run");
        assert!(status.success());
        assert_eq!(stdout.len(), BOUNDED_OUTPUT_CAP);
    }

    #[cfg(unix)]
    #[test]
    fn run_bounded_kills_over_budget_child() {
        // A child that outlives the budget is killed and reaped promptly.
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "sleep 30"]);
        let started = Instant::now();
        let result = run_bounded(command, Duration::from_millis(200), BOUNDED_OUTPUT_CAP);
        assert!(result.is_err());
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
