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
use assistant_protocol::v2::credential::CredentialBrokerSession;
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(test)]
use std::sync::{Mutex, MutexGuard};
use std::sync::{OnceLock, RwLock};

#[derive(Clone)]
struct BrokerPeer {
    pid: u32,
    generation: u64,
    instance_id: String,
    auth_token: String,
}

static BROKER_PEER: OnceLock<RwLock<Option<BrokerPeer>>> = OnceLock::new();
static NEXT_BROKER_PEER_GENERATION: AtomicU64 = AtomicU64::new(1);
#[cfg(test)]
static BROKER_PEER_TEST_LOCK: Mutex<()> = Mutex::new(());

fn broker_peer() -> &'static RwLock<Option<BrokerPeer>> {
    BROKER_PEER.get_or_init(|| RwLock::new(None))
}

/// Install one supervised daemon PID. The generation prevents an old lifecycle
/// from revoking a newer daemon's identity.
pub(crate) fn install_broker_peer(
    pid: u32,
    session: CredentialBrokerSession,
) -> std::result::Result<u64, String> {
    if pid == 0 || session.instance_id.trim().is_empty() || session.auth_token.trim().is_empty() {
        return Err("broker peer identity is incomplete".into());
    }
    let mut peer = broker_peer()
        .write()
        .map_err(|_| "broker peer state unavailable".to_string())?;
    if peer.is_some() {
        return Err("broker peer already installed".into());
    }
    let generation = NEXT_BROKER_PEER_GENERATION.fetch_add(1, Ordering::Relaxed);
    *peer = Some(BrokerPeer {
        pid,
        generation,
        instance_id: session.instance_id,
        auth_token: session.auth_token,
    });
    Ok(generation)
}

#[cfg(test)]
pub(crate) struct BrokerPeerTestGuard {
    _lock: MutexGuard<'static, ()>,
}

#[cfg(test)]
impl Drop for BrokerPeerTestGuard {
    fn drop(&mut self) {
        clear_broker_peer_for_test();
    }
}

#[cfg(test)]
pub(crate) fn lock_broker_peer_for_test() -> BrokerPeerTestGuard {
    let lock = BROKER_PEER_TEST_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    clear_broker_peer_for_test();
    BrokerPeerTestGuard { _lock: lock }
}

#[cfg(test)]
pub(crate) struct BrokerPeerTestLease(u64);

#[cfg(test)]
impl BrokerPeerTestLease {
    pub(crate) fn generation(&self) -> u64 {
        self.0
    }
}

#[cfg(test)]
impl Drop for BrokerPeerTestLease {
    fn drop(&mut self) {
        clear_broker_peer(self.0);
    }
}

#[cfg(test)]
pub(crate) fn install_broker_peer_for_test(
    pid: u32,
) -> std::result::Result<BrokerPeerTestLease, String> {
    install_broker_peer(
        pid,
        CredentialBrokerSession {
            instance_id: "test-instance".into(),
            auth_token: "test-auth".into(),
        },
    )
    .map(BrokerPeerTestLease)
}

/// Revoke only the lifecycle that installed this generation.
pub(crate) fn clear_broker_peer(generation: u64) {
    if let Ok(mut peer) = broker_peer().write() {
        if peer
            .as_ref()
            .is_some_and(|current| current.generation == generation)
        {
            if let Some(mut current) = peer.take() {
                wipe_string(&mut current.auth_token);
            }
        }
    }
}

/// Test cleanup must recover from a prior panic without weakening the
/// generation-scoped production revoke path.
#[cfg(test)]
fn clear_broker_peer_for_test() {
    if let Ok(mut peer) = broker_peer().write() {
        if let Some(mut current) = peer.take() {
            wipe_string(&mut current.auth_token);
        }
    }
}

/// Best-effort safe wipe before release. Equal-length NUL replacement
/// overwrites the String contents without unsafe UTF-8 mutation or an
/// additional secret-bearing buffer.
fn wipe_string(value: &mut String) {
    value.replace_range(.., &"\0".repeat(value.len()));
}

