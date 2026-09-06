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
    install_state, App, AppError, AppPackage, AppPermission, InstallPackageResult, InstallRequest,
    InstallTransaction, PACKAGE_DATA_MAX_BASE64_BYTES, PACKAGE_MAX_PAYLOAD_BYTES,
    PACKAGE_MAX_WIRE_BYTES, REQUIRED_MAX_PACKAGES,
};
use crate::app_install;
use crate::workspace_store::schema::uuid_v4;

pub struct AppStore {
    conn: Mutex<Connection>,
    /// Root of the app install directory (`~/.natives/apps`). Injected in
    /// tests; `default_app_root()` outside.
    app_root: std::path::PathBuf,
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
        Self::open_at(path, app_install::default_app_root())
    }

    /// Test/override constructor with an explicit install root
    /// (`~/.natives/apps` by default, ADR-0025 D16).
    pub fn open_at(path: &Path, app_root: impl Into<std::path::PathBuf>) -> Result<Self, AppError> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        schema::migrate_apps(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
            app_root: app_root.into(),
        })
    }

    /// `~/.natives/apps` — the single install root for every app.
    fn app_root(&self) -> &std::path::Path {
        &self.app_root
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

    /// Ingest one decompressed package payload (ADR-0025 D8/D10/D11):
    /// base64-decode, enforce the 20 MiB payload gate, stage under
    /// `~/.natives/apps/<app_id>/staging/<install_id>/`, verify the
    /// payload SHA-256 declared in the (signed) catalog, and advance
    /// the per-package stage to `staging`.
    ///
    /// The 5 MiB wire gate and the artifact SHA-256 are enforced by the
    /// browser (which fetched the bytes); the Host re-checks everything
    /// it owns — payload size, payload hash, staging path, registry
    /// state — so a compromised UI cannot smuggle an oversized or
    /// tampered payload through.
    pub fn install_package(
        &self,
        install_id: &str,
        package_id: &str,
        data_base64: &str,
    ) -> Result<InstallPackageResult, AppError> {
        // 1) Transaction must exist and not be terminal.
        let record = self.with_conn(|conn| query::transaction(conn, install_id))?;
        match record.state.as_str() {
            install_state::INSTALLED => {
                return Err(AppError::Conflict(format!(
                    "install {install_id} already committed"
                )))
            }
            install_state::FAILED | install_state::ROLLING_BACK => {
                return Err(AppError::InvalidState(format!(
                    "install {install_id} is {state} — start a new transaction",
                    state = record.state
                )))
            }
            _ => {}
        }
        // 2) Decode the base64 payload (size-checked BEFORE decoding so a
        //    hostile client cannot allocate gigabytes).
        if data_base64.len() > PACKAGE_DATA_MAX_BASE64_BYTES {
            return Err(AppError::InvalidState(format!(
                "package payload too large: base64 length {} exceeds {} bytes",
                data_base64.len(),
                PACKAGE_DATA_MAX_BASE64_BYTES
            )));
        }
        use base64::Engine as _;
        let data = base64::engine::general_purpose::STANDARD
            .decode(data_base64)
            .map_err(|error| {
                AppError::InvalidState(format!("payload is not valid base64: {error}"))
            })?;
        if (data.len() as u64) > PACKAGE_MAX_PAYLOAD_BYTES {
            return Err(AppError::InvalidState(format!(
                "payload too large: {} bytes exceeds the {} byte (20 MiB) gate",
                data.len(),
                PACKAGE_MAX_PAYLOAD_BYTES
            )));
        }
        // 3) The package must be declared in the signed catalog snapshot
        //    stored at begin time (request_json is the self-contained
        //    authority — the catalog entry cannot drift mid-install).
        let request: InstallRequest = serde_json::from_str(&record.request_json)
            .map_err(|error| AppError::InvalidState(error.to_string()))?;
        let declared = request
            .packages
            .iter()
            .find(|package| package.package_id == package_id)
            .ok_or_else(|| {
                AppError::NotFound(format!(
                    "package {package_id} is not in the install transaction"
                ))
            })?;
        if (declared.wire_size as u64) > PACKAGE_MAX_WIRE_BYTES {
            return Err(AppError::InvalidState(format!(
                "catalog wire_size {} exceeds the {} byte (5 MiB) gate",
                declared.wire_size, PACKAGE_MAX_WIRE_BYTES
            )));
        }
        // 4) Stage it (Core-decided path, never catalog-provided).
        let staging = app_install::staging_dir(self.app_root(), &record.app_id, install_id)?;
        let staged_path = app_install::staged_payload_path(&staging, package_id)?;
        let (size, hash) = app_install::write_staged_payload(&staged_path, &data)?;
        // 5) Verify the payload hash declared by the signed catalog.
        if !app_install::sha256_matches(&declared.payload_sha256, &hash) {
            let _ = app_install::remove_staging_dir(&staging);
            return self.fail_transaction(
                install_id,
                "PAYLOAD_HASH_MISMATCH",
                format!(
                    "payload sha256 mismatch for package {package_id} (declared {})",
                    declared.payload_sha256
                ),
            );
        }
        if (declared.payload_size as u64) != size {
            let _ = app_install::remove_staging_dir(&staging);
            return self.fail_transaction(
                install_id,
                "PAYLOAD_SIZE_MISMATCH",
                format!(
                    "payload size {size} != catalog payload_size {}",
                    declared.payload_size
                ),
            );
        }
        // 6) Advance per-package + transaction state.
        let ready = self.with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            tx.execute(
                "INSERT INTO app_package_stages (install_id, package_id, state, staged_path, payload_size, payload_sha256)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(install_id, package_id) DO UPDATE SET
                    state = excluded.state,
                    staged_path = excluded.staged_path,
                    payload_size = excluded.payload_size,
                    payload_sha256 = excluded.payload_sha256",
                params![
                    install_id,
                    package_id,
                    install_state::STAGING,
                    staged_path.to_string_lossy(),
                    size as i64,
                    hash
                ],
            )?;
            // App-level state machine: the first Host-authoritative
            // advance is into `staging` (A2 transactions start at
            // catalog_resolved); later calls keep it there until commit
            // drives runtime_registering/health_check/committing.
            let current: String = tx
                .query_row(
                    "SELECT state FROM app_install_transactions WHERE install_id = ?1",
                    [install_id],
                    |row| row.get(0),
                )
                .unwrap_or_else(|_| install_state::CATALOG_RESOLVED.to_string());
            if current == install_state::CATALOG_RESOLVED {
                tx.execute(
                    "UPDATE app_install_transactions SET state = ?2 WHERE install_id = ?1",
                    params![install_id, install_state::STAGING],
                )?;
            }
            let staged_count: i64 = tx
                .query_row(
                    "SELECT COUNT(*) FROM app_package_stages
                     WHERE install_id = ?1 AND state = 'staging'",
                    [install_id],
                    |row| row.get(0),
                )
                .unwrap_or(0);
            let required_count = request.packages.len() as i64;
            let tx_state: String = tx
                .query_row(
                    "SELECT state FROM app_install_transactions WHERE install_id = ?1",
                    [install_id],
                    |row| row.get(0),
                )
                .unwrap_or_default();
            tx.commit()?;
            Ok::<(i64, i64, String), AppError>((staged_count, required_count, tx_state))
        })?;
        let (staged_count, required_count, state) = ready;
        let ready = required_count > 0 && staged_count == required_count;
        Ok(InstallPackageResult {
            install_id: install_id.to_string(),
            package_id: package_id.to_string(),
            state,
            payload_size: size,
            payload_sha256: hash,
            ready,
        })
    }

    /// Mark a transaction `failed` with an explicit error code. The
    /// registry never shows a half-installed app; staged artifacts are
    /// cleaned by `install_abort`.
    fn fail_transaction(
        &self,
        install_id: &str,
        error_code: &str,
        error_message: String,
    ) -> Result<InstallPackageResult, AppError> {
        self.install_abort(install_id, error_code, &error_message)?;
        Err(AppError::InvalidState(error_message))
    }

    /// Mark a transaction `failed` with an explicit error (ADR-0025 D11:
    /// the registry never shows a half-installed app; nothing here
    /// deletes committed registry state — rollback of staged artifacts
    /// happens in `install_abort`).
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
