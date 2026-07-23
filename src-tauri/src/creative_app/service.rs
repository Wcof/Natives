//! High-level Creative App service: unified list + lifecycle dispatch.
//!
//! Dispatch is owned by [`crate::creative_app::adapters`] — three real source
//! adapters hide Workshop / Docker / Local Process differences.

use super::adapters::{self, LifecycleCtx};
use super::model::*;
use crate::Result;
use rusqlite::Connection;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Global async mutation lock (v1: one install/lifecycle write at a time).
pub type MutationLock = Arc<Mutex<()>>;

pub fn new_mutation_lock() -> MutationLock {
    Arc::new(Mutex::new(()))
}

pub struct CreativeAppService;

impl CreativeAppService {
    pub fn list(conn: &Connection) -> Result<Vec<CreativeAppSummary>> {
        adapters::list_all(conn)
    }

    pub async fn start(
        conn: &Connection,
        ctx: &LifecycleCtx,
        lock: &MutationLock,
        id: &str,
    ) -> Result<CreativeAppSummary> {
        let _guard = lock.lock().await;
        adapters::start(conn, ctx, id).await
    }

    pub async fn stop(
        conn: &Connection,
        ctx: &LifecycleCtx,
        lock: &MutationLock,
        id: &str,
    ) -> Result<CreativeAppSummary> {
        let _guard = lock.lock().await;
        adapters::stop(conn, ctx, id).await
    }

    pub async fn delete(
        conn: &Connection,
        ctx: &LifecycleCtx,
        lock: &MutationLock,
        id: &str,
        opts: DeleteOptions,
    ) -> Result<DeleteResult> {
        let _guard = lock.lock().await;
        adapters::delete(conn, ctx, id, opts).await
    }

    pub async fn restart(
        conn: &Connection,
        ctx: &LifecycleCtx,
        lock: &MutationLock,
        id: &str,
    ) -> Result<CreativeAppSummary> {
        let _guard = lock.lock().await;
        adapters::restart(conn, ctx, id).await
    }

    pub fn get_open_target(conn: &Connection, id: &str) -> Result<OpenTarget> {
        adapters::open_target(conn, id)
    }

    pub fn get_summary(conn: &Connection, id: &str) -> Result<CreativeAppSummary> {
        adapters::get_summary(conn, id)
    }

    /// Resolve source for callers that need source-specific non-lifecycle APIs
    /// (logs, local config, …) without re-implementing lookup order.
    pub fn resolve_source(
        conn: &Connection,
        id: &str,
    ) -> Result<adapters::ResolvedSource> {
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

/// Navigation allow-list for child webview.
pub fn navigation_allowed(url: &str) -> bool {
    let u = url.trim();
    if u.starts_with("http://") || u.starts_with("https://") {
        return true;
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
        assert!(navigation_allowed("http://127.0.0.1:1/"));
        assert!(navigation_allowed("https://example.com/x"));
        assert!(!navigation_allowed("file:///tmp"));
        assert!(!navigation_allowed("data:text/html,hi"));
    }
}
