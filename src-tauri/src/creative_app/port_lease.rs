//! Port lease registry (batch 7 CR-703, T09 TOCTOU fix).
//!
//! Operation-scoped short leases for host ports. A lease HOLDS the port at the
//! OS level (a live `TcpListener` bound to `127.0.0.1:{port}`) from `acquire`
//! until the caller proves the child/compose bound it. This closes the TOCTOU
//! window the old `bind(:0)` → drop → later `spawn` had: while the lease is
//! held, no other process can bind the port.
//!
//! The caller drops the hold (`release_hold`) at the last moment before
//! spawning the child, then confirms the bind (`confirm_bound`) once the child
//! proves it listens — only then is the reservation actually released.

use crate::{Error, Result};
use std::collections::HashMap;
use std::net::TcpListener;
use std::sync::Arc;
use std::sync::Mutex;

/// Default lease TTL (seconds). A lease that outlives its operation is a leak.
const DEFAULT_LEASE_TTL_SECS: i64 = 300;

struct LeaseEntry {
    _port: u16,
    op_key: String,
    expires_at: i64,
}

/// Port lease registry — thread-safe.
///
/// The registry records which ports are reserved by in-flight start
/// operations. The actual OS-level reservation lives in the returned
/// [`PortLease`] guard, which holds a bound `TcpListener` until `release_hold`
/// (right before the child binds) and a registry entry until `confirm_bound`
/// (the child proved it is listening).
pub struct PortLeaseRegistry {
    inner: Mutex<LeaseState>,
}

struct LeaseState {
    leases: HashMap<u16, LeaseEntry>,
}

impl PortLeaseRegistry {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(LeaseState {
                leases: HashMap::new(),
            }),
        }
    }

    /// Acquire a lease on the given port for an operation.
    ///
    /// The port is reserved twice: the registry records the reservation and a
    /// live `TcpListener` holds the port at the OS level so no other process
    /// (Natives or external) can grab it between now and the child's bind.
    /// Fails when the port is already leased by a different operation or is
    /// actually in use.
    ///
    /// Takes `self: &Arc<Self>` so the returned lease can share the registry
    /// handle and remove its entry on drop / confirm.
    pub fn acquire(self: &Arc<Self>, port: u16, op_key: &str) -> Result<PortLease> {
        let mut st = self.inner.lock().unwrap_or_else(|e| e.into_inner());

        // Expire stale leases.
        st.leases.retain(|_, e| e.expires_at > unix_now());

        if let Some(existing) = st.leases.get(&port) {
            return Err(Error::InvalidInput(format!(
                "port {port} already leased by operation {}",
                existing.op_key
            )));
        }

        // Bind and KEEP the listener: while it is alive no other process can
        // bind 127.0.0.1:{port} (verified on Linux/macOS/BSD).
        let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|_| {
            Error::InvalidInput(format!("port {port} is in use by another process"))
        })?;
        let expires = unix_now() + DEFAULT_LEASE_TTL_SECS;
        st.leases.insert(
            port,
            LeaseEntry {
                _port: port,
                op_key: op_key.to_string(),
                expires_at: expires,
            },
        );
        Ok(PortLease {
            port,
            listener: Some(listener),
            op_key: op_key.to_string(),
            confirmed: false,
            registry: self.clone(),
        })
    }

    /// Acquire a lease on an OS-selected free port (bind `127.0.0.1:0`).
    ///
    /// The returned port is the REAL bound address of a live listener, so the
    /// random port is derived from an actual bind — never a guessed value.
    pub fn acquire_auto(self: &Arc<Self>, op_key: &str) -> Result<PortLease> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .map_err(|e| Error::InvalidInput(format!("could not allocate a free port: {e}")))?;
        let port = listener
            .local_addr()
            .map_err(|e| Error::InvalidInput(format!("could not read allocated port: {e}")))?
            .port();
        let mut st = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        st.leases.retain(|_, e| e.expires_at > unix_now());
        if let Some(existing) = st.leases.get(&port) {
            // Kernel just handed us this port, but never silently share a
            // reserved port.
            return Err(Error::InvalidInput(format!(
                "port {port} already leased by operation {}",
                existing.op_key
            )));
        }
        let expires = unix_now() + DEFAULT_LEASE_TTL_SECS;
        st.leases.insert(
            port,
            LeaseEntry {
                _port: port,
                op_key: op_key.to_string(),
                expires_at: expires,
            },
        );
        Ok(PortLease {
            port,
            listener: Some(listener),
            op_key: op_key.to_string(),
            confirmed: false,
            registry: self.clone(),
        })
    }

    /// Check whether a port is currently reserved by an in-flight lease.
    pub fn is_leased(&self, port: u16) -> bool {
        let st = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        st.leases.contains_key(&port)
    }

    /// Number of active leases.
    pub fn len(&self) -> usize {
        let st = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        st.leases.len()
    }

    /// Whether there are no active leases.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for PortLeaseRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Shared registry handle.
