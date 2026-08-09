//! # Natives Agent Daemon
//!
//! A local sidecar microservice that manages conversations, runs, tool execution,
//! provider connections, and the agent lifecycle. Communicates with the Tauri client
//! via authenticated Unix Domain Socket (or Named Pipe on Windows).
//!
//! ## Architecture
//!
//! ```text
//! main()
//!   ├── parse env / config
//!   ├── bootstrap from NATIVES_DAEMON_BOOTSTRAP (never print)
//!   ├── start RPC server
//!   │   ├── Unix Domain Socket (macOS/Linux)
//!   │   └── Named Pipe (Windows) — abstraction point
//!   ├── handle connections
//!   │   ├── handshake (bootstrap → session token)
//!   │   ├── authenticate (session token)
//!   │   └── dispatch (versioned RPC methods)
//!   └── graceful shutdown
//! ```

use natives_agent_daemon::rpc::RpcServer;
use std::path::PathBuf;

/// macOS/Linux sockaddr_un path limit (~104 incl. NUL). Prefer short /tmp when too long.
#[cfg(unix)]
fn shorten_socket_if_needed(path: PathBuf) -> PathBuf {
    // Conservative: many BSDs/macOS limit sun_path to 104 bytes.
    const MAX: usize = 100;
    let s = path.to_string_lossy();
    if s.len() <= MAX {
        return path;
    }
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    let short = std::env::temp_dir().join(format!("nuds-{:x}.sock", h.finish()));
    eprintln!(
        "Socket path too long for UDS ({} > {MAX}); using {}",
        s.len(),
        short.display()
    );
    short
}

/// Default socket path for the daemon.
#[cfg(unix)]
fn default_socket_path() -> PathBuf {
    if let Ok(p) = std::env::var("NATIVES_DAEMON_SOCKET") {
        if !p.trim().is_empty() {
            return shorten_socket_if_needed(PathBuf::from(p));
        }
    }
    if let Ok(runtime) = std::env::var("NATIVES_RUNTIME_DIR") {
        return shorten_socket_if_needed(PathBuf::from(runtime).join("natives-agent.sock"));
    }
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            PathBuf::from(home).join(".natives").join("runtime")
        });
    shorten_socket_if_needed(runtime_dir.join("natives-agent.sock"))
}

/// Default socket path for Windows (pipe name).
#[cfg(windows)]
fn default_socket_path() -> PathBuf {
    if let Ok(p) = std::env::var("NATIVES_DAEMON_SOCKET") {
        if !p.trim().is_empty() {
            return PathBuf::from(p);
        }
    }
    PathBuf::from(r"\\.\pipe\natives-agent")
}

/// Daemon configuration.
#[derive(Debug, Clone)]
pub struct DaemonConfig {
    /// Socket path (Unix) or pipe name (Windows).
    pub socket_path: PathBuf,
    /// Bootstrap token for initial handshake.
    pub bootstrap_token: String,
    /// Daemon version string.
    pub version: String,
    /// Protocol version string.
    pub protocol_version: String,
}

impl DaemonConfig {
    fn from_env() -> Self {
        let socket_path = default_socket_path();
        let bootstrap_token = std::env::var("NATIVES_DAEMON_BOOTSTRAP")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(generate_bootstrap_token);
        DaemonConfig {
            socket_path,
            bootstrap_token,
            version: env!("CARGO_PKG_VERSION").to_string(),
            protocol_version: natives_agent_daemon::protocol_version().to_string(),
        }
    }
}

/// Generate a cryptographically random bootstrap token.
fn generate_bootstrap_token() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: Vec<u8> = (0..32).map(|_| rng.gen()).collect();
    hex::encode(bytes)
}

