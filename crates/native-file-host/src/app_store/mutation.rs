//! App registry authority. Install transactions and cleanup have separate owners.

use super::types::{App, AppError, AppPackage, AppPermission, InstallTransaction};
use super::{query, schema};
use crate::app_install;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub struct AppStore {
    conn: Mutex<Connection>,
    app_root: PathBuf,
    caller_origin: Mutex<Option<String>>,
    manifest_dir: Mutex<Option<PathBuf>>,
    product_source: Mutex<Option<PathBuf>>,
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
            product_source: Mutex::new(None),
            operation: Mutex::new(()),
            installs: Mutex::new(BTreeMap::new()),
        })
    }

    #[cfg(any(test, debug_assertions))]
    pub(crate) fn set_manifest_dir(&self, dir: PathBuf) {
        *self.manifest_dir.lock().expect("test manifest lock") = Some(dir);
    }

    /// Override the verified product system source (isolated local candidates
    /// and tests; production always uses the fixed system directory).
    #[cfg(any(test, debug_assertions))]
    pub(crate) fn set_product_source(&self, dir: PathBuf) {
        *self.product_source.lock().expect("product source lock") = Some(dir);
    }

    pub(super) fn product_source(&self) -> PathBuf {
        self.product_source
            .lock()
            .map_err(|_| ())
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

    /// Crate-wide shared connection access (app_runtime audit/invoke gates
    /// use the same Mutex discipline; see AGENTS.md lock rules).
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
        self.with_conn(|conn| {
            Ok(AppDetail {
                app: query::app(conn, app_id, Some(self.app_root()))?,
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
        let _operation = self.operation.lock().map_err(lock_error)?;
        // Plan §137: a running app cannot be disabled. Probe the runtime lock
        // (install→runtime order is trivially satisfied: install lock is not
        // held here) and release it after the DB update closes the read window.
        let _runtime = if !enabled {
            Some(app_install::acquire_app_lock(
                self.app_root(),
                app_id,
                true,
            )?)
        } else {
            None
        };
        let current_app = self.app(app_id)?;
        crate::app_activation::update_activation_enabled(self.app_root(), app_id, enabled)?;
        let updated = self.with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            if tx.execute(
                "UPDATE apps SET enabled = ?2, updated_at = ?3, revision = revision + 1 WHERE app_id = ?1",
                params![app_id, enabled, now_millis()],
            )? == 0 {
                return Err(AppError::NotFound(app_id.into()));
            }
            bump_revision(&tx)?;
            let app = query::app(&tx, app_id, Some(self.app_root()))?;
            tx.commit()?;
            Ok(app)
        });
        match updated {
            Ok(app) => Ok(app),
            Err(err) => {
                // AC-14: the projection compensation must not be silently
                // dropped. If it also fails, the app is left in a mixed DB/
                // activation state — surface repair_required instead of a
                // plain error so the center can offer a repair action.
                if let Err(restore_err) = crate::app_activation::update_activation_enabled(
                    self.app_root(),
                    app_id,
                    current_app.enabled,
                ) {
                    return Err(AppError::InvalidState(format!(
                        "APP_REPAIR_REQUIRED: set_enabled failed ({}); activation projection restore also failed ({})",
                        err, restore_err
                    )));
                }
                Err(err)
            }
        }
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
            if tx.execute(
                "UPDATE apps SET show_in_sidebar = ?2, sidebar_order = COALESCE(?3, sidebar_order),
                updated_at = ?4, revision = revision + 1 WHERE app_id = ?1",
                params![app_id, show, order, now_millis()],
            )? == 0
            {
                return Err(AppError::NotFound(app_id.into()));
            }
            bump_revision(&tx)?;
            let app = query::app(&tx, app_id, Some(self.app_root()))?;
            tx.commit()?;
            Ok(app)
        })
    }

    /// Explicit management rollback (contract §4.1): only allowed when stopped,
    /// previous version retained, and data schema is compatible via --inspect-data.
    pub fn rollback(&self, app_id: &str) -> Result<App, AppError> {
        let _operation = self.operation.lock().map_err(lock_error)?;
        let current_app = self.app(app_id)?;
        // Must be stopped: acquire runtime lock before checking or modifying state
        let _runtime = app_install::acquire_app_lock(self.app_root(), app_id, true)?;

        use rusqlite::OptionalExtension;
        // AC-05: the journal lives on the same transaction row; capture its
        // id so every stage below can mark progress crash-safely. The
        // existing recovery projection treats an installed app with a
        // non-empty rollback_json as recovery_pending.
        let (prev_version, journal_tx): (String, String) = self.with_conn(|conn| {
            Ok(conn.query_row(
                "SELECT from_version, install_id FROM app_install_transactions \
                 WHERE app_id = ?1 AND state = 'installed' AND from_version != '' AND from_version != to_version \
                 ORDER BY completed_at DESC LIMIT 1",
                params![app_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            ).optional()?)
        })?.ok_or_else(|| AppError::InvalidState("APP_NO_PREVIOUS_VERSION: no previous version available for rollback".into()))?;
        let mark_journal = |stage: &str| -> Result<(), AppError> {
            self.with_conn(|conn| {
                conn.execute(
                    "UPDATE app_install_transactions SET rollback_json = ?2 WHERE install_id = ?1",
                    rusqlite::params![journal_tx, stage],
                )
                .map_err(AppError::from)?;
                Ok(())
            })
        };

        let prev_target =
            crate::app_activation::find_runtime_binary(self.app_root(), app_id, &prev_version)
                .ok_or_else(|| {
                    AppError::InvalidState(
                        "APP_PREVIOUS_VERSION_MISSING: previous binary missing".into(),
                    )
                })?;

        // Inspect data schema compatibility using current binary's --inspect-data
        // Contract §4.1: bounded 5s, <=64 KiB, strict deserialization, default-deny.
        let current_target = crate::app_activation::find_runtime_binary(
            self.app_root(),
            app_id,
            &current_app.version,
        )
        .ok_or_else(|| {
            AppError::InvalidState(
                "APP_INSPECT_UNAVAILABLE: current binary missing for inspect".into(),
            )
        })?;
        let inspect_output =
            crate::app_activation::run_inspect_data(&current_target, self.app_root())?;
        // The rollback target's own declared dataSchema range (if any) comes
        // from its recorded install request.
        let prev_request_json: Option<String> = self.with_conn(|conn| {
            Ok(conn
                .query_row(
                    "SELECT request_json FROM app_install_transactions WHERE install_id = ?1",
                    [&journal_tx],
                    |row| row.get(0),
                )
                .optional()?)
        })?;
        let target_readable: Option<(u32, u32)> = prev_request_json
            .as_deref()
            .and_then(|json| serde_json::from_str::<serde_json::Value>(json).ok())
            .map(|v| v["app"]["manifest"]["dataSchema"].clone())
            .and_then(|ds| match ds {
                serde_json::Value::Number(n) => {
                    let m = n.as_u64()? as u32;
                    Some((m, m))
                }
                serde_json::Value::Object(o) => {
                    let min = o.get("readableMin").and_then(serde_json::Value::as_u64)? as u32;
                    let max = o
                        .get("readableMax")
                        .or_else(|| o.get("schemaVersion"))
                        .and_then(serde_json::Value::as_u64)? as u32;
                    Some((min, max))
                }
                _ => None,
            });
        inspect_allows_rollback(&inspect_output, target_readable)?;
        mark_journal("inspect_passed")?;

        // Journal BEFORE the DB swap: a crash after this point leaves the
        // recovery_pending marker (rolled-back DB version vs old binary) so
        // reopen must repair, never silently serve a mixed state.
        mark_journal("db_swap_pending")?;

        // Restore previous version in SQLite
        self.with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            tx.execute(
                "UPDATE apps SET version = ?2, updated_at = ?3, revision = revision + 1 WHERE app_id = ?1",
                params![app_id, prev_version, now_millis()],
            )?;
            bump_revision(&tx)?;
            tx.commit()?;
            Ok(())
        })?;

        // Re-register Host & update activation projection. Failures AFTER the
        // DB swap keep the journal marker and return repair_required — the
        // mixed state must be visible, never reported as a clean rollback.
        let dir = self.manifest_dir()?;
        let origin = self.caller_origin().unwrap_or_default();
        let host_name = crate::app_activation::register_runtime_host(app_id, &prev_target, &origin, &dir)
            .map_err(|e| {
                let _ = mark_journal("host_registration_failed");
                AppError::InvalidState(format!(
                    "APP_REPAIR_REQUIRED: rollback swapped the registry version but host registration failed: {}",
                    e
                ))
            })?;
        mark_journal("host_registered")?;
        let prev_payload_sha256 = {
            use sha2::{Digest, Sha256};
            let mut file = std::fs::File::open(&prev_target)?;
            let mut hasher = Sha256::new();
            std::io::copy(&mut file, &mut hasher)?;
            format!("{:x}", hasher.finalize())
        };
        let next_generation =
            crate::app_activation::next_activation_generation(self.app_root(), app_id);
        let projection = serde_json::json!({
            "receiptVersion": 1,
            "appId": app_id,
            "runtimeHost": host_name,
            "activeVersion": prev_version,
            "generation": next_generation,
            "activationState": if current_app.enabled { "ready" } else { "disabled" },
            "enabled": current_app.enabled,
            "appProtocolVersion": 1,
            "payloadSha256": prev_payload_sha256,
            "allowedOrigins": [origin],
        });
        crate::app_activation::write_activation_projection(self.app_root(), app_id, &projection)
            .map_err(|e| {
                let _ = mark_journal("projection_failed");
                AppError::InvalidState(format!(
                    "APP_REPAIR_REQUIRED: rollback swapped the registry version but the activation projection failed: {}",
                    e
                ))
            })?;
        // Journal cleared: the recovery projection no longer reports pending.
        let _ = mark_journal("");

        self.app(app_id)
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

