//! High-level Creative App service: unified list + lifecycle dispatch.

use super::install;
use super::model::*;
use super::store;
use crate::module_manager;
use crate::{Error, Result};
use rusqlite::Connection;
use std::sync::Arc;
use tauri::AppHandle;
use tokio::sync::Mutex;

/// Global async mutation lock (v1: one install/lifecycle write at a time).
pub type MutationLock = Arc<Mutex<()>>;

pub fn new_mutation_lock() -> MutationLock {
    Arc::new(Mutex::new(()))
}

pub struct CreativeAppService;

impl CreativeAppService {
    pub fn list(conn: &Connection) -> Result<Vec<CreativeAppSummary>> {
        let mut out = Vec::new();

        // Internal modules — reuse module_manager, do not reimplement
        let modules = module_manager::list_modules(conn)?;
        for m in modules {
            // Prefer richer columns when present
            let (description, icon) = load_module_meta(conn, &m.id);
            out.push(install::summary_from_internal(
                &m.id,
                &m.name,
                &m.version,
                m.enabled,
                description,
                icon,
            ));
        }

        for rec in store::list_apps(conn)? {
            out.push(install::summary_from_external(&rec));
        }

        // Sort: external running first, then title
        out.sort_by(|a, b| {
            let rank = |s: &CreativeAppSummary| match s.state {
                CreativeAppState::Running => 0,
                CreativeAppState::Available => 1,
                CreativeAppState::InstalledStopped => 2,
                CreativeAppState::StartFailed | CreativeAppState::InstallFailed => 3,
                _ => 4,
            };
            rank(a)
                .cmp(&rank(b))
                .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
        });
        Ok(out)
    }

    pub async fn start(
        conn: &Connection,
        app: &AppHandle,
        lock: &MutationLock,
        id: &str,
    ) -> Result<CreativeAppSummary> {
        let _guard = lock.lock().await;
        if let Some(rec) = store::get_app(conn, id)? {
            let _ = rec;
            return install::start_app(conn, app, id).await;
        }
        // Internal: enable
        module_manager::enable_module(conn, id)?;
        crate::emit_db_state_changed(
            app,
            "module",
            serde_json::json!({ "action": "enable", "moduleId": id }),
        );
        crate::emit_db_state_changed(
            app,
            "creative-app",
            serde_json::json!({ "action": "start", "id": id }),
        );
        Self::get_summary(conn, id)
    }

    pub async fn stop(
        conn: &Connection,
        app: &AppHandle,
        lock: &MutationLock,
        id: &str,
    ) -> Result<CreativeAppSummary> {
        let _guard = lock.lock().await;
        if store::get_app(conn, id)?.is_some() {
            return install::stop_app(conn, app, id).await;
        }
        module_manager::disable_module(conn, id)?;
        crate::emit_db_state_changed(
            app,
            "module",
            serde_json::json!({ "action": "disable", "moduleId": id }),
        );
        crate::emit_db_state_changed(
            app,
            "creative-app",
            serde_json::json!({ "action": "stop", "id": id }),
        );
        Self::get_summary(conn, id)
    }

    pub async fn delete(
        conn: &Connection,
        app: &AppHandle,
        lock: &MutationLock,
        id: &str,
        opts: DeleteOptions,
        modules_dir: &std::path::Path,
    ) -> Result<DeleteResult> {
        let _guard = lock.lock().await;
        if store::get_app(conn, id)?.is_some() {
            return install::delete_app(conn, app, id, opts).await;
        }
        module_manager::uninstall_module(conn, modules_dir, id)?;
        crate::emit_db_state_changed(
            app,
            "module",
            serde_json::json!({ "action": "uninstall", "moduleId": id }),
        );
        crate::emit_db_state_changed(
            app,
            "creative-app",
            serde_json::json!({ "action": "deleted", "id": id }),
        );
        Ok(DeleteResult {
            ok: true,
            warnings: vec![],
        })
    }

    pub fn get_open_target(conn: &Connection, id: &str) -> Result<OpenTarget> {
        if let Some(rec) = store::get_app(conn, id)? {
            if rec.state != CreativeAppState::Running {
                return Err(Error::InvalidInput(
                    "external app is not running".into(),
                ));
            }
            let url = rec
                .open_url
                .ok_or_else(|| Error::InvalidInput("missing openUrl".into()))?;
            validate_local_url(&url)?;
            return Ok(OpenTarget::LocalUrl {
                url,
                app_id: id.to_string(),
            });
        }
        // Internal module must be enabled
        let modules = module_manager::list_modules(conn)?;
        let m = modules
            .into_iter()
            .find(|m| m.id == id)
            .ok_or_else(|| Error::NotFound(id.into()))?;
        if m.enabled == 0 {
            return Err(Error::InvalidInput("module is disabled".into()));
        }
        Ok(OpenTarget::WorkshopModule {
            module_id: id.to_string(),
        })
    }

    pub fn get_summary(conn: &Connection, id: &str) -> Result<CreativeAppSummary> {
        if let Some(rec) = store::get_app(conn, id)? {
            return Ok(install::summary_from_external(&rec));
        }
        let modules = module_manager::list_modules(conn)?;
        let m = modules
            .into_iter()
            .find(|m| m.id == id)
            .ok_or_else(|| Error::NotFound(id.into()))?;
        let (description, icon) = load_module_meta(conn, &m.id);
        Ok(install::summary_from_internal(
            &m.id,
            &m.name,
            &m.version,
            m.enabled,
            description,
            icon,
        ))
    }
}

fn load_module_meta(conn: &Connection, id: &str) -> (Option<String>, Option<String>) {
    let mut stmt = match conn.prepare(
        "SELECT description, icon FROM modules WHERE id = ?1",
    ) {
        Ok(s) => s,
        Err(_) => return (None, None),
    };
    stmt.query_row(rusqlite::params![id], |row| {
        Ok((
            row.get::<_, Option<String>>(0)?,
            row.get::<_, Option<String>>(1)?,
        ))
    })
    .unwrap_or((None, None))
}

/// Only allow http://127.0.0.1:{port}{path} (or localhost).
pub fn validate_local_url(url: &str) -> Result<()> {
    let u = url.trim();
    let rest = if let Some(r) = u.strip_prefix("http://") {
        r
    } else if let Some(r) = u.strip_prefix("https://") {
        r
    } else {
        return Err(Error::InvalidInput(
            "only http/https open URLs are allowed".into(),
        ));
    };
    let hostport = rest.split('/').next().unwrap_or("");
    let host = hostport.split(':').next().unwrap_or("");
    if host != "127.0.0.1" && host != "localhost" {
        return Err(Error::InvalidInput(
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
