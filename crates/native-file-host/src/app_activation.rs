//! Commit-time activation for managed_local apps (ADR-0027; contract §2/§5):
//! runtimeHost naming, executable bit, bounded `--health` probe and Native
//! Host registration (macOS/Linux file, Windows HKCU). All paths are
//! Core-decided; the catalog and the page never supply disk paths, host
//! names or launch arguments.

use crate::app_store::types::AppError;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Health probe budget (contract §5 step 4): bounded, five seconds, always
/// reaped.
const HEALTH_TIMEOUT: Duration = Duration::from_secs(5);

/// runtimeHost is computed by Core (contract §2): `com.natives.app.` plus
/// the full lowercase SHA-256 hex of the app_id. Persisted on the receipt;
/// never guessed by the page or overridden by the catalog.
pub fn runtime_host_name(app_id: &str) -> String {
    let digest = Sha256::digest(app_id.as_bytes());
    let mut hex = String::with_capacity(64);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    format!("com.natives.app.{hex}")
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
/// the candidate; the child is always reaped.
pub fn health_probe(executable: &Path) -> Result<(), AppError> {
    let mut command = Command::new(executable);
    command
        .arg("--health")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|error| {
        AppError::InvalidState(format!("APP_HEALTH_CHECK_FAILED: spawn: {error}"))
    })?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if status.success() {
                    return Ok(());
                }
                return Err(AppError::InvalidState(
                    "APP_HEALTH_CHECK_FAILED: candidate exited non-zero".into(),
                ));
            }
            Ok(None) if started.elapsed() > HEALTH_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(AppError::InvalidState(
                    "APP_HEALTH_CHECK_FAILED: health probe timed out".into(),
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(AppError::InvalidState(format!(
                    "APP_HEALTH_CHECK_FAILED: {error}"
                )));
            }
        }
    }
}

/// Register the per-app Native Host so the page can `connectNative` the
/// managed executable (contract §5 step 4 of the app.html flow). macOS and
/// Linux use the Chrome JSON manifest file; Windows uses HKCU. The manifest
/// exposes ONLY this app's executable to the caller's extension origin.
pub fn register_runtime_host(
    app_id: &str,
    executable: &Path,
    origin: &str,
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
        windows_hkcu_register(&host, executable)?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        let dir = crate::app_host_manifest::chrome_manifest_dir().ok_or_else(|| {
            AppError::InvalidState("APP_REGISTER_FAILED: Chrome manifest dir unknown".into())
        })?;
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{host}.json"));
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
pub fn unregister_runtime_host(app_id: &str) -> Result<(), AppError> {
    let host = runtime_host_name(app_id);
    #[cfg(target_os = "windows")]
    {
        windows_hkcu_unregister(&host)?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        if let Some(dir) = crate::app_host_manifest::chrome_manifest_dir() {
            let path = dir.join(format!("{host}.json"));
            match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_host_name_is_stable_sha256() {
        let a = runtime_host_name("fund");
        let b = runtime_host_name("fund");
        assert_eq!(a, b);
        assert!(a.starts_with("com.natives.app."));
        assert_eq!(a.len(), "com.natives.app.".len() + 64);
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
}
