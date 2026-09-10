//! Connection-owned install transactions with recoverable filesystem changes.

use super::mutation::{bump_revision, lock_error, now_millis, AppStore};
use super::{query, types::*};
use crate::{app_host, app_install, app_signing};
use base64::Engine;
use rusqlite::{params, OptionalExtension};
use sha2::{Digest, Sha256};
use std::io::Read;

impl AppStore {
    /// Core Apps protocol v4 `apps:install_begin` (contract §4.0): the page
    /// uploads the SIGNED catalog plus its Ed25519 signature; Core verifies
    /// the signature against the fixed trust root, selects the executable
    /// package for this platform itself, and fixes the chunk size. The page
    /// cannot mark anything as verified.
    pub fn install_begin_catalog(
        &self,
        catalog_base64: &str,
        signature_base64: &str,
    ) -> Result<InstallBeginResult, AppError> {
        if catalog_base64.len() > CATALOG_MAX_BYTES * 4 / 3 + 4 {
            return Err(AppError::InvalidState(
                "APP_CATALOG_TOO_LARGE: catalog exceeds 256 KiB".into(),
            ));
        }
        let catalog = base64::engine::general_purpose::STANDARD
            .decode(catalog_base64)
            .map_err(|_| AppError::InvalidState("invalid catalog encoding".into()))?;
        if catalog.len() > CATALOG_MAX_BYTES {
            return Err(AppError::InvalidState(
                "APP_CATALOG_TOO_LARGE: catalog exceeds 256 KiB".into(),
            ));
        }
        app_signing::verify_catalog_signature(&catalog, signature_base64)?;
        let request: InstallRequest = serde_json::from_slice(&catalog)
            .map_err(|_| AppError::InvalidState("catalog is not a valid install request".into()))?;
        request.validate()?;
        // Core selects the package for THIS platform; the catalog never
        // decides disk paths or what runs.
        let package = request
            .packages
            .iter()
            .find(|p| p.kind == KIND_MANAGED_LOCAL)
            .ok_or_else(|| {
                AppError::InvalidState("catalog declares no managed_local executable".into())
            })?
            .clone();

        if self.caller_origin().is_none() {
            return Err(AppError::InvalidState(
                "APP_ORIGIN_REQUIRED: Chrome caller origin missing".into(),
            ));
        }
        let _operation = self.operation.lock().map_err(lock_error)?;
        // Self-heal FIRST: a leftover transaction from this connection's own
        // failed install still holds the install lock in self.installs, so
        // acquiring first would self-conflict on the same flock file
        // (AGENTS.md lock discipline). Cross-connection leftovers are still
        // handled by recover_app after the lock is taken.
        self.recover_owned_stale(&request.app.app_id)?;
        let lock = app_install::acquire_app_lock(self.app_root(), &request.app.app_id, false)?;
        // Plan §137: install→runtime lock order; a running app cannot be
        // updated. Never wait long or kill the running process.
        let _runtime = app_install::acquire_app_lock(self.app_root(), &request.app.app_id, true)?;
        let install_id = crate::workspace_store::schema::uuid_v4();
        let staging = app_install::staging_dir(self.app_root(), &request.app.app_id, &install_id)?;
        std::fs::create_dir_all(&staging)?;
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
            let request_json = serde_json::to_string(&request).map_err(|_| AppError::InvalidState("invalid install request".into()))?;
            tx.execute("INSERT INTO app_install_transactions
                (install_id, app_id, from_version, to_version, request_json, state, staging_path, started_at)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![install_id, request.app.app_id, from.unwrap_or_default(), request.app.version,
                    request_json, install_state::DOWNLOADING, staging.to_string_lossy(), now_millis()])?;
            tx.commit()?;
            Ok(())
        })?;
        self.installs
            .lock()
            .map_err(lock_error)?
            .insert(install_id.clone(), lock);
        let _ = record;
        Ok(InstallBeginResult {
            install_id,
            package_id: package.package_id,
            chunk_size: INSTALL_CHUNK_SIZE,
            next_offset: 0,
        })
    }

    /// v4 `apps:install_chunk` (contract §4.0): sequential append-only
    /// upload of the raw gzip artifact. `offset` must equal the confirmed
    /// length; an identical re-send of the last confirmed chunk is idempotent;
    /// overlap, divergence, out-of-order or oversized uploads are rejected
    /// with APP_PACKAGE_INVALID. The confirmed length is durable
    /// (staged_path + payload_size on the stage row), so reconnects resume.
    pub fn install_chunk(
        &self,
        install_id: &str,
        package_id: &str,
        offset: u64,
        data_base64: &str,
        chunk_sha256: &str,
    ) -> Result<InstallChunkResult, AppError> {
        let _operation = self.operation.lock().map_err(lock_error)?;
        let record = self.owned_transaction(install_id)?;
        if !matches!(
            record.state.as_str(),
            install_state::DOWNLOADING | install_state::CATALOG_RESOLVED
        ) {
            return Err(AppError::InvalidState(
                "install no longer accepts chunks".into(),
            ));
        }
        let request = decode_request(&record)?;
        let package = request
            .packages
            .iter()
            .find(|p| p.package_id == package_id)
            .ok_or_else(|| AppError::NotFound("package not declared in transaction".into()))?;
        if data_base64.len() > INSTALL_CHUNK_DATA_MAX_BASE64_BYTES {
            return Err(AppError::PackageInvalid(
                "chunk exceeds base64 budget".into(),
            ));
        }
        let data = base64::engine::general_purpose::STANDARD
            .decode(data_base64)
            .map_err(|_| AppError::PackageInvalid("invalid chunk encoding".into()))?;
        if data.len() > INSTALL_CHUNK_SIZE {
            return Err(AppError::PackageInvalid("chunk exceeds 256 KiB".into()));
        }
        if !app_install::sha256_matches(chunk_sha256, &app_install::hex_sha256(&data)) {
            return Err(AppError::PackageInvalid("chunk hash mismatch".into()));
        }
        if offset as u64 + data.len() as u64 > package.wire_size as u64 {
            return Err(AppError::PackageInvalid(
                "chunk extends past declared wire size".into(),
            ));
        }

        let staging = app_install::staging_dir(self.app_root(), &record.app_id, install_id)?;
        let artifact = app_install::staged_artifact_path(&staging, package_id)?;
        let confirmed: u64 = {
            let stage = self.with_conn(|conn| {
                Ok(conn.query_row(
                    "SELECT staged_path, payload_size FROM app_package_stages WHERE install_id = ?1 AND package_id = ?2",
                    params![install_id, package_id],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
                ).optional()?)
            })?;
            match stage {
                Some((path, len)) if path == artifact.to_string_lossy() => len.max(0) as u64,
                Some(_) => {
                    return Err(AppError::PackageInvalid(
                        "staged artifact path changed inside a transaction".into(),
                    ))
                }
                None => {
                    if offset != 0 {
                        return Err(AppError::PackageInvalid(
                            "first chunk must start at offset 0".into(),
                        ));
                    }
                    0
                }
            }
        };

        // Idempotent resend of the trailing confirmed chunk (or the whole
        // prefix at offset 0 when everything is already confirmed).
        if offset + data.len() as u64 <= confirmed {
            let existing = std::fs::read(&artifact).map_err(AppError::from)?;
            let slice = &existing[offset as usize..(offset + data.len() as u64) as usize];
            if slice == data.as_slice() {
                self.set_install_state(install_id, install_state::DOWNLOADING)?;
                return Ok(InstallChunkResult {
                    install_id: install_id.into(),
                    package_id: package_id.into(),
                    next_offset: confirmed,
                });
            }
            return Err(AppError::PackageInvalid(
                "resent chunk diverges from confirmed bytes".into(),
            ));
        }
        if offset != confirmed {
            return Err(AppError::PackageInvalid(format!(
                "out-of-order chunk: offset {offset} != confirmed {confirmed}"
            )));
        }

        // Sequential append; fsync so the confirmed length is durable.
        {
            use std::io::Write as _;
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&artifact)?;
            file.write_all(&data)?;
            file.sync_all()?;
        }
        let new_len = confirmed + data.len() as u64;
        self.with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            tx.execute(
                "INSERT INTO app_package_stages (install_id, package_id, state, staged_path, payload_size, payload_sha256)
                 VALUES (?1, ?2, 'downloading', ?3, ?4, ?5)
                 ON CONFLICT(install_id, package_id) DO UPDATE SET
                 state = excluded.state, staged_path = excluded.staged_path, payload_size = excluded.payload_size",
                params![install_id, package_id, artifact.to_string_lossy(), new_len as i64, chunk_sha256],
            )?;
            tx.execute(
                "UPDATE app_install_transactions SET state = ?2 WHERE install_id = ?1",
                params![install_id, install_state::DOWNLOADING],
            )?;
            tx.commit()?;
            Ok(())
        })?;
        Ok(InstallChunkResult {
            install_id: install_id.into(),
            package_id: package_id.into(),
            next_offset: new_len,
        })
    }

    /// v4 `apps:install_finish` (contract §4.0): the confirmed artifact must
    /// match the signed wire size and overall hash; Core streams the gunzip
    /// itself (bounded per step), verifies length/payload hash and executable
    /// format, then marks the package staged for commit.
    pub fn install_finish(
        &self,
        install_id: &str,
        package_id: &str,
        artifact_bytes: u64,
    ) -> Result<InstallFinishResult, AppError> {
        let _operation = self.operation.lock().map_err(lock_error)?;
        let record = self.owned_transaction(install_id)?;
        if !matches!(
            record.state.as_str(),
            install_state::DOWNLOADING | install_state::CATALOG_RESOLVED
        ) {
            return Err(AppError::InvalidState(
                "install no longer accepts uploads".into(),
            ));
        }
        let request = decode_request(&record)?;
        let package = request
            .packages
            .iter()
            .find(|p| p.package_id == package_id)
            .ok_or_else(|| AppError::NotFound("package not declared in transaction".into()))?;

        let staging = app_install::staging_dir(self.app_root(), &record.app_id, install_id)?;
        let artifact = app_install::staged_artifact_path(&staging, package_id)?;
        let confirmed: u64 = {
            let stage = self.with_conn(|conn| {
                Ok(conn.query_row(
                    "SELECT staged_path, payload_size FROM app_package_stages WHERE install_id = ?1 AND package_id = ?2",
                    params![install_id, package_id],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
                ).optional()?)
            })?;
            match stage {
                Some((path, len)) if path == artifact.to_string_lossy() => len.max(0) as u64,
                _ => {
                    return Err(AppError::PackageInvalid(
                        "no confirmed chunks to finish".into(),
                    ))
                }
            }
        };
        if artifact_bytes != package.wire_size as u64 || confirmed != package.wire_size as u64 {
            return Err(AppError::PackageInvalid(format!(
                "artifact length {artifact_bytes} (confirmed {confirmed}) != signed wire size {}",
                package.wire_size
            )));
        }

        let result = (|| {
            // Overall artifact hash over the staged file.
            let mut file = std::fs::File::open(&artifact)?;
            let mut hasher = Sha256::new();
            let mut buf = [0u8; 64 * 1024];
            loop {
                let n = file.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                hasher.update(&buf[..n]);
            }
            let actual: String = hasher
                .finalize()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            if !app_install::sha256_matches(&package.artifact_sha256, &actual) {
                return Err(AppError::PackageInvalid(format!(
                    "APP_ARTIFACT_HASH_MISMATCH: staged artifact hash {actual} != declared {}",
                    package.artifact_sha256
                )));
            }
            // Streamed gunzip with per-step bomb guard; trailing members and
            // truncation are rejected by the multi-member read below.
            let payload = app_install::staged_payload_path(&staging, package_id)?;
            app_install::decompress_gzip_bounded(
                &artifact,
                &payload,
                package.payload_size as u64,
                &package.payload_sha256,
            )?;
            self.with_conn(|conn| {
                let tx = conn.unchecked_transaction()?;
                tx.execute(
                    "UPDATE app_package_stages SET state = 'staging' WHERE install_id = ?1 AND package_id = ?2",
                    params![install_id, package_id],
                )?;
                tx.execute(
                    "UPDATE app_install_transactions SET state = ?2 WHERE install_id = ?1",
                    params![install_id, install_state::STAGING],
                )?;
                tx.commit()?;
                Ok(())
            })?;
            Ok(())
        })();
        if let Err(error) = &result {
            self.abort_owned(&record, error.code(), "artifact verification failed")?;
        }
        result?;
        Ok(InstallFinishResult {
            install_id: install_id.into(),
            package_id: package_id.into(),
            state: install_state::STAGING.into(),
            payload_size: package.payload_size as u64,
            payload_sha256: package.payload_sha256.clone(),
            ready: true,
        })
    }

    pub fn install_begin(&self, request: &InstallRequest) -> Result<InstallTransaction, AppError> {
        request.validate()?;
        if self.caller_origin().is_none() {
            return Err(AppError::InvalidState(
                "APP_ORIGIN_REQUIRED: Chrome caller origin missing".into(),
            ));
        }
        let _operation = self.operation.lock().map_err(lock_error)?;
        // Self-heal FIRST (see install_begin_catalog): this connection's own
        // leftover transaction holds the install lock in self.installs.
        self.recover_owned_stale(&request.app.app_id)?;
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
            app_install::validate_resource_payload(&package.kind, &data)?;
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
        if !request.packages.is_empty() {
            let count = self.with_conn(|conn| Ok(conn.query_row("SELECT COUNT(*) FROM app_package_stages WHERE install_id = ?1 AND state = 'staging'",
                [install_id], |row| row.get::<_, usize>(0))?))?;
            if record.state != install_state::STAGING || count != request.packages.len() {
                return Err(AppError::InvalidState(
                    "all packages must be staged before commit".into(),
                ));
            }
        }
        self.with_conn(|conn| {
            conn.execute("UPDATE app_install_transactions SET rollback_json = 'commit_done' WHERE install_id = ?1",
                [install_id])?;
            Ok(())
        })?;
        let result = (|| {
            self.set_install_state(install_id, install_state::COMMITTING)?;
            for pkg in &request.packages {
                let staged = app_install::staged_payload_path(
                    std::path::Path::new(&record.staging_path),
                    &pkg.package_id,
                )?;
                let target = app_install::install_path_for(
                    self.app_root(),
                    &record.app_id,
                    &pkg.kind,
                    &request.app.version,
                    &pkg.package_id,
                )?;
                app_install::install_staged_payload(&staged, &target)?;
                // ADR-0027 managed_local activation (contract §5 steps 4–7):
                // platform signature gate → exec bit → bounded --health
                // probe → Native Host registration. The executable is NOT
                // started as the app.
                if pkg.kind == KIND_MANAGED_LOCAL {
                    // Contract §4.0.1 step 4: verify code identity BEFORE the
                    // probe. Dev fixtures carry the explicit fixture flag.
                    let fixture = request
                        .app
                        .manifest
                        .get("fixture")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false);
                    let _backend = app_signing::verify_platform_signature(&target, fixture)?;
                    crate::app_activation::make_executable(&target)?;
                    crate::app_activation::health_probe(&target)?;
                    self.with_conn(|conn| {
                        conn.execute("UPDATE app_install_transactions SET rollback_json = 'host_registered' WHERE install_id = ?1",
                            [install_id])?;
                        Ok(())
                    })?;
                    let origin = trusted.clone();
                    crate::app_activation::register_runtime_host(&record.app_id, &target, &origin)?;
                    self.with_conn(|conn| {
                        conn.execute("UPDATE app_install_transactions SET rollback_json = 'activated' WHERE install_id = ?1",
                            [install_id])?;
                        Ok(())
                    })?;
                }
            }
            self.commit_registry(&record, &request)
        })();
        match result {
            Ok(_) => {
                let cleanup = self.finish_committed(&record);
                // Drop the returned File immediately (temporary value): the
                // flock must release before this function returns.
                self.installs.lock().map_err(lock_error)?.remove(install_id);
                if let Err(e) = cleanup {
                    return Err(e);
                }
                self.app(&record.app_id)
            }
            Err(error) => {
                // Activation failure after registration: do not leave a
                // "success but registration unusable" half-install (A3 §143);
                // the previous version stays usable via rollback_install.
                if error.code() != "APP_NOT_FOUND" {
                    let _ = crate::app_activation::unregister_runtime_host(&record.app_id);
                }
                self.abort_owned(&record, error.code(), "package install failed")?;
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
                 installed_at, updated_at, revision, host_registered, needs_migration)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11, 0, 0, 0)
                ON CONFLICT(app_id) DO UPDATE SET kind = excluded.kind, name = excluded.name, version = excluded.version,
                runtime_spec_json = excluded.runtime_spec_json, surface_json = excluded.surface_json, manifest_json = excluded.manifest_json,
                updated_at = excluded.updated_at, revision = apps.revision + 1, host_registered = 0, needs_migration = 0",
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
            tx.execute("UPDATE app_install_transactions SET state = 'installed', rollback_json = 'installed', completed_at = ?2, error_code = NULL, error_message = NULL WHERE install_id = ?1",
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
        // Release the install lock FIRST: a rollback failure must not leak
        // the lock in self.installs (flock self-conflict on the next begin).
        // The removed File drops immediately (temporary value).
        self.installs
            .lock()
            .map_err(lock_error)?
            .remove(&record.install_id);
        self.rollback_install(record, code, message)
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
            let _ = phase;
            app_host::remove_version(self.app_root(), &record.app_id, &record.to_version)?;
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
        // Contract §5 step 7: keep the new version plus ONE previous version;
        // anything older is cleaned. The update itself does not run the app.
        if !record.from_version.is_empty() && record.from_version != record.to_version {
            app_host::retain_versions(
                self.app_root(),
                &record.app_id,
                &[record.to_version.as_str(), record.from_version.as_str()],
            )?;
        } else {
            app_host::retain_versions(
                self.app_root(),
                &record.app_id,
                &[record.to_version.as_str()],
            )?;
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

    /// Roll back this connection's own leftover transactions for `app_id`
    /// (ones still registered in `self.installs`, i.e. holding the install
    /// lock). Called BEFORE acquiring the app lock in the begin methods so
    /// a same-connection retry after a failed install cannot self-conflict
    /// on the flock file. Transactions owned by another connection (not in
    /// `self.installs`) are left untouched — `recover_app` after the lock
    /// acquisition owns that decision.
    fn recover_owned_stale(&self, app_id: &str) -> Result<(), AppError> {
        let ids = self.with_conn(|conn| {
            let mut stmt = conn.prepare("SELECT install_id FROM app_install_transactions WHERE app_id = ?1 AND (state NOT IN ('installed', 'failed') OR rollback_json != '') ORDER BY started_at")?;
            let rows = stmt.query_map([app_id], |row| row.get::<_, String>(0))?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })?;
        let mut owned = self.installs.lock().map_err(lock_error)?;
        for id in ids {
            // Only roll back what THIS connection still owns (lock held in
            // the map). Removing the entry drops the install lock.
            if owned.remove(&id).is_some() {
                let record = self.transaction(&id)?;
                if record.state == install_state::INSTALLED {
                    drop(owned);
                    self.finish_committed(&record)?;
                    owned = self.installs.lock().map_err(lock_error)?;
                } else {
                    drop(owned);
                    self.rollback_install(
                        &record,
                        "APP_INTERRUPTED",
                        "interrupted install recovered",
                    )?;
                    owned = self.installs.lock().map_err(lock_error)?;
                }
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