pub type PortLeaseRegistryHandle = Arc<PortLeaseRegistry>;

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// A held port reservation (T09). While the guard lives:
/// - the registry records the reservation (`is_leased`),
/// - a live `TcpListener` holds the port at the OS level.
///
/// Drop without `confirm_bound` releases the reservation. `confirm_bound` must
/// be called once the child/compose proved it is bound.
pub struct PortLease {
    port: u16,
    listener: Option<TcpListener>,
    op_key: String,
    confirmed: bool,
    registry: Arc<PortLeaseRegistry>,
}

impl PortLease {
    /// The reserved port.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// True while the OS-level hold (bound listener) is still alive.
    pub fn is_held(&self) -> bool {
        self.listener.is_some()
    }

    /// Drop the OS-level hold right before spawning the child so the child can
    /// bind the port. The registry reservation persists until `confirm_bound`
    /// (or drop) so no second Natives start claims the port during the
    /// spawn→bind window.
    pub fn release_hold(&mut self) {
        self.listener = None;
    }

    /// Confirm the child/compose proved it bound the port, releasing the
    /// reservation (the child's own socket is the durable protection from now
    /// on).
    pub fn confirm_bound(mut self) {
        self.confirmed = true;
        self.remove_entry();
    }

    fn remove_entry(&self) {
        let mut st = self
            .registry
            .inner
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(key) = st.leases.get(&self.port) {
            if key.op_key == self.op_key {
                st.leases.remove(&self.port);
            }
        }
    }
}

impl Drop for PortLease {
    fn drop(&mut self) {
        if !self.confirmed {
            self.remove_entry();
        }
    }
}

impl std::fmt::Debug for PortLease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PortLease")
            .field("port", &self.port)
            .field("op_key", &self.op_key)
            .field("held", &self.listener.is_some())
            .field("confirmed", &self.confirmed)
            .finish()
    }
}

// ── Resource snapshot (CR-703: on-demand, graceful degradation) ──────

/// A point-in-time resource snapshot of the host process.
/// On platforms where the data is unavailable, fields are None (degraded).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResourceSnapshot {
    /// CPU usage fraction (0.0–1.0) if available.
    pub cpu_usage: Option<f64>,
    /// Resident set size in bytes if available.
    pub rss_bytes: Option<u64>,
    /// Number of threads in the host process.
    pub threads: Option<u32>,
}

impl ResourceSnapshot {
    /// Take a snapshot. Graceful degradation: on error or unsupported platform,
    /// returns a snapshot with None fields rather than failing.
    pub fn take() -> Self {
        let threads = std::thread::available_parallelism()
            .map(|p| p.get() as u32)
            .ok();
        // RSS is platform-specific; on macOS read from task info if possible.
        #[cfg(target_os = "macos")]
        let rss_bytes = macos_rss_bytes();
        #[cfg(not(target_os = "macos"))]
        let rss_bytes = None;

        Self {
            cpu_usage: None, // CPU sampling requires a baseline; leave None here
            rss_bytes,
            threads,
        }
    }
}

#[cfg(target_os = "macos")]
fn macos_rss_bytes() -> Option<u64> {
    use std::mem::size_of;

    // mach_task_basic_info via libc
    let mut info = unsafe { std::mem::zeroed::<mach_task_basic_info>() };
    let mut count = (size_of::<mach_task_basic_info>() / size_of::<natural_t>()) as u32;
    let result = unsafe {
        task_info(
            mach_task_self(),
            MACH_TASK_BASIC_INFO,
            &mut info as *mut _ as *mut i32,
            &mut count,
        )
    };
    if result == KERN_SUCCESS {
        Some(info.resident_size)
    } else {
        None
    }
}