pub(crate) fn broker_peer_matches(pid: u32) -> bool {
    let Ok(peer) = broker_peer().read() else {
        return false;
    };
    peer.as_ref().is_some_and(|expected| expected.pid == pid)
}

pub(crate) fn broker_peer_authorizes(pid: u32, instance_id: &str, auth_token: &str) -> bool {
    let Ok(peer) = broker_peer().read() else {
        return false;
    };
    let Some(expected) = peer.as_ref() else {
        return false;
    };
    expected.pid == pid
        && constant_time_eq(expected.instance_id.as_bytes(), instance_id.as_bytes())
        && constant_time_eq(expected.auth_token.as_bytes(), auth_token.as_bytes())
}

fn constant_time_eq(expected: &[u8], actual: &[u8]) -> bool {
    let length_diff = expected.len() ^ actual.len();
    let mut diff = length_diff;
    for index in 0..expected.len().max(actual.len()) {
        diff |= usize::from(
            expected.get(index).copied().unwrap_or(0) ^ actual.get(index).copied().unwrap_or(0),
        );
    }
    diff == 0
}

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

/// Serve one broker request: peer identity check, bounded one-line read, dispatch,
/// one-line reply. Never panics on I/O errors.
pub(super) fn handle_broker_connection(
    stream: std::os::unix::net::UnixStream,
) -> std::result::Result<(), String> {
    use std::io::{BufRead, BufReader, Read, Write};

    // Same-UID and supervised-PID checks happen before any blocking read.
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
    let peer_pid = broker_peer_pid(&stream)?;
    if !broker_peer_matches(peer_pid) {
        return Err("broker peer PID mismatch (rejected before read)".into());
    }

    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(30)));
    let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(30)));

    let mut reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
    // Bounded read: a single envelope line must fit within a small frame.
    const MAX_FRAME_BYTES: u64 = 16 * 1024;
    let mut frame = Vec::new();
    let read = reader
        .by_ref()
        .take(MAX_FRAME_BYTES + 1)
        .read_until(b'\n', &mut frame)
        .map_err(|e| super::redact_broker_error(&format!("broker read failed: {e}")))?;
    if read == 0 || frame.iter().all(u8::is_ascii_whitespace) {
        return Err("broker empty request".into());
    }
    if frame.len() as u64 > MAX_FRAME_BYTES {
        return Err("broker request frame too large".into());
    }
    let line =
        std::str::from_utf8(&frame).map_err(|_| "broker request must be UTF-8".to_string())?;
    let reply =
        super::dispatch_broker_uds(&line, peer_pid).map_err(|e| super::redact_broker_error(&e))?;
    let mut writer = stream;
    writer
        .write_all(reply.as_bytes())
        .map_err(|e| super::redact_broker_error(&format!("broker write failed: {e}")))?;
    let _ = writer.flush();
    Ok(())
}

#[cfg(target_os = "macos")]
fn broker_peer_pid(stream: &std::os::unix::net::UnixStream) -> std::result::Result<u32, String> {
    use std::os::unix::io::AsRawFd;

    let mut pid: libc::pid_t = 0;
    let mut length = std::mem::size_of::<libc::pid_t>() as libc::socklen_t;
    // SAFETY: the socket fd and pid out-buffer are valid for this getsockopt call.
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_LOCAL,
            libc::LOCAL_PEEREPID,
            &mut pid as *mut libc::pid_t as *mut libc::c_void,
            &mut length,
        )
    };
    if result != 0 || length != std::mem::size_of::<libc::pid_t>() as libc::socklen_t || pid <= 0 {
        return Err("broker peer PID unavailable".into());
    }
    Ok(pid as u32)
}

#[cfg(not(target_os = "macos"))]
fn broker_peer_pid(_stream: &std::os::unix::net::UnixStream) -> std::result::Result<u32, String> {
    Err("broker peer PID verification unsupported".into())
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
