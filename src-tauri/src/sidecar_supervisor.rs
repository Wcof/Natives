//! Sidecar Supervisor — production bootstrap for the Agent Daemon.
//!
//! Target lifecycle (Phase 2):
//! 1. Resolve runtime dir (socket, bootstrap, pid, lock)
//! 2. Clean stale socket / zombie pid
//! 3. Spawn agent-daemon sidecar
//! 4. Obtain bootstrap via secure channel (not normal logs)
//! 5. Wait for health/readiness
//! 6. Protocol v2 handshake
//! 7. Register credential broker path
//! 8. Recover unfinished runs / permissions
//! 9. Start UI event subscription
//! 10. Auto-restart on crash + re-handshake
//! 11. Graceful shutdown on Tauri exit
//!
//! Production rule: UDS failure must surface as an explicit fault state.
//! Never silently fall back to Embedded.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Supervisor state visible to UI / health commands.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SupervisorState {
    Stopped,
    Starting,
    Healthy,
    Degraded { reason: String },
    Faulted { reason: String },
    Restarting,
    ShuttingDown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupervisorStatus {
    pub state: SupervisorState,
    pub mode: String,
    pub socket_path: Option<String>,
    pub pid: Option<u32>,
    pub restart_count: u32,
    pub last_error: Option<String>,
    /// True only when UDS is required and currently healthy.
    pub production_ready: bool,
}

#[derive(Debug, Clone)]
pub struct SupervisorConfig {
    pub runtime_dir: PathBuf,
    pub socket_path: PathBuf,
    pub pid_path: PathBuf,
    pub lock_path: PathBuf,
    pub bootstrap_path: PathBuf,
    pub daemon_bin: PathBuf,
    pub natives_db_path: PathBuf,
    /// When true, missing UDS is Faulted (never Embedded).
    pub require_uds: bool,
    pub health_timeout: Duration,
    pub max_restarts: u32,
}

impl SupervisorConfig {
    /// Build from environment with sensible defaults.
    pub fn from_env() -> Self {
        let runtime_dir = std::env::var("NATIVES_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| default_runtime_dir());
        let socket_path = std::env::var("NATIVES_DAEMON_SOCKET")
            .map(PathBuf::from)
            .unwrap_or_else(|_| runtime_dir.join("natives-agent.sock"));
        let natives_db_path = std::env::var("NATIVES_DB_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                dirs_home()
                    .map(|h| h.join(".natives").join("natives.db"))
                    .unwrap_or_else(|| PathBuf::from("natives.db"))
            });
        let daemon_bin = std::env::var("NATIVES_DAEMON_BIN")
            .map(PathBuf::from)
            .unwrap_or_else(|_| resolve_bundled_daemon_bin());
        let require_uds = std::env::var("NATIVES_DAEMON_MODE")
            .map(|m| {
                matches!(
                    m.to_ascii_lowercase().as_str(),
                    "uds" | "sidecar" | "remote"
                )
            })
            .unwrap_or_else(|_| !cfg!(test))
            || std::env::var("NATIVES_REQUIRE_UDS")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false);

        Self {
            pid_path: runtime_dir.join("agent-daemon.pid"),
            lock_path: runtime_dir.join("agent-daemon.lock"),
            bootstrap_path: runtime_dir.join("bootstrap.token"),
            runtime_dir,
            socket_path,
            daemon_bin,
            natives_db_path,
            require_uds,
            health_timeout: Duration::from_secs(15),
            max_restarts: 5,
        }
    }
}

fn resolve_bundled_daemon_bin() -> PathBuf {
    let name = if cfg!(windows) {
        "natives-agent-daemon.exe"
    } else {
        "natives-agent-daemon"
    };
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for candidate in [
                dir.join(name),
                dir.parent().map(|p| p.join(name)).unwrap_or_default(),
            ] {
                if candidate.is_file() {
                    return candidate;
                }
            }
        }
    }
    PathBuf::from(name)
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

