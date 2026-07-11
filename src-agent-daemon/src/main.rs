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
//!   ├── parse args / config
//!   ├── generate bootstrap token
//!   ├── start RPC server
//!   │   ├── Unix Domain Socket (macOS/Linux)
//!   │   └── Named Pipe (Windows) — abstraction point
//!   ├── handle connections
//!   │   ├── handshake (bootstrap → session token)
//!   │   ├── authenticate (session token)
//!   │   └── dispatch (versioned RPC methods)
//!   └── graceful shutdown
//! ```

mod rpc;

use rpc::RpcServer;
use std::path::PathBuf;

/// Default socket path for the daemon.
#[cfg(unix)]
fn default_socket_path() -> PathBuf {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let tmp = std::env::temp_dir();
            tmp.join("natives")
        });
    runtime_dir.join("natives-agent.sock")
}

/// Default socket path for Windows (pipe name).
#[cfg(windows)]
fn default_socket_path() -> PathBuf {
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

impl Default for DaemonConfig {
    fn default() -> Self {
        let bootstrap_token = generate_bootstrap_token();
        DaemonConfig {
            socket_path: default_socket_path(),
            bootstrap_token,
            version: env!("CARGO_PKG_VERSION").to_string(),
            protocol_version: "0.1.0".to_string(),
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
    let config = DaemonConfig::default();
    println!("Natives Agent Daemon v{}", config.version);
    println!("Protocol: {}", config.protocol_version);
    println!("Socket: {}", config.socket_path.display());
    println!("Bootstrap token: {}", config.bootstrap_token);

    // Create RPC server and start listening
    let server = RpcServer::new(
        config.socket_path.to_str().unwrap_or("/tmp/natives-agent.sock"),
        &config.bootstrap_token,
        &config.protocol_version,
        &config.version,
    );

    println!("Starting RPC server...");
    if let Err(e) = server.run().await {
        eprintln!("Fatal error: {}", e);
        std::process::exit(1);
    }
}