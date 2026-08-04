//! Cross-run conflict-key lease authority (TASK-012 / D02).
//!
//! A tool with a `conflict_key` (e.g. an exclusive file or a process) must
//! never overlap across runs. This is the in-memory single-daemon authority:
//! at most one run holds a key at a time, a second run asking for the same key
//! fails instead of overlapping, and cancel/completion releases the lease.
//!
//! Documented limit: this authority is process-local. Multi-process daemons
//! would need an additive DB-backed lease; single-daemon (the current
//! deployment) is fully covered here.

use std::collections::HashMap;
use std::sync::Mutex;

/// Cross-run conflict-key lease registry.
#[derive(Default)]
pub struct ConflictLeaseRegistry {
    holders: Mutex<HashMap<String, String>>,
}

impl ConflictLeaseRegistry {
    pub fn new() -> Self {
        Self {
            holders: Mutex::new(HashMap::new()),
        }
    }

    /// Acquire the lease for `key` on behalf of `run_id`. Idempotent for the
    /// same run; an overlapping run is rejected.
    pub fn acquire(&self, key: &str, run_id: &str) -> Result<(), String> {
        let mut holders = self.holders.lock().map_err(|e| e.to_string())?;
        match holders.get(key) {
            Some(holder) if holder == run_id => Ok(()),
            Some(holder) => Err(format!(
                "conflict key '{key}' is leased by run {holder}; cross-run overlap is not allowed"
            )),
            None => {
                holders.insert(key.to_string(), run_id.to_string());
                Ok(())
            }
        }
    }

    /// Release the lease if held by `run_id`. Cancellation and every terminal
    /// outcome must call this so a cancelled run never blocks the key forever.
    pub fn release(&self, key: &str, run_id: &str) {
        if let Ok(mut holders) = self.holders.lock() {
            if holders.get(key).map(String::as_str) == Some(run_id) {
                holders.remove(key);
            }
        }
    }

    /// Number of currently held leases.
    pub fn held(&self) -> usize {
        self.holders.lock().map(|h| h.len()).unwrap_or(0)
    }

    /// Acquire a lease that releases itself on drop — safe on every exit path
    /// (completion, failure, cancellation, early return).
    pub fn acquire_guard(&self, key: &str, run_id: &str) -> Result<ConflictLeaseGuard<'_>, String> {
        self.acquire(key, run_id)?;
        Ok(ConflictLeaseGuard {
            registry: self,
            key: key.to_string(),
            run_id: run_id.to_string(),
        })
    }
}

/// RAII conflict lease: releasing on drop guarantees a cancelled or failed
/// tool call never leaves its conflict key leased forever.
pub struct ConflictLeaseGuard<'a> {
    registry: &'a ConflictLeaseRegistry,
    key: String,
    run_id: String,
}

impl Drop for ConflictLeaseGuard<'_> {
    fn drop(&mut self) {
        self.registry.release(&self.key, &self.run_id);
    }
}

/// Process-wide conflict lease authority.
pub fn global_conflict_leases() -> &'static ConflictLeaseRegistry {
    use std::sync::OnceLock;
    static GLOBAL: OnceLock<ConflictLeaseRegistry> = OnceLock::new();
    GLOBAL.get_or_init(ConflictLeaseRegistry::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_key_never_overlaps_across_runs() {
        let registry = ConflictLeaseRegistry::new();
        registry.acquire("file:///tmp/x.txt", "run-a").unwrap();
        assert!(
            registry.acquire("file:///tmp/x.txt", "run-b").is_err(),
            "a second run must be rejected while the key is leased"
        );
        // Same run is idempotent (re-entrant within a batch).
        registry.acquire("file:///tmp/x.txt", "run-a").unwrap();
        assert_eq!(registry.held(), 1);
        // Release frees it for the next run.
        registry.release("file:///tmp/x.txt", "run-a");
        registry.acquire("file:///tmp/x.txt", "run-b").unwrap();
        assert_eq!(registry.held(), 1);
    }

    #[test]
    fn release_only_by_holder_keeps_lease() {
        let registry = ConflictLeaseRegistry::new();
        registry.acquire("k", "run-a").unwrap();
        registry.release("k", "run-b"); // wrong holder — no effect
        assert!(registry.acquire("k", "run-b").is_err());
        registry.release("k", "run-a");
        registry.acquire("k", "run-b").unwrap();
    }

    #[test]
    fn different_keys_do_not_block() {
        let registry = ConflictLeaseRegistry::new();
        registry.acquire("k1", "run-a").unwrap();
        registry.acquire("k2", "run-b").unwrap();
        assert_eq!(registry.held(), 2);
    }

    /// D02: the RAII guard releases the lease when it drops, so a cancelled or
    /// failed tool call never leaves its conflict key leased.
    #[test]
    fn guard_releases_lease_on_drop() {
        let registry = ConflictLeaseRegistry::new();
        {
            let _guard = registry.acquire_guard("k", "run-a").unwrap();
            assert!(
                registry.acquire("k", "run-b").is_err(),
                "the key is leased while the guard is alive"
            );
        }
        // Guard dropped → the key is free again.
        registry.acquire("k", "run-b").unwrap();
        assert_eq!(registry.held(), 1);
    }
}