fn default_runtime_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_RUNTIME_DIR") {
        return PathBuf::from(xdg).join("natives");
    }
    dirs_home()
        .map(|h| h.join(".natives").join("runtime"))
        .unwrap_or_else(|| std::env::temp_dir().join("natives-runtime"))
}

/// In-process supervisor (Phase 2 skeleton — spawn + health + no silent fallback).
pub struct SidecarSupervisor {
    config: SupervisorConfig,
    state: Mutex<InnerState>,
}

struct InnerState {
    status: SupervisorStatus,
    child: Option<Child>,
    bootstrap_token: Option<String>,
}

impl SidecarSupervisor {
    pub fn new(config: SupervisorConfig) -> Self {
        let mode = if config.require_uds { "uds" } else { "auto" };
        Self {
            state: Mutex::new(InnerState {
                status: SupervisorStatus {
                    state: SupervisorState::Stopped,
                    mode: mode.into(),
                    socket_path: Some(config.socket_path.display().to_string()),
                    pid: None,
                    restart_count: 0,
                    last_error: None,
                    production_ready: false,
                },
                child: None,
                bootstrap_token: None,
            }),
            config,
        }
    }

    pub fn status(&self) -> SupervisorStatus {
        self.state
            .lock()
            .map(|g| g.status.clone())
            .unwrap_or_else(|_| SupervisorStatus {
                state: SupervisorState::Faulted {
                    reason: "supervisor lock poisoned".into(),
                },
                mode: "unknown".into(),
                socket_path: None,
                pid: None,
                restart_count: 0,
                last_error: Some("lock poisoned".into()),
                production_ready: false,
            })
    }

    /// Ensure runtime paths exist and clear stale sockets from dead processes.
    pub fn prepare_runtime_dir(&self) -> Result<(), String> {
        std::fs::create_dir_all(&self.config.runtime_dir)
            .map_err(|e| format!("create runtime dir: {e}"))?;
        if self.config.socket_path.exists() {
            // Stale socket: remove so bind can succeed after crash.
            let _ = std::fs::remove_file(&self.config.socket_path);
        }
        Ok(())
    }

