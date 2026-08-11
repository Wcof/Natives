//! Host-side Credential Broker UDS listener (W3 P0-04).
//!
//! Binds a private mode-0600 socket at `NATIVES_BROKER_SOCKET` (or the runtime
//! directory default), accepts one connection at a time, reads one JSON line
//! ([`assistant_protocol::v2::credential::CredentialLeaseEnvelope`]),
//! dispatches it through `super::dispatch_broker_uds`, and writes back one
//! JSON line ([`assistant_protocol::v2::credential::CredentialLeaseReply`]).
//! Same-UID check via `getpeereid` on macOS / BSD (libc) when available.
//! Errors are redacted; no token/secret ever logs. The daemon never falls back
//! to reading natives.db when the broker is absent.

use crate::error::{Error, Result};

/// Host-side Credential Broker UDS listener (W3 P0-04).
///
/// Binds a private mode-0600 socket at `NATIVES_BROKER_SOCKET` (or the runtime
/// directory default), accepts one connection at a time, reads one JSON line
/// ([`wire::CredentialLeaseEnvelope`]), dispatches it through
/// [`super::dispatch_broker_uds`], and writes back one JSON line
/// ([`wire::CredentialLeaseReply`]). Same-UID check via `getpeereid` on macOS /
/// BSD (libc) when available. Errors are redacted; no token/secret ever logs.
/// The daemon never falls back to reading natives.db when the broker is absent.
pub fn spawn_broker_uds_listener(socket_path: &std::path::Path) -> Result<()> {
    use std::os::unix::net::UnixListener;

    if let Some(parent) = socket_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| Error::Internal(format!("broker socket dir: {e}")))?;
    }
    let _ = std::fs::remove_file(socket_path);
    let listener = UnixListener::bind(socket_path)
        .map_err(|e| Error::Internal(format!("broker bind {}: {e}", socket_path.display())))?;

    // Private socket: only the same user may connect (mode 0600).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(socket_path, std::fs::Permissions::from_mode(0o600));
    }

    std::thread::spawn(move || {
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    let _ = handle_broker_connection(stream);
                }
                Err(e) => {
                    eprintln!("[credential_broker] accept failed (redacted): {e}");
                }
            }
        }
    });
    Ok(())
}

/// Serve one broker request: peer-UID check, bounded one-line read, dispatch,
/// one-line reply. Never panics on I/O errors.
fn handle_broker_connection(
    stream: std::os::unix::net::UnixStream,
) -> std::result::Result<(), String> {
    use std::io::{BufRead, BufReader, Write};

    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(30)));
    let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(30)));

    // Same-UID check before any credential parse/store access.
    #[cfg(any(target_os = "macos", target_os = "freebsd", target_os = "openbsd"))]
    {
        use std::os::unix::io::AsRawFd;
        let mut peer_uid: libc::uid_t = 0;
        let mut peer_gid: libc::gid_t = 0;
        // SAFETY: getpeereid writes into valid out-params owned by this frame.
        let rc = unsafe { libc::getpeereid(stream.as_raw_fd(), &mut peer_uid, &mut peer_gid) };
        if rc != 0 || peer_uid != unsafe { libc::geteuid() } {
            return Err("broker peer UID mismatch (rejected before parse)".into());
        }
    }

    let mut reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
    let mut line = String::new();
    // Bounded read: a single envelope line must fit within a small frame.
    let read = reader
        .read_line(&mut line)
        .map_err(|e| super::redact_broker_error(&format!("broker read failed: {e}")))?;
    if read == 0 || line.trim().is_empty() {
        return Err("broker empty request".into());
    }
    if line.len() > 16 * 1024 {
        return Err("broker request frame too large".into());
    }
    let reply = super::dispatch_broker_uds(&line).map_err(|e| super::redact_broker_error(&e))?;
    let mut writer = stream;
    writer
        .write_all(reply.as_bytes())
        .map_err(|e| super::redact_broker_error(&format!("broker write failed: {e}")))?;
    let _ = writer.flush();
    Ok(())
}

/// Resolve the broker socket path (mirrors the daemon-side default).
pub fn broker_socket_path() -> std::result::Result<std::path::PathBuf, String> {
    if let Ok(p) = std::env::var("NATIVES_BROKER_SOCKET") {
        if !p.trim().is_empty() {
            return Ok(std::path::PathBuf::from(p));
        }
    }
    let runtime_dir = std::env::var_os("NATIVES_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from))
        .unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join(".natives")
                .join("runtime")
        });
    Ok(runtime_dir.join("natives-broker.sock"))
}
