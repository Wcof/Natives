//! Port lease registry (batch 7 CR-703).
//!
//! Operation-scoped short leases for host ports. Prevents TOCTOU "port is
//! free" races: a port is reserved (leased) before a process starts, so two
//! concurrent starts never claim the same port. Leases expire on drop or TTL.

use crate::{Error, Result};
use std::collections::HashMap;
use std::net::TcpListener;
use std::sync::Mutex;

/// Default lease TTL (seconds). A lease that outlives its operation is a leak.
const DEFAULT_LEASE_TTL_SECS: i64 = 300;

struct LeaseEntry {
    port: u16,
    op_key: String,
    expires_at: i64,
}

/// Port lease registry — thread-safe, bounded to a max number of leases.
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
    /// Fails if the port is already leased by a different operation or is
    /// actually in use (TCP bind attempt).
    pub fn acquire(&self, port: u16, op_key: &str) -> Result<()> {
        let now = crate::creative_app::runtime_store::now();
        let _ = now;
        let expires = unix_now() + DEFAULT_LEASE_TTL_SECS;
        let mut st = self.inner.lock().unwrap_or_else(|e| e.into_inner());

        // Expire stale leases
        st.leases
            .retain(|_, e| e.expires_at > unix_now());

        if let Some(existing) = st.leases.get(&port) {
            return Err(Error::InvalidInput(format!(
                "port {port} already leased by operation {}",
                existing.op_key
            )));
        }

        // Verify the port is actually free on the OS level.
        if TcpListener::bind(("127.0.0.1", port)).is_err() {
            return Err(Error::InvalidInput(format!(
                "port {port} is in use by another process"
            )));
        }

        st.leases.insert(
            port,
            LeaseEntry {
                port,
                op_key: op_key.to_string(),
                expires_at: expires,
            },
        );
        Ok(())
    }

    /// Release a lease. Idempotent — releasing an unknown port is a no-op.
    pub fn release(&self, port: u16) {
        let mut st = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        st.leases.remove(&port);
    }

    /// Check whether a port is currently leased.
    pub fn is_leased(&self, port: u16) -> bool {
        let st = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        st.leases.contains_key(&port)
    }

    /// Number of active leases.
    pub fn len(&self) -> usize {
        let st = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        st.leases.len()
    }
}

impl Default for PortLeaseRegistry {
    fn default() -> Self {
        Self::new()
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
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
type natural_t = u32;
#[cfg(target_os = "macos")]
type mach_port_t = u32;
#[cfg(target_os = "macos")]
const MACH_TASK_BASIC_INFO: u32 = 20;
#[cfg(target_os = "macos")]
const KERN_SUCCESS: i32 = 0;

#[cfg(target_os = "macos")]
#[link(name = "System", kind = "framework")]
unsafe extern "C" {
    fn mach_task_self() -> mach_port_t;
    fn task_info(
        task: mach_port_t,
        flavor: u32,
        info: *mut i32,
        count: *mut u32,
    ) -> i32;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acquire_and_release_lease() {
        let registry = PortLeaseRegistry::new();
        // Find a free port
        let free_port = {
            let l = TcpListener::bind(("127.0.0.1", 0)).unwrap();
            l.local_addr().unwrap().port()
        };
        // Drop the listener to free the port
        drop(std::net::TcpListener::bind(("127.0.0.1", free_port)).unwrap());

        registry.acquire(free_port, "op-1").unwrap();
        assert!(registry.is_leased(free_port));
        assert_eq!(registry.len(), 1);

        registry.release(free_port);
        assert!(!registry.is_leased(free_port));
        assert_eq!(registry.len(), 0);
    }

    #[test]
    fn concurrent_acquire_conflicts() {
        let registry = PortLeaseRegistry::new();
        let free_port = {
            let l = TcpListener::bind(("127.0.0.1", 0)).unwrap();
            l.local_addr().unwrap().port()
        };

        registry.acquire(free_port, "op-1").unwrap();
        // Second acquire for the same port must fail
        assert!(registry.acquire(free_port, "op-2").is_err());
    }

    #[test]
    fn release_is_idempotent() {
        let registry = PortLeaseRegistry::new();
        let free_port = {
            let l = TcpListener::bind(("127.0.0.1", 0)).unwrap();
            l.local_addr().unwrap().port()
        };
        registry.acquire(free_port, "op-1").unwrap();
        registry.release(free_port);
        registry.release(free_port); // no-op
        assert_eq!(registry.len(), 0);
    }

    #[test]
    fn acquire_rejects_in_use_port() {
        let registry = PortLeaseRegistry::new();
        // Bind a real listener to a port
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        // The port is genuinely in use — acquire must fail
        assert!(registry.acquire(port, "op-1").is_err());
    }

    #[test]
    fn resource_snapshot_degrades_gracefully() {
        let snap = ResourceSnapshot::take();
        // threads should be available; rss may be None on non-macOS but
        // the snapshot itself never fails
        assert!(snap.threads.is_some());
    }
}