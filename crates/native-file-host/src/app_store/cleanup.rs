//! Uninstall keeps a durable cleanup receipt until every requested deletion succeeds.

use super::mutation::{bump_revision, lock_error, now_millis, AppStore, UninstallReceipt};
use super::{query, types::AppError};
use crate::{app_host_manifest, app_install};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;

#[derive(Serialize)]
pub struct RetainedData {
    pub app_id: String,
    pub name: String,
    pub cleanup_pending: bool,
    pub purge_data: bool,
}

impl AppStore {
    pub fn retained_data(&self) -> Result<Vec<RetainedData>, AppError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare("SELECT app_id, name, cleanup_pending, purge_data FROM app_retained_data ORDER BY name")?;
            let rows = stmt.query_map([], |row| Ok(RetainedData {
                app_id: row.get(0)?, name: row.get(1)?, cleanup_pending: row.get(2)?, purge_data: row.get(3)?,
            }))?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
    }

    pub fn uninstall(&self, app_id: &str) -> Result<UninstallReceipt, AppError> {
        self.uninstall_with_data(app_id, false)
    }

    pub fn uninstall_with_data(
        &self,
        app_id: &str,
        purge: bool,
    ) -> Result<UninstallReceipt, AppError> {
        let _operation = self.operation.lock().map_err(lock_error)?;
        app_install::validate_identifier(app_id, "app id")?;
        let _install = app_install::acquire_app_lock(self.app_root(), app_id, false)?;
        self.recover_app(app_id)?;
        let _runtime = app_install::acquire_app_lock(self.app_root(), app_id, true)?;
        let base = self.app_root().join(app_id);
        app_install::validate_app_path(self.app_root(), &base)?;
        let manifests = self.manifest_dir()?;
        let (host, permissions, purge) = self.with_conn(|conn| {
            let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)?;
            let existing = tx.query_row("SELECT host, permissions_json, purge_data FROM app_retained_data WHERE app_id = ?1",
                [app_id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, bool>(2)?))).optional()?;
            let (name, host, permissions, previous_purge) = match query::app(&tx, app_id) {
                Ok(app) => {
                    let spec: serde_json::Value = serde_json::from_str(&app.runtime_spec_json)
                        .map_err(|_| AppError::InvalidState("invalid app runtime registration".into()))?;
                    let host = spec.get("host").and_then(|h| h.as_str()).unwrap_or("").to_owned();
                    let permissions: Vec<String> = query::permissions(&tx, app_id)?.into_iter().map(|p| p.permission).collect();
                    (app.name, host, serde_json::to_string(&permissions).map_err(|_| AppError::InvalidState("invalid permissions".into()))?,
                        existing.as_ref().is_some_and(|entry| entry.2))
                }
                Err(AppError::NotFound(_)) => {
                    let (host, permissions, was_purge) = existing.ok_or_else(|| AppError::NotFound(app_id.into()))?;
                    (app_id.to_owned(), host, permissions, was_purge)
                }
                Err(error) => return Err(error),
            };
            if !host.is_empty() {
                app_host_manifest::manifest_path_in(&manifests, &host)?;
            }
            if previous_purge && !purge {
                return Err(AppError::InvalidState("APP_CONFIRMATION_REQUIRED: retry data deletion with confirmation".into()));
            }
            tx.execute("INSERT INTO app_retained_data (app_id, name, host, permissions_json, cleanup_pending, purge_data, updated_at)
                VALUES (?1, ?2, ?3, ?4, 1, ?5, ?6) ON CONFLICT(app_id) DO UPDATE SET
                cleanup_pending = 1, purge_data = excluded.purge_data, updated_at = excluded.updated_at",
                params![app_id, name, host, permissions, purge, now_millis()])?;
            tx.execute("UPDATE apps SET enabled = 0, revision = revision + 1 WHERE app_id = ?1", [app_id])?;
            bump_revision(&tx)?;
            tx.commit()?;
            Ok((host, permissions, purge))
        })?;

        // Validate every deletion root before removing registration or files.
        for sub in ["runtime", "packages", "staging"] {
            app_install::validate_app_path(self.app_root(), &base.join(sub))?;
        }
        let natives = self
            .app_root()
            .parent()
            .ok_or_else(|| AppError::InvalidState("invalid app root".into()))?;
        let logs = natives.join("logs/apps").join(app_id);
        if purge {
            app_install::validate_app_path(natives, &logs)?;
        }
        if !host.is_empty() {
            self.remove_registration(&host)?;
            app_host_manifest::remove_manifest_in(&manifests, &host)?;
        }
        if purge {
            let permissions: Vec<String> = serde_json::from_str(&permissions)
                .map_err(|_| AppError::InvalidState("invalid retained permissions".into()))?;
            let namespace = if app_id.starts_with("com.natives.app.") {
                app_id.to_owned()
            } else {
                format!("com.natives.app.{app_id}")
            };
            if permissions
                .iter()
                .any(|p| p == &format!("keychain:{namespace}"))
            {
                crate::app_secrets::purge_namespace(&namespace)?;
            }
            app_install::remove_staging_dir(&base)?;
            app_install::remove_staging_dir(&logs)?;
        } else {
            app_install::remove_app_install_dirs(self.app_root(), app_id)?;
        }
        let revision = self.with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            tx.execute("DELETE FROM app_packages WHERE app_id = ?1", [app_id])?;
            tx.execute("DELETE FROM app_permissions WHERE app_id = ?1", [app_id])?;
            tx.execute("DELETE FROM apps WHERE app_id = ?1", [app_id])?;
            if purge {
                tx.execute("DELETE FROM app_retained_data WHERE app_id = ?1", [app_id])?;
            } else {
                tx.execute(
                    "UPDATE app_retained_data SET cleanup_pending = 0 WHERE app_id = ?1",
                    [app_id],
                )?;
            }
            let revision = bump_revision(&tx)?;
            tx.commit()?;
            Ok(revision)
        })?;
        Ok(UninstallReceipt {
            app_id: app_id.into(),
            revision,
            data_preserved: !purge,
        })
    }
}
