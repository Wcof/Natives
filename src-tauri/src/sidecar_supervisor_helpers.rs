use crate::sidecar_supervisor::{global_supervisor, SupervisorStatus};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Child;

pub(crate) fn force_kill_child_tree(child: &mut Child) -> Result<(), String> {
    #[cfg(unix)]
    {
        let pid = child.id() as i32;
        // Negative pid: signal the process group started via setpgid in spawn.
        let rc = unsafe { libc::kill(-pid, libc::SIGKILL) };
        if rc != 0 {
            child.kill().map_err(|e| format!("kill child {pid}: {e}"))?;
        }
        let _ = child.try_wait();
        Ok(())
    }
    #[cfg(windows)]
    {
        let pid = child.id();
        let status = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .status()
            .map_err(|e| format!("taskkill spawn failed: {e}"))?;
        if !status.success() {
            // Fallback to direct kill if taskkill fails.
            let _ = child.kill();
        }
        let _ = child.wait();
        Ok(())
    }
    #[cfg(not(any(unix, windows)))]
    {
        child.kill().map_err(|e| format!("kill child: {e}"))?;
        let _ = child.wait();
        Ok(())
    }
}

pub(crate) fn generate_bootstrap_token() -> String {
    use rand::RngCore;
    let mut bytes = [0_u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

pub(crate) fn write_broker_session(
    child: &mut Child,
    session: &assistant_protocol::v2::credential::CredentialBrokerSession,
) -> Result<(), String> {
    let line = serde_json::to_string(session)
        .map_err(|_| "broker session bootstrap serialize failed".to_string())?;
    let stdin = child
        .stdin
        .as_mut()
        .ok_or_else(|| "broker session lifeline unavailable".to_string())?;
    stdin
        .write_all(line.as_bytes())
        .and_then(|_| stdin.write_all(b"\n"))
        .and_then(|_| stdin.flush())
        .map_err(|e| format!("broker session bootstrap write failed: {e}"))
}

#[cfg(test)]
pub(crate) fn uuid_like() -> String {
    format!(
        "{:x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    )
}

pub(crate) fn which_in_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// UI / ops: current supervisor health (never leaks bootstrap token).
#[tauri::command]
pub fn daemon_supervisor_status() -> SupervisorStatus {
    global_supervisor().status()
}

/// Ensure sidecar is running when production requires UDS.
/// Returns status; on require_uds failure returns Err with explicit fault text.
#[tauri::command]
pub fn daemon_supervisor_ensure() -> Result<SupervisorStatus, String> {
    let sup = global_supervisor();
    match sup.status().state {
        crate::sidecar_supervisor::SupervisorState::Stopped => sup.ensure_started(),
        crate::sidecar_supervisor::SupervisorState::Faulted { .. }
        | crate::sidecar_supervisor::SupervisorState::Degraded { .. } => {
            sup.ensure_healthy_or_restart()
        }
        _ => Ok(sup.status()),
    }
}

/// Poll sidecar liveness (no spawn). UI can show Faulted without auto-restart.
#[tauri::command]
pub fn daemon_supervisor_poll() -> SupervisorStatus {
    global_supervisor().poll_child_health()
}

/// Graceful sidecar stop (dev / shutdown path).
#[tauri::command]
pub fn daemon_supervisor_shutdown() -> Result<(), String> {
    global_supervisor().shutdown()
}

/// Validate NATIVES_DB_PATH exists and is a readable file (schema checks later).
pub fn validate_natives_db_path(path: &Path) -> Result<(), String> {
    if path.as_os_str().is_empty() {
        return Err("NATIVES_DB_PATH is empty".into());
    }
    if !path.is_absolute() {
        // Absolute required by remediation plan; relative allowed only for tests.
        if std::env::var("NATIVES_ALLOW_RELATIVE_DB")
            .map(|v| v == "1")
            .unwrap_or(false)
        {
            // ok
        } else if !path.exists() {
            return Err(format!(
                "NATIVES_DB_PATH must be absolute or existing: {}",
                path.display()
            ));
        }
    }
    if path.exists() {
        let meta = std::fs::metadata(path).map_err(|e| format!("stat db: {e}"))?;
        if !meta.is_file() {
            return Err(format!("NATIVES_DB_PATH is not a file: {}", path.display()));
        }
    }
    Ok(())
}
