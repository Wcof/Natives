//! High-level Creative App service: unified list + lifecycle dispatch.
//!
//! Dispatch is owned by [`crate::creative_app::adapters`] — three real source
//! adapters hide Workshop / Docker / Local Process differences.

use super::adapters;
use super::model::*;
use crate::Result;
use rusqlite::Connection;

pub use crate::apps::mutation_lock::{new_mutation_lock, MutationLock, MutationLockRegistry};

pub struct CreativeAppService;

impl CreativeAppService {
    pub fn list(conn: &Connection) -> Result<Vec<CreativeAppSummary>> {
        adapters::list_all(conn)
    }

    pub fn get_open_target(conn: &Connection, id: &str) -> Result<OpenTarget> {
        adapters::open_target(conn, id)
    }

    pub fn get_summary(conn: &Connection, id: &str) -> Result<CreativeAppSummary> {
        adapters::get_summary(conn, id)
    }

    /// Resolve source for callers that need source-specific non-lifecycle APIs
    /// (logs, local config, …) without re-implementing lookup order.
    pub fn resolve_source(conn: &Connection, id: &str) -> Result<adapters::ResolvedSource> {
        adapters::resolve(conn, id)
    }
}

/// Only allow http://127.0.0.1:{port}{path} (or localhost).
pub fn validate_local_url(url: &str) -> Result<()> {
    let u = url.trim();
    let rest = if let Some(r) = u.strip_prefix("http://") {
        r
    } else if let Some(r) = u.strip_prefix("https://") {
        r
    } else {
        return Err(crate::Error::InvalidInput(
            "only http/https open URLs are allowed".into(),
        ));
    };
    let hostport = rest.split('/').next().unwrap_or("");
    let host = hostport.split(':').next().unwrap_or("");
    if host != "127.0.0.1" && host != "localhost" {
        return Err(crate::Error::InvalidInput(
            "open URL must target 127.0.0.1".into(),
        ));
    }
    Ok(())
}

/// Navigation allow-list for child webview (Embed surface, P0).
///
/// The initial URL is validated by [`validate_local_url`]; subsequent navigation
/// must stay on `127.0.0.1` / `localhost` too — a local app must never drive the
/// child webview to a public host.
pub fn navigation_allowed(url: &str) -> bool {
    let u = url.trim();
    if u.starts_with("http://") || u.starts_with("https://") {
        return validate_local_url(u).is_ok();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_url_validation() {
        assert!(validate_local_url("http://127.0.0.1:8080/").is_ok());
        assert!(validate_local_url("https://example.com/").is_err());
        assert!(validate_local_url("file:///etc/passwd").is_err());
        assert!(validate_local_url("tauri://localhost").is_err());
    }

    #[test]
    fn navigation_filter() {
        // Loopback only — the Embed child webview must never leave 127.0.0.1/localhost.
        assert!(navigation_allowed("http://127.0.0.1:1/"));
        assert!(navigation_allowed("https://127.0.0.1:8443/"));
        assert!(navigation_allowed("http://localhost:5173/"));
        assert!(!navigation_allowed("https://example.com/x"));
        assert!(!navigation_allowed("http://localhost.evil.com/x"));
        assert!(!navigation_allowed("http://127.0.0.1.evil.com/x"));
        assert!(!navigation_allowed("file:///tmp"));
        assert!(!navigation_allowed("data:text/html,hi"));
        assert!(!navigation_allowed("tauri://localhost"));
    }
}

/// Batch 2 CR-202: application-keyed lock + bounded install semaphore.
#[cfg(test)]
mod lock_tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime")
    }

    #[test]
    fn same_app_is_exclusive() {
        let lock = new_mutation_lock();
        let _a = rt().block_on(lock.acquire_app("app-a"));
        let entered = Arc::new(AtomicBool::new(false));
        let lock2 = lock.clone();
        let entered2 = entered.clone();
        let handle = std::thread::spawn(move || {
            let _g = rt().block_on(lock2.acquire_app("app-a"));
            entered2.store(true, Ordering::SeqCst);
        });
        std::thread::sleep(Duration::from_millis(50));
        assert!(
            !entered.load(Ordering::SeqCst),
            "a second same-app mutation must wait for the first"
        );
        drop(_a);
        handle
            .join()
            .expect("second acquire completes after release");
        assert!(entered.load(Ordering::SeqCst));
    }

    #[test]
    fn different_apps_run_in_parallel() {
        let lock = new_mutation_lock();
        let _a = rt().block_on(lock.acquire_app("app-a"));
        let completed = Arc::new(AtomicBool::new(false));
        let lock2 = lock.clone();
        let c2 = completed.clone();
        let handle = std::thread::spawn(move || {
            let _g = rt().block_on(lock2.acquire_app("app-b"));
            c2.store(true, Ordering::SeqCst);
        });
        // B must acquire without waiting for A's long operation (CR-202 #04).
        handle.join().expect("B finishes while A is still held");
        assert!(completed.load(Ordering::SeqCst));
    }

    #[test]
    fn install_semaphore_bounds_concurrency() {
        let lock = new_mutation_lock();
        let rt_guard = rt();
        let _p1 = rt_guard.block_on(lock.acquire_install());
        let _p2 = rt_guard.block_on(lock.acquire_install());
        let entered = Arc::new(AtomicBool::new(false));
        let lock2 = lock.clone();
        let entered2 = entered.clone();
        let handle = std::thread::spawn(move || {
            let _p = rt().block_on(lock2.acquire_install());
            entered2.store(true, Ordering::SeqCst);
        });
        std::thread::sleep(Duration::from_millis(50));
        assert!(
            !entered.load(Ordering::SeqCst),
            "the third install must wait for a bounded permit"
        );
        drop(_p1);
        drop(_p2);
        handle
            .join()
            .expect("third install proceeds after permits free");
        assert!(entered.load(Ordering::SeqCst));
    }

    #[test]
    fn try_acquire_skips_locked_app_but_takes_free_one() {
        let lock = new_mutation_lock();
        let _a = rt().block_on(lock.acquire_app("app-a"));
        assert!(
            lock.try_acquire_app("app-a").is_none(),
            "watchdog must not stall on an app under a lifecycle mutation"
        );
        let g = lock
            .try_acquire_app("app-b")
            .expect("free app is acquirable");
        drop(g);
        assert!(
            lock.try_acquire_app("app-b").is_some(),
            "a released app is acquirable again"
        );
    }
}
