//! App registry authority. Install transactions and cleanup have separate owners.

use super::types::{App, AppError, AppPackage, AppPermission, InstallTransaction};
use super::{query, schema};
use rusqlite::{params, Connection};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub struct AppStore {
    conn: Mutex<Connection>,
    app_root: PathBuf,
    caller_origin: Mutex<Option<String>>,
    manifest_dir: Mutex<Option<PathBuf>>,
    pub(super) operation: Mutex<()>,
    pub(super) installs: Mutex<BTreeMap<String, File>>,
}

#[derive(Serialize)]
pub struct UninstallReceipt {
    pub app_id: String,
    pub revision: i64,
    pub data_preserved: bool,
}

#[derive(Serialize)]
pub struct AppDetail {
    pub app: App,
    pub packages: Vec<AppPackage>,
    pub permissions: Vec<AppPermission>,
}

impl AppStore {
    pub fn open(path: &Path) -> Result<Self, AppError> {
        Self::open_at(path, crate::app_install::default_app_root())
    }

    pub fn open_at(path: &Path, app_root: impl Into<PathBuf>) -> Result<Self, AppError> {
        // Serialize first-time WAL setup as well as schema migration across Hosts.
        let migration_lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
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
            operation: Mutex::new(()),
            installs: Mutex::new(BTreeMap::new()),
        })
    }

    #[cfg(any(test, debug_assertions))]
    pub(crate) fn set_manifest_dir(&self, dir: PathBuf) {
        *self.manifest_dir.lock().expect("test manifest lock") = Some(dir);
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

    pub(super) fn remove_registration(&self, host: &str) -> Result<(), AppError> {
        if self.manifest_dir.lock().map_err(lock_error)?.is_some() {
            return Ok(());
        }
        crate::app_host_manifest::remove_registration(&self.manifest_dir()?, host)?;
        Ok(())
    }

    pub fn clean_legacy_manifests(&self) -> Result<(), AppError> {
        let manifest_dir = self.manifest_dir()?;
        self.with_conn(|conn| {
            schema::clean_legacy_child_manifests_in(&manifest_dir, self.app_root(), conn)
        })
    }

    /// Bind Chrome's process argument. Page handshakes can only check it.
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

    pub(super) fn with_conn<F, T>(&self, f: F) -> Result<T, AppError>
    where
        F: FnOnce(&Connection) -> Result<T, AppError>,
    {
        let conn = self.conn.lock().map_err(lock_error)?;
        f(&conn)
    }

    pub fn apps(&self) -> Result<Vec<App>, AppError> {
        self.with_conn(query::apps)
    }
    pub fn app(&self, app_id: &str) -> Result<App, AppError> {
        self.with_conn(|conn| query::app(conn, app_id))
    }
    pub fn app_detail(&self, app_id: &str) -> Result<AppDetail, AppError> {
        self.with_conn(|conn| {
            Ok(AppDetail {
                app: query::app(conn, app_id)?,
                packages: query::packages(conn, app_id)?,
                permissions: query::permissions(conn, app_id)?,
            })
        })
    }
    pub fn transaction(&self, install_id: &str) -> Result<InstallTransaction, AppError> {
        self.with_conn(|conn| query::transaction(conn, install_id))
    }
    pub fn global_revision(&self) -> Result<i64, AppError> {
        self.with_conn(query::global_revision)
    }

    pub fn set_enabled(&self, app_id: &str, enabled: bool) -> Result<App, AppError> {
        self.with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            if tx.execute("UPDATE apps SET enabled = ?2, updated_at = ?3, revision = revision + 1 WHERE app_id = ?1",
                params![app_id, enabled, now_millis()])? == 0 {
                return Err(AppError::NotFound(app_id.into()));
            }
            bump_revision(&tx)?;
            let app = query::app(&tx, app_id)?;
            tx.commit()?;
            Ok(app)
        })
    }

    pub fn set_sidebar(
        &self,
        app_id: &str,
        show: bool,
        order: Option<i64>,
    ) -> Result<App, AppError> {
        self.with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            if tx.execute(
                "UPDATE apps SET show_in_sidebar = ?2, sidebar_order = COALESCE(?3, sidebar_order),
                updated_at = ?4, revision = revision + 1 WHERE app_id = ?1",
                params![app_id, show, order, now_millis()],
            )? == 0
            {
                return Err(AppError::NotFound(app_id.into()));
            }
            bump_revision(&tx)?;
            let app = query::app(&tx, app_id)?;
            tx.commit()?;
            Ok(app)
        })
    }

    pub fn read_resource(
        &self,
        app_id: &str,
        package_id: &str,
        offset: Option<u64>,
        length: Option<u64>,
    ) -> Result<super::types::ReadResourceResult, AppError> {
        crate::app_install::validate_identifier(app_id, "app_id")?;
        crate::app_install::validate_identifier(package_id, "package_id")?;
        use rusqlite::OptionalExtension;
        let (version, enabled, needs_migration, recovery_pending) = self
            .with_conn(|conn| {
                let res = conn
                    .query_row(
                        "SELECT version, enabled, needs_migration,
                         EXISTS(SELECT 1 FROM app_install_transactions AS recovery WHERE recovery.app_id = apps.app_id \
                         AND (recovery.state IN ('committing', 'rolling_back') \
                         OR (recovery.state = 'installed' AND recovery.rollback_json != '')))
                         FROM apps WHERE app_id = ?1",
                        [app_id],
                        |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? != 0, row.get::<_, i64>(2)? != 0, row.get::<_, i64>(3)? != 0)),
                    )
                    .optional()?;
                Ok(res)
            })?
            .ok_or_else(|| AppError::NotFound(format!("app not found: {app_id}")))?;

        if !enabled {
            return Err(AppError::InvalidState("app is disabled".into()));
        }
        if needs_migration {
            return Err(AppError::InvalidState(
                "app requires migration before reading resources".into(),
            ));
        }
        if recovery_pending {
            return Err(AppError::Conflict("app recovery pending".into()));
        }

        let (kind, pkg_version, installed_path) = self.with_conn(|conn| {
            let res = conn
                .query_row(
                    "SELECT kind, version, installed_path FROM app_packages WHERE app_id = ?1 AND package_id = ?2",
                    params![app_id, package_id],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)),
                )
                .optional()?;
            Ok(res)
        })?
        .ok_or_else(|| AppError::NotFound(format!("package not found: {package_id}")))?;

        if kind == "runtime" {
            return Err(AppError::InvalidState(
                "reading runtime binaries is forbidden".into(),
            ));
        }
        if pkg_version != version {
            return Err(AppError::InvalidState(
                "package version does not match installed app version".into(),
            ));
        }

        let expected_path = crate::app_install::install_path_for(
            self.app_root(),
            app_id,
            &kind,
            &version,
            package_id,
        )?;

        if !installed_path.is_empty() && Path::new(&installed_path) != expected_path {
            return Err(AppError::InvalidState(
                "installed path does not match canonical path".into(),
            ));
        }

        crate::app_install::validate_app_path(self.app_root(), &expected_path)?;

        if !expected_path.is_file() {
            return Err(AppError::NotFound("resource file not found on disk".into()));
        }

        let total_size = std::fs::metadata(&expected_path)?.len();
        let off = offset.unwrap_or(0);
        if off > total_size {
            return Err(AppError::InvalidState("offset exceeds file size".into()));
        }

        const MAX_RESOURCE_CHUNK_BYTES: u64 = 512 * 1024;
        let requested_len = length.unwrap_or(MAX_RESOURCE_CHUNK_BYTES);
        if requested_len == 0 || requested_len > MAX_RESOURCE_CHUNK_BYTES {
            return Err(AppError::InvalidState(
                "invalid length: must be 1..=524288 bytes".into(),
            ));
        }
        let read_len = (total_size - off).min(requested_len) as usize;

        use std::io::{Read, Seek, SeekFrom};
        let mut file = std::fs::File::open(&expected_path)?;
        let mut header = [0u8; 12];
        let header_len = file.read(&mut header)?;
        file.seek(SeekFrom::Start(off))?;
        let mut buf = vec![0u8; read_len];
        file.read_exact(&mut buf)?;

        let format = match &kind[..] {
            "data" => "json".to_string(),
            _ => {
                if header_len >= 8 && &header[..8] == b"\x89PNG\r\n\x1a\n" {
                    "png".to_string()
                } else if header_len >= 3 && &header[..3] == b"\xff\xd8\xff" {
                    "jpeg".to_string()
                } else if header_len >= 12 && &header[..4] == b"RIFF" && &header[8..12] == b"WEBP" {
                    "webp".to_string()
                } else {
                    "binary".to_string()
                }
            }
        };

        use base64::Engine;
        let encoded = base64::engine::general_purpose::STANDARD.encode(&buf);
        Ok(super::types::ReadResourceResult {
            ok: true,
            app_id: app_id.to_string(),
            package_id: package_id.to_string(),
            version: version.clone(),
            format,
            total_size,
            offset: off,
            length: read_len,
            data: encoded,
        })
    }
}

impl Drop for AppStore {
    fn drop(&mut self) {
        let ids: Vec<String> = match self.installs.lock() {
            Ok(installs) => installs.keys().cloned().collect(),
            Err(_) => return,
        };
        for id in ids {
            // An IO failure leaves the durable journal for the next Host.
            let _ = self.install_abort(&id, "APP_CANCELLED", "connection closed");
        }
    }
}

pub(super) fn lock_error<T>(_: std::sync::PoisonError<T>) -> AppError {
    AppError::InvalidState("app store lock unavailable".into())
}
pub(super) fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}
pub(super) fn bump_revision(conn: &Connection) -> Result<i64, AppError> {
    conn.execute(
        "UPDATE app_meta SET revision = revision + 1 WHERE key = 'app_store'",
        [],
    )?;
    query::global_revision(conn)
}
