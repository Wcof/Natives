//! 用户级 Natives 服务模式与 Native Relay 基础设施（ADR-0042）。

pub mod app_runtime;
pub mod auth;
pub mod dispatch;
mod events;
mod files;
mod http_helpers;
pub mod launcher;
pub mod model_host;
mod model_worker;
mod native_lease;
pub mod relay;
mod relay_events;
mod rpc;
mod runtime_worker;
pub mod server;
pub mod static_files;
mod surface_requests;
mod surface_routes;

use crate::app_store::AppStore;
use crate::workspace_store::{default_db_path, WorkspaceStore};
use app_runtime_core::lock::FileLock;
use app_runtime_core::session::{base64url, random_bytes};
use serde_json::{json, Value};
use std::fs;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{self, Command};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

pub const DEFAULT_PROD_PORT: u16 = 8770;
pub const DEFAULT_DEV_PORT: u16 = 8771;

pub fn default_port() -> u16 {
    if let Ok(p) = std::env::var("NATIVES_WEB_PORT") {
        if let Some(port) = p.parse::<u16>().ok().filter(|port| *port != 0) {
            return port;
        }
    }
    if cfg!(debug_assertions) {
        DEFAULT_DEV_PORT
    } else {
        DEFAULT_PROD_PORT
    }
}

pub fn state_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("NATIVES_STATE_DIR") {
        return PathBuf::from(dir);
    }
    let dir_name = if cfg!(debug_assertions) {
        ".natives-local"
    } else {
        ".natives"
    };
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
    home.join(dir_name)
}

pub fn lock_file_path() -> PathBuf {
    state_dir().join("native-file-host.lock")
}

pub fn relay_secret_path() -> PathBuf {
    state_dir().join("relay.secret")
}

pub fn read_relay_secret() -> io::Result<String> {
    fs::read_to_string(relay_secret_path()).map(|s| s.trim().to_string())
}

pub fn write_relay_secret(secret: &str) -> io::Result<()> {
    let path = relay_secret_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    file.set_len(0)?;
    file.write_all(secret.as_bytes())?;
    file.sync_all()?;
    Ok(())
}

pub struct LockGuard {
    _file_lock: FileLock,
    pub path: PathBuf,
}

pub fn acquire_lock(port: u16, epoch: &str) -> io::Result<Option<LockGuard>> {
    let lock_path = lock_file_path();
    if let Some(parent) = lock_path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    match FileLock::try_acquire(lock_path.clone())? {
        Some(file_lock) => {
            let payload = json!({
                "pid": process::id(),
                "port": port,
                "epoch": epoch,
                "startedAt": std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0)
            });
            let _ = fs::write(&lock_path, payload.to_string());
            Ok(Some(LockGuard {
                _file_lock: file_lock,
                path: lock_path,
            }))
        }
        None => {
            if relay::check_service_health(port) {
                Ok(None)
            } else {
                eprintln!("natives-file-host: lock held by another process but port {port} is not responding");
                Ok(None)
            }
        }
    }
}

pub fn request_pairing_code(port: u16, secret: &str) -> io::Result<String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port))?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;

    write!(
        stream,
        "POST /web/session/pair-code HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Natives-Relay-Secret: {secret}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    )?;
    stream.flush()?;

    let mut resp_bytes = Vec::new();
    stream.read_to_end(&mut resp_bytes)?;

    let text = String::from_utf8_lossy(&resp_bytes);
    let body = text.split("\r\n\r\n").nth(1).unwrap_or("");
    let parsed: Value = serde_json::from_str(body)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;

    parsed
        .get("pairingCode")
        .and_then(Value::as_str)
        .map(String::from)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "missing pairingCode in response",
            )
        })
}

pub fn run_service(port: Option<u16>, bootstrap: Option<String>) -> io::Result<()> {
    let port = port.unwrap_or_else(default_port);
    let mut epoch_buf = [0u8; 16];
    if !random_bytes(&mut epoch_buf) {
        return Err(io::Error::other("OS randomness unavailable"));
    }
    let epoch = base64url(&epoch_buf);

    let Some(_lock) = acquire_lock(port, &epoch)? else {
        eprintln!("natives-file-host: service already running on 127.0.0.1:{port}");
        return Ok(());
    };

    let mut secret_buf = [0u8; 32];
    if !random_bytes(&mut secret_buf) {
        return Err(io::Error::other("OS randomness unavailable"));
    }
    let relay_secret = base64url(&secret_buf);
    write_relay_secret(&relay_secret)?;

    let listener = TcpListener::bind(("127.0.0.1", port))?;
    let shutdown = Arc::new(AtomicBool::new(false));

    let db_path = std::env::var("NATIVES_WORKSPACE_DB")
        .map(PathBuf::from)
        .unwrap_or_else(|_| default_db_path());

    let ws_store = WorkspaceStore::open(&db_path).map_err(io::Error::other)?;
    let app_store = match std::env::var_os("NATIVES_APPS_ROOT") {
        Some(root) => AppStore::open_at(&db_path, PathBuf::from(root)),
        None => AppStore::open(&db_path),
    }
    .map_err(io::Error::other)?;
    let origin = app_store.trusted_extension_origin().ok();
    app_store.set_caller_origin(origin.as_deref());

    let state = Arc::new(server::ServiceState::new(
        port,
        epoch,
        relay_secret,
        bootstrap,
        Arc::clone(&shutdown),
        Some(ws_store),
        Some(app_store),
    ));

    eprintln!("natives-file-host: service ready on http://127.0.0.1:{port}");
    server::run_service_listener(listener, state, shutdown);
    Ok(())
}

pub fn ensure_service_and_run_relay(port: Option<u16>) -> io::Result<()> {
    relay::run_relay_loop(ensure_service(port)?)
}

pub fn ensure_service(port: Option<u16>) -> io::Result<u16> {
    let port = port.unwrap_or_else(default_port);
    if !relay::check_service_health(port) {
        if let Ok(exe) = std::env::current_exe() {
            let mut child = Command::new(exe)
                .arg("--service")
                .arg("--port")
                .arg(port.to_string())
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()?;
            thread::spawn(move || {
                let _ = child.wait();
            });

            for _ in 0..40 {
                thread::sleep(Duration::from_millis(50));
                if relay::check_service_health(port) {
                    break;
                }
            }
        }
    }

    if relay::check_service_health(port) {
        Ok(port)
    } else {
        Err(io::Error::new(
            io::ErrorKind::ConnectionRefused,
            format!("could not start or connect to native-file-host service on port {port}"),
        ))
    }
}

#[cfg(test)]
mod tests;