/// Strict wire shape of a candidate's `--inspect-data` output (contract §4.1).
/// Field names are camelCase on the wire; `lastDataWriterVersion` is optional
/// because the reference sample host omits it. Unknown fields are tolerated so
/// a newer host adding a field does not force a false default-deny; the fields
/// the rollback gate depends on are required and strictly typed.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InspectData {
    #[allow(dead_code)]
    current_schema: u32,
    migration_state: String,
    #[allow(dead_code)]
    has_committed_new_writes: bool,
    previous_version_compatible: bool,
    #[serde(default)]
    #[allow(dead_code)]
    last_data_writer_version: Option<String>,
}

/// Default-deny rollback compatibility decision (contract §4.1) from a
/// candidate's raw `--inspect-data` stdout. Rollback is permitted ONLY when the
/// output deserializes strictly, the migration journal is settled
/// (`committed`), and the previous version is declared compatible. Unparseable
/// output, an unsettled/failed journal, or an incompatible schema all refuse
/// the rollback — the compatibility boolean is never matched with a substring.
pub(super) fn inspect_allows_rollback(
    raw: &[u8],
    target_readable: Option<(u32, u32)>,
) -> Result<(), AppError> {
    let inspect: InspectData = serde_json::from_slice(raw).map_err(|error| {
        AppError::InvalidState(format!(
            "APP_DATA_SCHEMA_INCOMPATIBLE: unreadable --inspect-data output: {error}"
        ))
    })?;
    if inspect.migration_state != "committed" {
        return Err(AppError::InvalidState(format!(
            "APP_DATA_SCHEMA_INCOMPATIBLE: migration journal not settled (migrationState={})",
            inspect.migration_state
        )));
    }
    // AC-05: the compatibility boolean alone is not trusted — when the
    // rollback target declares its dataSchema range, the CURRENT schema must
    // actually fall inside it.
    if let Some((min, max)) = target_readable {
        if inspect.current_schema < min || inspect.current_schema > max {
            return Err(AppError::InvalidState(format!(
                "APP_DATA_SCHEMA_INCOMPATIBLE: current schema {} outside the rollback target's declared readable range {}..={}",
                inspect.current_schema, min, max
            )));
        }
    }
    if !inspect.previous_version_compatible {
        return Err(AppError::InvalidState(
            "APP_DATA_SCHEMA_INCOMPATIBLE: data schema is incompatible with previous version"
                .into(),
        ));
    }
    Ok(())
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