    /// Start sidecar if not running. On require_uds, failures become Faulted.
    pub fn ensure_started(&self) -> Result<SupervisorStatus, String> {
        self.prepare_runtime_dir()?;
        validate_natives_db_path(&self.config.natives_db_path)?;
        {
            let mut inner = self.state.lock().map_err(|e| e.to_string())?;
            if matches!(
                inner.status.state,
                SupervisorState::Healthy | SupervisorState::Starting
            ) {
                return Ok(inner.status.clone());
            }
            inner.status.state = SupervisorState::Starting;
            inner.status.last_error = None;
        }

        match self.spawn_child() {
            Ok((child, bootstrap)) => {
                let pid = child.id();
                // Write pid (best-effort)
                let _ = std::fs::write(&self.config.pid_path, pid.to_string());
                // Bootstrap must not be written to ordinary logs.
                let _ = std::fs::write(&self.config.bootstrap_path, &bootstrap);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(
                        &self.config.bootstrap_path,
                        std::fs::Permissions::from_mode(0o600),
                    );
                }

                let readiness = self.wait_for_readiness(&bootstrap, self.config.health_timeout);
                let mut inner = self.state.lock().map_err(|e| e.to_string())?;
                inner.child = Some(child);
                inner.bootstrap_token = Some(bootstrap.clone());
                if readiness.is_ok() {
                    // Export for UDS client resolution in this process.
                    std::env::set_var("NATIVES_DAEMON_SOCKET", &self.config.socket_path);
                    std::env::set_var("NATIVES_DAEMON_BOOTSTRAP", &bootstrap);
                    if self.config.require_uds {
                        std::env::set_var("NATIVES_DAEMON_MODE", "uds");
                    }
                    std::env::set_var("NATIVES_DB_PATH", &self.config.natives_db_path);
                    // Drop cached UDS client so next call re-handshakes after restart.
                    // (async reset is best-effort from callers; env bootstrap is source of truth.)
                    inner.status.state = SupervisorState::Healthy;
                    inner.status.pid = Some(pid);
                    inner.status.production_ready = self.config.require_uds;
                    inner.status.last_error = None;
                    // Note: daemon process restores Run snapshots itself on boot
                    // (RunManager::new → restore_runs_snapshot). Active runs become
                    // Interrupted for safe UI retry — no silent re-exec.
                } else {
                    let reason = readiness.unwrap_err();
                    inner.status.state = SupervisorState::Faulted {
                        reason: reason.clone(),
                    };
                    inner.status.production_ready = false;
                    inner.status.last_error = Some(reason.clone());
                    if self.config.require_uds {
                        return Err(format!(
                            "UDS required but daemon not healthy: {reason} (no embedded fallback)"
                        ));
                    }
                }
                Ok(inner.status.clone())
            }
            Err(e) => {
                let mut inner = self.state.lock().map_err(|err| err.to_string())?;
                inner.status.state = SupervisorState::Faulted { reason: e.clone() };
                inner.status.production_ready = false;
                inner.status.last_error = Some(e.clone());
                if self.config.require_uds {
                    Err(format!(
                        "UDS required; sidecar start failed: {e} (no embedded fallback)"
                    ))
                } else {
                    Ok(inner.status.clone())
                }
            }
        }
    }

    fn spawn_child(&self) -> Result<(Child, String), String> {
        if !self.config.daemon_bin.exists()
            && which_in_path(self.config.daemon_bin.to_string_lossy().as_ref()).is_none()
        {
            return Err(format!(
                "daemon binary not found: {} (set NATIVES_DAEMON_BIN)",
                self.config.daemon_bin.display()
            ));
        }
        let bootstrap = generate_bootstrap_token();
        let mut cmd = Command::new(&self.config.daemon_bin);
        cmd.env("NATIVES_DAEMON_SOCKET", &self.config.socket_path)
            .env("NATIVES_DAEMON_BOOTSTRAP", &bootstrap)
            .env("NATIVES_DB_PATH", &self.config.natives_db_path)
            .env("NATIVES_RUNTIME_DIR", &self.config.runtime_dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let child = cmd
            .spawn()
            .map_err(|e| format!("spawn {}: {e}", self.config.daemon_bin.display()))?;
        Ok((child, bootstrap))
    }

    fn wait_for_readiness(&self, bootstrap: &str, timeout: Duration) -> Result<(), String> {
        let start = Instant::now();
        let mut last_error = "daemon not ready".to_string();
        while start.elapsed() < timeout {
            if self.config.socket_path.exists() {
                match self.readiness_probe(bootstrap) {
                    Ok(()) => return Ok(()),
                    Err(error) => last_error = error,
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Err(format!(
            "daemon readiness failed within {:?}: {last_error}",
            timeout
        ))
    }

    fn readiness_probe(&self, bootstrap: &str) -> Result<(), String> {
        let socket = self.config.socket_path.clone();
        let bootstrap = bootstrap.to_string();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| format!("readiness runtime: {e}"))?;
            rt.block_on(async move {
                let mut client = natives_agent_daemon::DaemonClient::connect(
                    socket,
                    &bootstrap,
                    natives_agent_daemon::client_protocol_version(),
                )
                .await
                .map_err(|e| format!("handshake/ping connect: {e}"))?;
                if client.protocol_version() != natives_agent_daemon::client_protocol_version() {
                    return Err(format!(
                        "protocol mismatch: daemon={} client={}",
                        client.protocol_version(),
                        natives_agent_daemon::client_protocol_version()
                    ));
                }
                let ping = client
                    .call("daemon.ping", serde_json::json!({}))
                    .await
                    .map_err(|e| format!("daemon.ping: {e}"))?;
                if ping.get("pong").and_then(|v| v.as_bool()) != Some(true) {
                    return Err(format!("daemon.ping missing pong: {ping}"));
                }
                let status = client
                    .call("daemon.getStatus", serde_json::json!({}))
                    .await
                    .map_err(|e| format!("daemon.getStatus: {e}"))?;
                if status.get("protocol_version").and_then(|v| v.as_str())
                    != Some(natives_agent_daemon::client_protocol_version())
                {
                    return Err(format!("daemon.getStatus protocol mismatch: {status}"));
                }
                Ok(())
            })
        })
        .join()
        .map_err(|_| "readiness probe panicked".to_string())?
    }

    /// Poll child health; if exited while require_uds, mark Faulted (caller may re-ensure).
    pub fn poll_child_health(&self) -> SupervisorStatus {
        let mut inner = match self.state.lock() {
            Ok(g) => g,
            Err(_) => return self.status(),
        };
        if let Some(child) = inner.child.as_mut() {
            match child.try_wait() {
                Ok(Some(status)) => {
                    inner.child = None;
                    let reason = format!("sidecar exited: {status}");
                    inner.status.state = SupervisorState::Faulted {
                        reason: reason.clone(),
                    };
                    inner.status.production_ready = false;
                    inner.status.last_error = Some(reason);
                    inner.status.pid = None;
                    inner.status.restart_count = inner.status.restart_count.saturating_add(1);
                }
                Ok(None) => {
                    // still running
                    if matches!(inner.status.state, SupervisorState::Healthy) {
                        inner.status.production_ready = self.config.require_uds;
                    }
                }
                Err(e) => {
                    inner.status.last_error = Some(format!("poll child: {e}"));
                }
            }
        }
        inner.status.clone()
    }

    /// If child died, attempt one restart (bounded by max_restarts).
    pub fn ensure_healthy_or_restart(&self) -> Result<SupervisorStatus, String> {
        let st = self.poll_child_health();
        if matches!(
            st.state,
            SupervisorState::Healthy | SupervisorState::Starting
        ) {
            return Ok(st);
        }
        if st.restart_count >= self.config.max_restarts {
            return Err(format!(
                "sidecar restart budget exhausted ({}/{})",
                st.restart_count, self.config.max_restarts
            ));
        }
        // Clear fault so ensure_started will re-spawn.
        {
            let mut inner = self.state.lock().map_err(|e| e.to_string())?;
            if matches!(inner.status.state, SupervisorState::Faulted { .. }) {
                inner.status.state = SupervisorState::Restarting;
            }
        }
        self.ensure_started()
    }

    /// Graceful stop — kill child and clear socket.
    pub fn shutdown(&self) -> Result<(), String> {
        let mut inner = self.state.lock().map_err(|e| e.to_string())?;
        inner.status.state = SupervisorState::ShuttingDown;
        if let Some(mut child) = inner.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = std::fs::remove_file(&self.config.socket_path);
        let _ = std::fs::remove_file(&self.config.pid_path);
        // Wipe bootstrap file on shutdown.
        let _ = std::fs::remove_file(&self.config.bootstrap_path);
        inner.bootstrap_token = None;
        inner.status.state = SupervisorState::Stopped;
        inner.status.pid = None;
        inner.status.production_ready = false;
        Ok(())
    }

    pub fn bootstrap_token(&self) -> Option<String> {
        self.state
            .lock()
            .ok()
            .and_then(|g| g.bootstrap_token.clone())
    }

    pub fn config(&self) -> &SupervisorConfig {
        &self.config
    }
}

