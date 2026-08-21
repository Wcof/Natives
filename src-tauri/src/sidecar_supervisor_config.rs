use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

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
    pub daemon_bin: PathBuf,
    pub natives_db_path: PathBuf,
    /// Daemon conversation/run authority DB (Phase 0 assistant.db).
    pub assistant_db_path: PathBuf,
    /// When true, missing UDS is Faulted (never Embedded).
    pub require_uds: bool,
    pub health_timeout: Duration,
    /// How long Host waits after closing lifeline before force-killing the Daemon.
    pub shutdown_grace: Duration,
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
        let assistant_db_path = std::env::var("NATIVES_ASSISTANT_DB_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                dirs_home()
                    .map(|h| h.join(".natives").join("assistant.db"))
                    .unwrap_or_else(|| PathBuf::from("assistant.db"))
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
            runtime_dir,
            socket_path,
            daemon_bin,
            natives_db_path,
            assistant_db_path,
            require_uds,
            health_timeout: Duration::from_secs(15),
            shutdown_grace: Duration::from_millis(
                std::env::var("NATIVES_DAEMON_SHUTDOWN_GRACE_MS")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(2_000),
            ),
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
