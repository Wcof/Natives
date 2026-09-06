//! Mutation layer for the App Store registry (Phase A2, ADR-0025 D11/D37).
//!
//! Every successful mutation bumps the monotonic `app_meta` revision; the
//! navigation projection (Phase A3) rebuilds from it, and nothing else may
//! publish projection changes.
//!
//! Lock discipline (AGENTS.md): each public method holds the connection
//! guard exactly once and computes its return value inside that scope —
//! no method ever calls another public method while the guard is held.

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection};
use serde::Serialize;

use super::query;
use super::schema;
use super::types::{
    install_state, App, AppError, AppPackage, AppPermission, InstallRequest, InstallTransaction,
};
use crate::workspace_store::schema::uuid_v4;

pub struct AppStore {
    conn: Mutex<Connection>,
}

/// `uninstall` receipt. Personal data under
/// `~/.natives/apps/<app_id>/data/` is preserved by contract (ADR-0025
/// D32); only the registry rows are removed here. Deleting `fund.db`-style
/// personal data requires the separate, confirmed "Delete App Data" path.
#[derive(Serialize)]
pub struct UninstallReceipt {
    pub app_id: String,
    pub revision: i64,
    pub data_preserved: bool,
}

/// `apps:get` payload: the registry row plus its receipts and grants.
#[derive(Serialize)]
pub struct AppDetail {
    pub app: App,
    pub packages: Vec<AppPackage>,
    pub permissions: Vec<AppPermission>,
}