// FFI declarations for mach_task_basic_info (macOS only)
#[cfg(target_os = "macos")]
#[repr(C)]
struct mach_task_basic_info {
    virtual_size: u64,
    resident_size: u64,
    resident_size_max: u64,
    user_time: mach_time_value_t,
    system_time: mach_time_value_t,
    policy: i32,
    suspend_count: i32,
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy)]
struct mach_time_value_t {
    seconds: i32,
    microseconds: i32,
}

#[cfg(target_os = "macos")]
#[allow(non_camel_case_types)] // 与 macOS 系统类型名保持一致
type natural_t = u32;
#[cfg(target_os = "macos")]
#[allow(non_camel_case_types)] // 与 macOS 系统类型名保持一致
type mach_port_t = u32;
#[cfg(target_os = "macos")]
const MACH_TASK_BASIC_INFO: u32 = 20;
#[cfg(target_os = "macos")]
const KERN_SUCCESS: i32 = 0;

#[cfg(target_os = "macos")]
#[link(name = "System", kind = "framework")]
unsafe extern "C" {
    fn mach_task_self() -> mach_port_t;
    fn task_info(task: mach_port_t, flavor: u32, info: *mut i32, count: *mut u32) -> i32;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reg() -> PortLeaseRegistryHandle {
        Arc::new(PortLeaseRegistry::new())
    }

    #[test]
    fn acquire_holds_port_at_os_level() {
        let registry = reg();
        let lease = registry.acquire_auto("op-1").unwrap();
        let port = lease.port();
        assert!(lease.is_held());
        assert!(registry.is_leased(port));
        assert_eq!(registry.len(), 1);

        // While the lease holds the listener, no other bind on 127.0.0.1:port
        // can succeed — the TOCTOU window is closed at the OS level.
        assert!(
            TcpListener::bind(("127.0.0.1", port)).is_err(),
            "a live lease must hold the port at the OS level"
        );

        // A second acquire for the same port is rejected by the registry too.
        assert!(registry.acquire(port, "op-2").is_err());

        // Release the hold; the port is bindable again.
        let mut lease = lease;
        lease.release_hold();
        assert!(!lease.is_held());
        assert!(TcpListener::bind(("127.0.0.1", port)).is_ok());
    }

    #[test]
    fn confirm_bound_releases_reservation() {
        let registry = reg();
        let lease = registry.acquire_auto("op-1").unwrap();
        let port = lease.port();
        assert!(registry.is_leased(port));

        lease.confirm_bound();
        assert!(!registry.is_leased(port), "confirmed lease is released");
        assert_eq!(registry.len(), 0);
    }

    #[test]
    fn drop_without_confirm_releases_reservation() {
        let registry = reg();
        {
            let lease = registry.acquire_auto("op-1").unwrap();
            assert_eq!(registry.len(), 1);
            drop(lease);
        }
        assert_eq!(
            registry.len(),
            0,
            "a dropped, unconfirmed lease must release its reservation"
        );
    }

    #[test]
    fn acquire_rejects_in_use_port() {
        let registry = reg();
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(registry.acquire(port, "op-1").is_err());
    }

    #[test]
    fn fixed_acquire_holds_listener() {
        let registry = reg();
        let probe = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);

        let lease = registry.acquire(port, "op-1").unwrap();
        assert_eq!(lease.port(), port);
        assert!(lease.is_held());
        assert!(registry.is_leased(port));
        lease.confirm_bound();
        assert!(!registry.is_leased(port));
    }

    #[test]
    fn concurrent_acquire_conflicts() {
        let registry = reg();
        let lease = registry.acquire_auto("op-1").unwrap();
        let port = lease.port();
        assert!(registry.acquire(port, "op-2").is_err());
    }

    #[test]
    fn resource_snapshot_degrades_gracefully() {
        let snap = ResourceSnapshot::take();
        // threads should be available; rss may be None on non-macOS but
        // the snapshot itself never fails
        assert!(snap.threads.is_some());
    }
}
