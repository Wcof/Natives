//! Connection-owned install transactions with recoverable filesystem changes.

use super::mutation::{bump_revision, lock_error, now_millis, AppStore};
use super::{query, types::*};
use crate::{app_host, app_install};
use base64::Engine;
use rusqlite::{params, OptionalExtension};

impl AppStore {
    pub fn install_begin(&self, request: &InstallRequest) -> Result<InstallTransaction, AppError> {
        request.validate()?;
        if self.caller_origin().is_none() {
            return Err(AppError::InvalidState(
                "APP_ORIGIN_REQUIRED: Chrome caller origin missing".into(),
            ));
        }
        let _operation = self.operation.lock().map_err(lock_error)?;
        let lock = app_install::acquire_app_lock(self.app_root(), &request.app.app_id, false)?;
        self.recover_app(&request.app.app_id)?;
        let install_id = crate::workspace_store::schema::uuid_v4();
        let staging = app_install::staging_dir(self.app_root(), &request.app.app_id, &install_id)?;
        let record = self.with_conn(|conn| {
            let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)?;
            let pending: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM app_retained_data WHERE app_id = ?1 AND cleanup_pending = 1)",
                [&request.app.app_id], |row| row.get(0))?;
            if pending { return Err(AppError::Conflict("app cleanup must finish before installing".into())); }
            let from: Option<String> = tx.query_row("SELECT version FROM apps WHERE app_id = ?1",
                [&request.app.app_id], |row| row.get(0)).optional()?;
            if let Some(from) = &from {
                let current = semver::Version::parse(from).map_err(|_| AppError::InvalidState("installed version is invalid".into()))?;
                let next = semver::Version::parse(&request.app.version).map_err(|_| AppError::InvalidState("new version is invalid".into()))?;
                if next <= current { return Err(AppError::Conflict("update must increase the installed version".into())); }
            }
            let host = request.host_name()?;
            let collision: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM apps WHERE app_id != ?1 AND json_extract(runtime_spec_json, '$.host') = ?2)",
                params![request.app.app_id, host], |row| row.get(0))?;
            if collision { return Err(AppError::Conflict("runtime host already belongs to another app".into())); }
            let request_json = serde_json::to_string(request).map_err(|_| AppError::InvalidState("invalid install request".into()))?;
            tx.execute("INSERT INTO app_install_transactions
                (install_id, app_id, from_version, to_version, request_json, state, staging_path, started_at)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![install_id, request.app.app_id, from.unwrap_or_default(), request.app.version,
                    request_json, install_state::CATALOG_RESOLVED, staging.to_string_lossy(), now_millis()])?;
            let record = query::transaction(&tx, &install_id)?;
            tx.commit()?;
            Ok(record)
        })?;
        self.installs
            .lock()
            .map_err(lock_error)?
            .insert(install_id, lock);
        Ok(record)
    }

    fn owned_transaction(&self, install_id: &str) -> Result<InstallTransaction, AppError> {
        let record = self.transaction(install_id)?;
        if record.state == install_state::INSTALLED {
            return Err(AppError::Conflict("install already committed".into()));
        }
        if !self
            .installs
            .lock()
            .map_err(lock_error)?
            .contains_key(install_id)
        {
            return Err(AppError::Conflict(
                "install belongs to another connection or has ended".into(),
            ));
        }
        Ok(record)
    }

    pub fn install_package(
        &self,
        install_id: &str,
        package_id: &str,
        encoded: &str,
    ) -> Result<InstallPackageResult, AppError> {
        let _operation = self.operation.lock().map_err(lock_error)?;
        let record = self.owned_transaction(install_id)?;
        if !matches!(
            record.state.as_str(),
            install_state::CATALOG_RESOLVED | install_state::STAGING
        ) {
            return Err(AppError::InvalidState(
                "install no longer accepts packages".into(),
            ));
        }
        let result = (|| {
            let request = decode_request(&record)?;
            let package = request
                .packages
                .iter()
                .find(|p| p.package_id == package_id)
                .ok_or_else(|| {
                    AppError::InvalidState("package not declared in transaction".into())
                })?;
            if encoded.len() > PACKAGE_DATA_MAX_BASE64_BYTES {
                return Err(AppError::InvalidState(
                    "payload exceeds base64 budget".into(),
                ));
            }
            let data = base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .map_err(|_| AppError::InvalidState("invalid package encoding".into()))?;
            let hash = app_install::hex_sha256(&data);
            if data.len() as u64 > PACKAGE_MAX_PAYLOAD_BYTES
                || data.len() as i64 != package.payload_size
                || !app_install::sha256_matches(&package.payload_sha256, &hash)
            {
                return Err(AppError::InvalidState(
                    "package payload hash or size mismatch".into(),
                ));
            }
            let dir = app_install::staging_dir(self.app_root(), &record.app_id, install_id)?;
            let path = app_install::staged_payload_path(&dir, package_id)?;
            app_install::write_staged_payload(&path, &data)?;
            let ready = self.with_conn(|conn| {
                let tx = conn.unchecked_transaction()?;
                tx.execute("INSERT INTO app_package_stages (install_id, package_id, state, staged_path, payload_size, payload_sha256)
                    VALUES (?1, ?2, 'staging', ?3, ?4, ?5) ON CONFLICT(install_id, package_id) DO UPDATE SET
                    state = excluded.state, staged_path = excluded.staged_path, payload_size = excluded.payload_size, payload_sha256 = excluded.payload_sha256",
                    params![install_id, package_id, path.to_string_lossy(), data.len() as i64, hash])?;
                tx.execute("UPDATE app_install_transactions SET state = 'staging' WHERE install_id = ?1", [install_id])?;
                let count: usize = tx.query_row("SELECT COUNT(*) FROM app_package_stages WHERE install_id = ?1 AND state = 'staging'",
                    [install_id], |row| row.get(0))?;
                tx.commit()?;
                Ok(count == request.packages.len())
            })?;
            Ok(InstallPackageResult {
                install_id: install_id.into(),
                package_id: package_id.into(),
                state: install_state::STAGING.into(),
                payload_size: data.len() as u64,
                payload_sha256: hash,
                ready,
            })
        })();
        if let Err(error) = &result {
            self.abort_owned(&record, error.code(), "package verification failed")?;
        }
        result
    }

    pub fn install_commit(&self, install_id: &str) -> Result<App, AppError> {
        self.install_commit_with_origin(install_id, self.caller_origin().as_deref())
    }

    pub fn install_commit_with_origin(
        &self,
        install_id: &str,
        origin: Option<&str>,
    ) -> Result<App, AppError> {
        let _operation = self.operation.lock().map_err(lock_error)?;
        let record = self.owned_transaction(install_id)?;
        let trusted = self
            .caller_origin()
            .ok_or_else(|| AppError::InvalidState("Chrome caller origin required".into()))?;
        if origin
            .and_then(crate::app_host_manifest::normalize_chrome_extension_origin)
            .as_deref()
            != Some(&trusted)
        {
            return Err(AppError::InvalidState(
                "Chrome caller origin mismatch".into(),
            ));
        }
        let request = decode_request(&record)?;
        request.validate()?;
        let count = self.with_conn(|conn| Ok(conn.query_row("SELECT COUNT(*) FROM app_package_stages WHERE install_id = ?1 AND state = 'staging'",
            [install_id], |row| row.get::<_, usize>(0))?))?;
        if record.state != install_state::STAGING || count != request.packages.len() {
            return Err(AppError::InvalidState(
                "all packages must be staged before commit".into(),
            ));
        }
        let manifests = self.manifest_dir()?;
        let saved = app_host::snapshot(self.app_root(), &request, &manifests)?;
        self.with_conn(|conn| {
            let journal = serde_json::to_string(&saved).map_err(|_| AppError::InvalidState("invalid rollback record".into()))?;
            conn.execute("UPDATE app_install_transactions SET rollback_json = ?2, state = 'runtime_registering' WHERE install_id = ?1",
                params![install_id, journal])?;
            Ok(())
        })?;
        let result = (|| {
            self.set_install_state(install_id, install_state::HEALTH_CHECK)?;
            let runtime = app_host::prepare(self.app_root(), &request, install_id)?;
            let _runtime = app_install::acquire_app_lock(self.app_root(), &record.app_id, true)?;
            self.set_install_state(install_id, install_state::COMMITTING)?;
            app_host::activate(self.app_root(), &request, &runtime, &trusted, &manifests)?;
            self.sync_registration(request.host_name()?, true)?;
            self.commit_registry(&record, &request)
        })();
        match result {
            Ok(_) => {
                // Keep the journal until staging and the superseded version are gone.
                let cleanup = self.finish_committed(&record);
                self.installs.lock().map_err(lock_error)?.remove(install_id);
                cleanup?;
                self.app(&record.app_id)
            }
            Err(error) => {
                self.abort_owned(&record, error.code(), "runtime install failed")?;
                Err(error)
            }
        }
    }

    fn commit_registry(
        &self,
        record: &InstallTransaction,
        request: &InstallRequest,
    ) -> Result<App, AppError> {
        self.with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            let now = now_millis();
            tx.execute("INSERT INTO apps
                (app_id, kind, name, version, enabled, show_in_sidebar, sidebar_order, runtime_spec_json, surface_json, manifest_json,
                 installed_at, updated_at, revision, host_registered)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11, 0, 1)
                ON CONFLICT(app_id) DO UPDATE SET kind = excluded.kind, name = excluded.name, version = excluded.version,
                runtime_spec_json = excluded.runtime_spec_json, surface_json = excluded.surface_json, manifest_json = excluded.manifest_json,
                updated_at = excluded.updated_at, revision = apps.revision + 1, host_registered = 1",
                params![request.app.app_id, request.app.kind, request.app.name, request.app.version, request.app.enabled,
                    request.app.show_in_sidebar, request.app.sidebar_order, request.app.runtime_spec.to_string(),
                    request.app.surface.to_string(), request.app.manifest.to_string(), now])?;
            tx.execute("DELETE FROM app_packages WHERE app_id = ?1", [&record.app_id])?;
            for package in &request.packages {
                let path = app_install::install_path_for(self.app_root(), &record.app_id, &package.kind, &package.version, &package.package_id)?;
                tx.execute("INSERT INTO app_packages (app_id, package_id, kind, version, platform, arch, wire_size, payload_size,
                    artifact_sha256, payload_sha256, installed_path, installed_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
                    params![record.app_id, package.package_id, package.kind, package.version, package.platform, package.arch,
                        package.wire_size, package.payload_size, package.artifact_sha256, package.payload_sha256, path.to_string_lossy(), now])?;
            }
            tx.execute("DELETE FROM app_permissions WHERE app_id = ?1", [&record.app_id])?;
            for permission in &request.permissions {
                tx.execute("INSERT INTO app_permissions (app_id, permission, granted, granted_at) VALUES (?1, ?2, 1, ?3)",
                    params![record.app_id, permission, now])?;
            }
            tx.execute("DELETE FROM app_retained_data WHERE app_id = ?1", [&record.app_id])?;
            tx.execute("UPDATE app_install_transactions SET state = 'installed', completed_at = ?2, error_code = NULL, error_message = NULL WHERE install_id = ?1",
                params![record.install_id, now])?;
            bump_revision(&tx)?;
            let app = query::app(&tx, &record.app_id)?;
            tx.commit()?;
            Ok(app)
        })
    }

    fn set_install_state(&self, id: &str, state: &str) -> Result<(), AppError> {
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE app_install_transactions SET state = ?2 WHERE install_id = ?1",
                params![id, state],
            )?;
            Ok(())
        })
    }

    pub fn install_abort(
        &self,
        install_id: &str,
        code: &str,
        message: &str,
    ) -> Result<InstallTransaction, AppError> {
        let _operation = self.operation.lock().map_err(lock_error)?;
        let record = self.transaction(install_id)?;
        if record.state == install_state::FAILED {
            return Ok(record);
        }
        let record = self.owned_transaction(install_id)?;
        self.abort_owned(&record, code, message)?;
        self.transaction(install_id)
    }

    fn abort_owned(
        &self,
        record: &InstallTransaction,
        code: &str,
        message: &str,
    ) -> Result<(), AppError> {
        self.rollback_install(record, code, message)?;
        self.installs
            .lock()
            .map_err(lock_error)?
            .remove(&record.install_id);
        Ok(())
    }

    pub(super) fn rollback_install(
        &self,
        record: &InstallTransaction,
        code: &str,
        message: &str,
    ) -> Result<(), AppError> {
        let journal = self.with_conn(|conn| {
            Ok(conn.query_row(
                "SELECT rollback_json FROM app_install_transactions WHERE install_id = ?1",
                [&record.install_id],
                |row| row.get::<_, String>(0),
            )?)
        })?;
        let phase = self.transaction(&record.install_id)?.state;
        if !journal.is_empty() {
            let request = decode_request(record)?;
            let saved: app_host::Rollback = serde_json::from_str(&journal)
                .map_err(|_| AppError::InvalidState("invalid rollback journal".into()))?;
            if matches!(
                phase.as_str(),
                install_state::RUNTIME_REGISTERING | install_state::HEALTH_CHECK
            ) {
                app_host::remove_version(self.app_root(), &record.app_id, &record.to_version)?;
            } else {
                let _runtime =
                    app_install::acquire_app_lock(self.app_root(), &record.app_id, true)?;
                self.set_install_state(&record.install_id, install_state::ROLLING_BACK)?;
                app_host::rollback(self.app_root(), &request, &saved, &self.manifest_dir()?)?;
                self.sync_registration(request.host_name()?, saved.manifest.is_some())?;
            }
        }
        let staging =
            app_install::staging_dir(self.app_root(), &record.app_id, &record.install_id)?;
        app_install::remove_staging_dir(&staging)?;
        self.with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            tx.execute("DELETE FROM app_package_stages WHERE install_id = ?1", [&record.install_id])?;
            tx.execute("UPDATE app_install_transactions SET state = 'failed', rollback_json = '', completed_at = ?2, error_code = ?3, error_message = ?4 WHERE install_id = ?1",
                params![record.install_id, now_millis(), code.chars().take(64).collect::<String>(), message.chars().take(256).collect::<String>()])?;
            tx.commit()?;
            Ok(())
        })
    }

    fn finish_committed(&self, record: &InstallTransaction) -> Result<(), AppError> {
        let staging =
            app_install::staging_dir(self.app_root(), &record.app_id, &record.install_id)?;
        app_install::remove_staging_dir(&staging)?;
        if !record.from_version.is_empty() && record.from_version != record.to_version {
            app_host::remove_version(self.app_root(), &record.app_id, &record.from_version)?;
        }
        self.with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            tx.execute(
                "DELETE FROM app_package_stages WHERE install_id = ?1",
                [&record.install_id],
            )?;
            tx.execute(
                "UPDATE app_install_transactions SET rollback_json = '' WHERE install_id = ?1",
                [&record.install_id],
            )?;
            tx.commit()?;
            Ok(())
        })
    }

    pub(super) fn recover_app(&self, app_id: &str) -> Result<(), AppError> {
        let ids = self.with_conn(|conn| {
            let mut stmt = conn.prepare("SELECT install_id FROM app_install_transactions WHERE app_id = ?1 AND (state NOT IN ('installed', 'failed') OR rollback_json != '') ORDER BY started_at")?;
            let rows = stmt.query_map([app_id], |row| row.get::<_, String>(0))?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })?;
        for id in ids {
            let record = self.transaction(&id)?;
            if record.state == install_state::INSTALLED {
                self.finish_committed(&record)?;
            } else {
                self.rollback_install(&record, "APP_INTERRUPTED", "interrupted install recovered")?;
            }
        }
        Ok(())
    }

    pub fn recover_interrupted(&self) -> Result<(), AppError> {
        let _operation = self.operation.lock().map_err(lock_error)?;
        let ids = self.with_conn(|conn| {
            let mut stmt = conn.prepare("SELECT DISTINCT app_id FROM app_install_transactions WHERE state NOT IN ('installed','failed') OR rollback_json != ''")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })?;
        for id in ids {
            match app_install::acquire_app_lock(self.app_root(), &id, false) {
                Ok(_lock) => self.recover_app(&id)?,
                Err(AppError::Conflict(_)) => continue,
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    pub fn recover_install(&self, app_id: &str) -> Result<(), AppError> {
        let _operation = self.operation.lock().map_err(lock_error)?;
        let _install = app_install::acquire_app_lock(self.app_root(), app_id, false)?;
        self.recover_app(app_id)
    }
}

fn decode_request(record: &InstallTransaction) -> Result<InstallRequest, AppError> {
    serde_json::from_str(&record.request_json)
        .map_err(|_| AppError::InvalidState("invalid stored install request".into()))
}