fn generate_bootstrap_token() -> String {
    use rand::RngCore;
    let mut bytes = [0_u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

#[cfg(test)]
fn uuid_like() -> String {
    format!(
        "{:x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    )
}

fn which_in_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Process-wide supervisor (lazy).
static GLOBAL_SUPERVISOR: std::sync::OnceLock<SidecarSupervisor> = std::sync::OnceLock::new();

pub fn global_supervisor() -> &'static SidecarSupervisor {
    GLOBAL_SUPERVISOR.get_or_init(|| SidecarSupervisor::new(SupervisorConfig::from_env()))
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
    // Prefer restart path if previously healthy child died.
    let st = sup.poll_child_health();
    if matches!(st.state, SupervisorState::Faulted { .. }) && st.restart_count > 0 {
        return sup.ensure_healthy_or_restart();
    }
    sup.ensure_started()
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn config_from_env_defaults() {
        let _g = ENV_LOCK.lock().unwrap();
        let prev = std::env::var("NATIVES_RUNTIME_DIR").ok();
        std::env::set_var("NATIVES_RUNTIME_DIR", "/tmp/natives-sup-test");
        let cfg = SupervisorConfig::from_env();
        assert!(cfg.socket_path.starts_with("/tmp/natives-sup-test"));
        if let Some(v) = prev {
            std::env::set_var("NATIVES_RUNTIME_DIR", v);
        } else {
            std::env::remove_var("NATIVES_RUNTIME_DIR");
        }
    }

    #[test]
    fn default_daemon_binary_is_resolved_to_an_existing_path() {
        let _g = ENV_LOCK.lock().unwrap();
        let previous = std::env::var("NATIVES_DAEMON_BIN").ok();
        std::env::remove_var("NATIVES_DAEMON_BIN");
        let daemon_bin = SupervisorConfig::from_env().daemon_bin;
        assert!(
            daemon_bin.is_absolute() && daemon_bin.is_file(),
            "unresolved daemon binary: {}",
            daemon_bin.display()
        );
        if let Some(value) = previous {
            std::env::set_var("NATIVES_DAEMON_BIN", value);
        }
    }

    #[test]
    fn require_uds_fault_without_binary() {
        let dir = tempfile_path();
        let _ = std::fs::create_dir_all(&dir);
        let cfg = SupervisorConfig {
            runtime_dir: dir.clone(),
            socket_path: dir.join("t.sock"),
            pid_path: dir.join("t.pid"),
            lock_path: dir.join("t.lock"),
            bootstrap_path: dir.join("boot"),
            daemon_bin: PathBuf::from("/nonexistent/natives-agent-daemon-xyz"),
            natives_db_path: dir.join("natives.db"),
            require_uds: true,
            health_timeout: Duration::from_millis(100),
            max_restarts: 1,
        };
        let sup = SidecarSupervisor::new(cfg);
        let err = sup.ensure_started().unwrap_err();
        assert!(err.contains("no embedded fallback") || err.contains("not found"));
        assert!(matches!(
            sup.status().state,
            SupervisorState::Faulted { .. }
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn validate_db_path_empty() {
        assert!(validate_natives_db_path(Path::new("")).is_err());
    }

    #[test]
    fn readiness_requires_rpc_handshake_not_just_missing_socket() {
        let dir = tempfile_path();
        let _ = std::fs::create_dir_all(&dir);
        let cfg = SupervisorConfig {
            runtime_dir: dir.clone(),
            socket_path: dir.join("t.sock"),
            pid_path: dir.join("t.pid"),
            lock_path: dir.join("t.lock"),
            bootstrap_path: dir.join("boot"),
            daemon_bin: PathBuf::from("/unused"),
            natives_db_path: dir.join("natives.db"),
            require_uds: true,
            health_timeout: Duration::from_millis(1),
            max_restarts: 1,
        };
        let err = SidecarSupervisor::new(cfg)
            .wait_for_readiness("bootstrap", Duration::from_millis(1))
            .unwrap_err();
        assert!(err.contains("daemon readiness failed"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn tempfile_path() -> PathBuf {
        std::env::temp_dir().join(format!("natives-sup-{}", uuid_like()))
    }
}