impl AppStore {
    /// Open the shared authoritative `natives.db` and run the App Store
    /// capability migrations (idempotent, no `user_version` write).
    pub fn open(path: &Path) -> Result<Self, AppError> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        schema::migrate_apps(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn with_conn<F, T>(&self, f: F) -> Result<T, AppError>
    where
        F: FnOnce(&Connection) -> Result<T, AppError>,
    {
        let conn = self
            .conn
            .lock()
            .map_err(|error| AppError::InvalidState(error.to_string()))?;
        f(&conn)
    }

    // ── read side ────────────────────────────────────────────────────

    /// All registered apps, sidebar order first (ADR-0025 D38).
    pub fn apps(&self) -> Result<Vec<App>, AppError> {
        self.with_conn(|conn| query::apps(conn))
    }

    /// One app row.
    pub fn app(&self, app_id: &str) -> Result<App, AppError> {
        self.with_conn(|conn| query::app(conn, app_id))
    }

    /// App row plus package receipts and permission grants.
    pub fn app_detail(&self, app_id: &str) -> Result<AppDetail, AppError> {
        self.with_conn(|conn| {
            Ok(AppDetail {
                app: query::app(conn, app_id)?,
                packages: query::packages(conn, app_id)?,
                permissions: query::permissions(conn, app_id)?,
            })
        })
    }

    /// One install transaction (survives uninstall for audit).
    pub fn transaction(&self, install_id: &str) -> Result<InstallTransaction, AppError> {
        self.with_conn(|conn| query::transaction(conn, install_id))
    }

    /// Monotonic global revision consumed by the navigation projection.
    pub fn global_revision(&self) -> Result<i64, AppError> {
        self.with_conn(|conn| query::global_revision(conn))
    }

    // ── install transaction (ADR-0025 D11) ───────────────────────────
    //
    // Phase A2 covers the metadata-only cycle: begin (catalog resolved)
    // → commit (registry rows) / abort (failed). Artifact transfer,
    // staging, host registration and rollback land in Phase A5.

    /// Register a resolved catalog entry as an install transaction.
    /// The app does NOT become an `App` until `install_commit`.
    pub fn install_begin(&self, request: &InstallRequest) -> Result<InstallTransaction, AppError> {
        request.app.validate()?;
        let install_id = uuid_v4();
        self.with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            let installed: Option<String> = tx
                .query_row(
                    "SELECT version FROM apps WHERE app_id = ?1",
                    [request.app.app_id.as_str()],
                    |row| row.get(0),
                )
                .ok();
            if let Some(version) = installed {
                return Err(AppError::Conflict(format!(
                    "app {} is already installed (version {version}); uninstall first",
                    request.app.app_id
                )));
            }
            let request_json = serde_json::to_string(request)
                .map_err(|error| AppError::InvalidState(error.to_string()))?;
            let now = now_millis();
            tx.execute(
                "INSERT INTO app_install_transactions (
                    install_id, app_id, from_version, to_version, request_json,
                    state, staging_path, started_at
                 ) VALUES (?1, ?2, '', ?3, ?4, ?5, '', ?6)",
                params![
                    install_id,
                    request.app.app_id,
                    request.app.version,
                    request_json,
                    install_state::CATALOG_RESOLVED,
                    now
                ],
            )?;
            tx.commit()?;
            Ok(InstallTransaction {
                install_id,
                app_id: request.app.app_id.clone(),
                from_version: String::new(),
                to_version: request.app.version.clone(),
                request_json,
                state: install_state::CATALOG_RESOLVED.to_string(),
                staging_path: String::new(),
                started_at: now,
                completed_at: None,
                error_code: None,
                error_message: None,
            })
        })
    }

    /// Atomically apply the stored request: app row, package receipts,
    /// permission grants, transaction → `installed`, revision bump.
    /// The commit is self-contained from `request_json`, so a crash
    /// between begin and commit never loses the catalog snapshot.
    pub fn install_commit(&self, install_id: &str) -> Result<App, AppError> {
        self.with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            let record = query::transaction(&tx, install_id)?;
            if record.state == install_state::INSTALLED {
                return Err(AppError::Conflict(format!(
                    "install {install_id} already committed"
                )));
            }
            let request: InstallRequest = serde_json::from_str(&record.request_json)
                .map_err(|error| AppError::InvalidState(error.to_string()))?;
            request.app.validate()?;
            let now = now_millis();
            tx.execute(
                "INSERT INTO apps (
                    app_id, kind, name, version, enabled, show_in_sidebar, sidebar_order,
                    runtime_spec_json, surface_json, manifest_json,
                    installed_at, updated_at, revision
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11, 0)
                 ON CONFLICT(app_id) DO UPDATE SET
                    kind = excluded.kind,
                    name = excluded.name,
                    version = excluded.version,
                    enabled = excluded.enabled,
                    show_in_sidebar = excluded.show_in_sidebar,
                    sidebar_order = excluded.sidebar_order,
                    runtime_spec_json = excluded.runtime_spec_json,
                    surface_json = excluded.surface_json,
                    manifest_json = excluded.manifest_json,
                    updated_at = excluded.updated_at,
                    revision = revision + 1",
                params![
                    request.app.app_id,
                    request.app.kind,
                    request.app.name,
                    request.app.version,
                    request.app.enabled as i64,
                    request.app.show_in_sidebar as i64,
                    request.app.sidebar_order,
                    value_to_string(&request.app.runtime_spec),
                    value_to_string(&request.app.surface),
                    value_to_string(&request.app.manifest),
                    now
                ],
            )?;
            for package in &request.packages {
                tx.execute(
                    "INSERT INTO app_packages (
                        app_id, package_id, kind, version, platform, arch,
                        wire_size, payload_size, artifact_sha256, payload_sha256,
                        installed_path, installed_at
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                     ON CONFLICT(app_id, package_id) DO UPDATE SET
                        kind = excluded.kind,
                        version = excluded.version,
                        platform = excluded.platform,
                        arch = excluded.arch,
                        wire_size = excluded.wire_size,
                        payload_size = excluded.payload_size,
                        artifact_sha256 = excluded.artifact_sha256,
                        payload_sha256 = excluded.payload_sha256,
                        installed_path = excluded.installed_path,
                        installed_at = excluded.installed_at",
                    params![
                        request.app.app_id,
                        package.package_id,
                        package.kind,
                        package.version,
                        package.platform,
                        package.arch,
                        package.wire_size,
                        package.payload_size,
                        package.artifact_sha256,
                        package.payload_sha256,
                        // A2 metadata-only stage: the receipt lands with an
                        // empty path; Phase A5 fills the Core-decided path
                        // at atomic commit time (ADR-0025 D8).
                        String::new(),
                        now
                    ],
                )?;
            }
            for permission in &request.permissions {
                tx.execute(
                    "INSERT INTO app_permissions (app_id, permission, granted, granted_at)
                     VALUES (?1, ?2, 1, ?3)
                     ON CONFLICT(app_id, permission) DO UPDATE SET
                        granted = 1,
                        granted_at = excluded.granted_at",
                    params![request.app.app_id, permission, now],
                )?;
            }
            tx.execute(
                "UPDATE app_install_transactions
                 SET state = ?2, completed_at = ?3, error_code = NULL, error_message = NULL
                 WHERE install_id = ?1",
                params![install_id, install_state::INSTALLED, now],
            )?;
            bump_revision(&tx)?;
            // Read the committed row BEFORE committing: `commit` consumes
            // the guard (AGENTS.md lock discipline), and the row is fully
            // visible inside the open transaction.
            let installed = query::app(&tx, &request.app.app_id)?;
            tx.commit()?;
            Ok(installed)
        })
    }

    /// Mark a transaction `failed` with an explicit error (ADR-0025 D11:
    /// the registry never shows a half-installed app; nothing here
    /// deletes committed registry state — rollback of staged artifacts
    /// is Phase A5 filesystem work).
    pub fn install_abort(
        &self,
        install_id: &str,
        error_code: &str,
        error_message: &str,
    ) -> Result<InstallTransaction, AppError> {
        self.with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            let record = query::transaction(&tx, install_id)?;
            if record.state != install_state::FAILED {
                let now = now_millis();
                tx.execute(
                    "UPDATE app_install_transactions
                     SET state = ?2, completed_at = ?3, error_code = ?4, error_message = ?5
                     WHERE install_id = ?1",
                    params![
                        install_id,
                        install_state::FAILED,
                        now,
                        error_code,
                        error_message
                    ],
                )?;
                bump_revision(&tx)?;
            }
            let record = query::transaction(&tx, install_id)?;
            tx.commit()?;
            Ok(record)
        })
    }

    /// Remove registry rows only; personal data is preserved (ADR-0025
    /// D32). Install history is retained for audit.
    pub fn uninstall(&self, app_id: &str) -> Result<UninstallReceipt, AppError> {
        self.with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            let known: Option<String> = tx
                .query_row("SELECT name FROM apps WHERE app_id = ?1", [app_id], |row| {
                    row.get(0)
                })
                .ok();
            if known.is_none() {
                return Err(AppError::NotFound(app_id.to_string()));
            }
            tx.execute("DELETE FROM apps WHERE app_id = ?1", [app_id])?;
            tx.execute("DELETE FROM app_packages WHERE app_id = ?1", [app_id])?;
            tx.execute("DELETE FROM app_permissions WHERE app_id = ?1", [app_id])?;
            let revision = bump_revision(&tx)?;
            tx.commit()?;
            Ok(UninstallReceipt {
                app_id: app_id.to_string(),
                revision,
                data_preserved: true,
            })
        })
    }

    // ── registry mutations (projection triggers, ADR-0025 D37) ───────

    pub fn set_enabled(&self, app_id: &str, enabled: bool) -> Result<App, AppError> {
        self.with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            let now = now_millis();
            let updated = tx.execute(
                "UPDATE apps
                 SET enabled = ?2, updated_at = ?3, revision = revision + 1
                 WHERE app_id = ?1",
                params![app_id, enabled as i64, now],
            )?;
            if updated == 0 {
                return Err(AppError::NotFound(app_id.to_string()));
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
            let now = now_millis();
            let updated = match order {
                Some(order) => tx.execute(
                    "UPDATE apps
                     SET show_in_sidebar = ?2, sidebar_order = ?3, updated_at = ?4,
                         revision = revision + 1
                     WHERE app_id = ?1",
                    params![app_id, show as i64, order, now],
                )?,
                None => tx.execute(
                    "UPDATE apps
                     SET show_in_sidebar = ?2, updated_at = ?3, revision = revision + 1
                     WHERE app_id = ?1",
                    params![app_id, show as i64, now],
                )?,
            };
            if updated == 0 {
                return Err(AppError::NotFound(app_id.to_string()));
            }
            bump_revision(&tx)?;
            let app = query::app(&tx, app_id)?;
            tx.commit()?;
            Ok(app)
        })
    }
}

fn value_to_string(value: &serde_json::Value) -> String {
    value.to_string()
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

/// Bump the monotonic `app_meta` revision (same connection/transaction
/// as the mutation, so the projection can never observe a half-write).
fn bump_revision(conn: &Connection) -> Result<i64, AppError> {
    conn.execute(
        "UPDATE app_meta SET revision = revision + 1 WHERE key = 'app_store'",
        [],
    )?;
    query::global_revision(conn)
}
