//! Registry mutations for built-in module preferences.

use super::types::{App, AppError};
use super::{query, schema};
use crate::app_files;
use rusqlite::{params, Connection};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub struct AppStore {
    conn: Mutex<Connection>,
    app_root: PathBuf,
    caller_origin: Mutex<Option<String>>,
    manifest_dir: Mutex<Option<PathBuf>>,
    product_source: Mutex<Option<PathBuf>>,
    pub(super) operation: Mutex<()>,
}

#[derive(Serialize)]
pub struct AppDetail {
    pub app: App,
}

impl AppStore {
    pub fn open(path: &Path) -> Result<Self, AppError> {
        Self::open_at(path, app_files::default_app_root())
    }

    pub fn open_at(path: &Path, app_root: impl Into<PathBuf>) -> Result<Self, AppError> {
        let migration_lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path.with_extension("apps-migration.lock"))?;
        migration_lock.lock()?;
        let conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_secs(2))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        schema::migrate_apps(&conn)?;
        drop(migration_lock);
        Ok(Self {
            conn: Mutex::new(conn),
            app_root: app_root.into(),
            caller_origin: Mutex::new(None),
            manifest_dir: Mutex::new(None),
            product_source: Mutex::new(None),
            operation: Mutex::new(()),
        })
    }

    #[cfg(any(test, debug_assertions))]
    pub(crate) fn set_manifest_dir(&self, dir: PathBuf) {
        *self.manifest_dir.lock().expect("manifest lock") = Some(dir);
    }
    #[cfg(any(test, debug_assertions))]
    pub(crate) fn set_product_source(&self, dir: PathBuf) {
        *self.product_source.lock().expect("product source lock") = Some(dir);
    }
    pub(super) fn product_source(&self) -> PathBuf {
        self.product_source
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
            .unwrap_or_else(crate::app_product::default_system_source)
    }
    pub(super) fn manifest_dir(&self) -> Result<PathBuf, AppError> {
        self.manifest_dir
            .lock()
            .map_err(lock_error)?
            .clone()
            .or_else(crate::app_host_manifest::chrome_manifest_dir)
            .ok_or_else(|| AppError::InvalidState("browser manifest directory unavailable".into()))
    }
    pub(crate) fn app_root(&self) -> &Path {
        &self.app_root
    }

    pub fn set_caller_origin(&self, origin: Option<&str>) {
        if let Ok(mut guard) = self.caller_origin.lock() {
            *guard = origin.and_then(crate::app_host_manifest::normalize_chrome_extension_origin);
        }
    }
    pub fn caller_origin(&self) -> Option<String> {
        self.caller_origin
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
    }

    pub(crate) fn with_conn<F, T>(&self, f: F) -> Result<T, AppError>
    where
        F: FnOnce(&Connection) -> Result<T, AppError>,
    {
        let conn = self.conn.lock().map_err(lock_error)?;
        f(&conn)
    }
    pub fn apps(&self) -> Result<Vec<App>, AppError> {
        self.with_conn(|conn| query::apps(conn, Some(self.app_root())))
    }
    pub fn app(&self, app_id: &str) -> Result<App, AppError> {
        self.with_conn(|conn| query::app(conn, app_id, Some(self.app_root())))
    }
    pub fn app_detail(&self, app_id: &str) -> Result<AppDetail, AppError> {
        Ok(AppDetail {
            app: self.app(app_id)?,
        })
    }
    pub fn global_revision(&self) -> Result<i64, AppError> {
        self.with_conn(query::global_revision)
    }

    pub fn set_enabled(&self, app_id: &str, enabled: bool) -> Result<App, AppError> {
        let _operation = self.operation.lock().map_err(lock_error)?;
        let current = self.app(app_id)?;
        // Keep the runtime lock through the activation update and DB commit so a
        // concurrent module request cannot race a disable operation.
        let _runtime = if !enabled {
            Some(app_files::acquire_app_lock(self.app_root(), app_id, true)?)
        } else {
            None
        };
        crate::app_activation::update_activation_enabled(self.app_root(), app_id, enabled)?;
        let result = self.with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            if tx.execute("UPDATE apps SET enabled = ?2, updated_at = ?3, revision = revision + 1 WHERE app_id = ?1", params![app_id, enabled, now_millis()])? == 0 { return Err(AppError::NotFound(app_id.into())); }
            bump_revision(&tx)?;
            let app = query::app(&tx, app_id, Some(self.app_root()))?;
            tx.commit()?;
            Ok(app)
        });
        if result.is_err() {
            let _ = crate::app_activation::update_activation_enabled(
                self.app_root(),
                app_id,
                current.enabled,
            );
        }
        result
    }

    pub fn set_sidebar(
        &self,
        app_id: &str,
        show: bool,
        order: Option<i64>,
    ) -> Result<App, AppError> {
        let _operation = self.operation.lock().map_err(lock_error)?;
        self.with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            if tx.execute("UPDATE apps SET show_in_sidebar = ?2, sidebar_order = COALESCE(?3, sidebar_order), updated_at = ?4, revision = revision + 1 WHERE app_id = ?1", params![app_id, show, order, now_millis()])? == 0 { return Err(AppError::NotFound(app_id.into())); }
            bump_revision(&tx)?;
            let app = query::app(&tx, app_id, Some(self.app_root()))?;
            tx.commit()?;
            Ok(app)
        })
    }
}

pub(super) fn lock_error<T>(_: std::sync::PoisonError<T>) -> AppError {
    AppError::InvalidState("app store lock unavailable".into())
}
pub(super) fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
pub(super) fn bump_revision(conn: &Connection) -> Result<i64, AppError> {
    conn.execute(
        "UPDATE app_meta SET revision = revision + 1 WHERE key = 'app_store'",
        [],
    )?;
    query::global_revision(conn)
}