#[tokio::main]
async fn main() {
    let config = DaemonConfig::from_env();
    // Ensure parent dir for socket exists (UDS path length limits still apply on some OS).
    if let Some(parent) = config.socket_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    println!("Natives Agent Daemon v{}", config.version);
    // Credential broker from natives.db when NATIVES_DB_PATH is set (sidecar mode).
    let broker_ok = natives_agent_daemon::try_install_natives_db_broker();
    // The loopback listener remains entirely daemon-owned and polls only its
    // Host-owned configuration; no Renderer can access provider credentials.
    natives_agent_daemon::loopback::spawn_supervisor();

    // Initialize governor from persisted settings if available.
    let mut rl_settings = natives_agent_daemon::governor::EngineRateLimitSettings::default();
    if let Ok(Some(val)) = natives_agent_daemon::natives_db_broker::read_setting(
        natives_agent_daemon::governor::SETTINGS_KEY,
    ) {
        if let Ok(settings) =
            serde_json::from_str::<natives_agent_daemon::governor::EngineRateLimitSettings>(&val)
        {
            if settings.validate().is_ok() {
                rl_settings = settings;
            }
        }
    }
    let governor = std::sync::Arc::new(
        natives_agent_daemon::governor::ProviderRequestGovernor::new(rl_settings),
    );
    let _ = natives_agent_daemon::GLOBAL_GOVERNOR.set(governor);

    // Capability library: trusted configuration source for MCP + skill catalogue (ADR-0016).
    natives_agent_daemon::capability::bootstrap();
    // Idle reaper: stop stdio MCP servers with zero run references for >10min.
    tokio::spawn(async {
        let idle = std::time::Duration::from_secs(600);
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(300));
        loop {
            tick.tick().await;
            let stopped = natives_agent_daemon::mcp_runtime::global_mcp().reap_idle(idle);
            if !stopped.is_empty() {
                println!("[mcp] reaped idle stdio servers: {}", stopped.join(", "));
            }
        }
    });
    // Run snapshot path is restored on RunManager::new(); ensure process-wide manager is warm.
    let _ = natives_agent_daemon::global_run_manager().list_runs(None);
    println!("Protocol: {}", config.protocol_version);
    println!("Socket: {}", config.socket_path.display());
    // NEVER print bootstrap token — Supervisor/scripts pass via env or 0600 file only.
    println!(
        "Bootstrap: {}",
        if std::env::var("NATIVES_DAEMON_BOOTSTRAP")
            .map(|s| !s.is_empty())
            .unwrap_or(false)
        {
            "from NATIVES_DAEMON_BOOTSTRAP"
        } else {
            "generated in-process (ephemeral)"
        }
    );
    println!(
        "Credential broker: {}",
        if broker_ok {
            // W1: do not print the natives.db absolute path (home-path leak).
            "UDS lease broker installed (host decrypts natives.db)".into()
        } else {
            "env-only (NATIVES_TEST_*)".into()
        }
    );

    let socket_path = config.socket_path.clone();
    let server = RpcServer::new(
        socket_path.to_str().unwrap_or("/tmp/natives-agent.sock"),
        &config.bootstrap_token,
        &config.protocol_version,
        &config.version,
    );

    println!("Starting RPC server...");
    let lifeline_enabled = std::env::var("NATIVES_PARENT_LIFELINE")
        .map(|v| v.eq_ignore_ascii_case("stdio"))
        .unwrap_or(false);
    if lifeline_enabled {
        println!("Parent lifeline: stdio (EOF triggers shutdown)");
    }

    tokio::select! {
        result = server.run() => {
            if let Err(e) = result {
                eprintln!("Fatal error: {}", e);
                std::process::exit(1);
            }
        }
        reason = parent_lifeline(lifeline_enabled) => {
            eprintln!("Daemon shutdown requested: {reason}");
            // Cancel all execution roots and wait for quiet (task-03/13).
            let outcomes = natives_agent_daemon::global_run_manager()
                .runtime
                .cancel_all_execution_roots()
                .await;
            if !outcomes.is_empty() {
                eprintln!(
                    "Execution drain finished: {} root outcome(s)",
                    outcomes.len()
                );
            }
            // Best-effort runtime file cleanup. Host force-kills if we hang.
            let _ = std::fs::remove_file(&socket_path);
            let grace_ms = std::env::var("NATIVES_DAEMON_SHUTDOWN_GRACE_MS")
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(500);
            tokio::time::sleep(std::time::Duration::from_millis(grace_ms)).await;
            std::process::exit(0);
        }
    }
}

/// Wait for Host→Daemon inherited stdin EOF when `NATIVES_PARENT_LIFELINE=stdio`.
/// Standalone daemons leave the env unset and never observe terminal stdin.
async fn parent_lifeline(enabled: bool) -> &'static str {
    if !enabled {
        std::future::pending::<()>().await;
        return "unreachable";
    }
    use tokio::io::AsyncReadExt;
    let mut stdin = tokio::io::stdin();
    let mut buf = [0_u8; 64];
    loop {
        match stdin.read(&mut buf).await {
            Ok(0) => return "parent_lifeline_eof",
            Ok(_) => {
                // Host keeps the write end open; ignore any incidental bytes.
            }
            Err(_) => return "parent_lifeline_read_error",
        }
    }
}

#[cfg(test)]
mod parent_lifeline_tests {
    use super::parent_lifeline;

    #[tokio::test]
    async fn disabled_lifeline_is_pending_not_eof() {
        let result =
            tokio::time::timeout(std::time::Duration::from_millis(50), parent_lifeline(false))
                .await;
        assert!(result.is_err(), "disabled lifeline must not resolve");
    }
}
