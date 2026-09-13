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

/// User-confirmed scope of a data reset (plan §4.3): the module's data
/// directory is always the target; imports/cache/logs and the Keychain
/// namespace are cleared only when explicitly selected. Credentials default
/// to preserved.
#[derive(Serialize, serde::Deserialize)]
pub struct ClearDataScope {
    pub imports: bool,
    pub cache: bool,
    pub logs: bool,
    pub credentials: bool,
}

#[derive(Serialize)]
pub struct ClearDataReceipt {
    pub request_id: String,
    pub app_id: String,
    /// `completed` receipts replay their original result; `pending` marks an
    /// unfinished reset whose scope stays blocked until retried.
    pub state: String,
    pub cleared: Vec<String>,
    /// Always true: code, registration, activation and display/enable/order
    /// preferences are never touched by a data reset.
    pub data_preserved: bool,
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
        // AC-04: record the removal intent BEFORE teardown so a crash between
        // receipt and cleanup can never lead to a seed reinstall.
        self.set_user_intent(app_id, "removed")?;
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
            let (name, host, permissions, previous_purge) = match query::app(&tx, app_id, Some(self.app_root())) {
                Ok(app) => {
                    let spec: serde_json::Value = serde_json::from_str(&app.runtime_spec_json)
                        .map_err(|_| AppError::InvalidState("invalid app runtime registration".into()))?;
                    let host = if app.kind == super::types::KIND_MANAGED_LOCAL && app.host_registered {
                        crate::app_activation::runtime_host_name(app_id)
                    } else {
                        spec.get("host").and_then(|h| h.as_str()).unwrap_or("").to_owned()
                    };
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
        let _ = crate::app_activation::remove_activation_projection(self.app_root(), app_id);
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

    /// Fixed built-in module cards (plan §3.4): the definition comes from the
    /// compile-time product manifest, overlaid with the user's stored
    /// preferences when a record exists, and honest availability from the
    /// actual payload on disk. A read-only projection — never a write, never
    /// an install record.
    pub fn module_projections(
        &self,
    ) -> Result<Vec<crate::app_product::ModuleProjection>, AppError> {
        self.with_conn(|conn| {
            let mut out = Vec::new();
            for module in crate::app_product::FIXED_MODULES {
                let preference = conn
                    .query_row(
                        "SELECT enabled, show_in_sidebar, sidebar_order FROM apps WHERE app_id = ?1",
                        [module.app_id],
                        |row| {
                            Ok((
                                row.get::<_, i64>(0)? != 0,
                                row.get::<_, i64>(1)? != 0,
                                row.get::<_, i64>(2)?,
                            ))
                        },
                    )
                    .optional()
                    .map_err(AppError::Sql)?;
                let configured = conn
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM apps WHERE app_id = ?1 AND host_registered != 0)",
                        [module.app_id],
                        |row| row.get::<_, i64>(0),
                    )
                    .map(|v| v != 0)
                    .map_err(AppError::Sql)?;
                out.push(crate::app_product::module_projection(
                    module,
                    preference,
                    configured,
                    self.app_root(),
                ));
            }
            Ok(out)
        })
    }

    /// Modules whose confirmed data reset has not completed (plan §4.3.5):
    /// their writes stay blocked until the same confirmation is retried.
    pub fn pending_data_resets(&self) -> Result<Vec<String>, AppError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT DISTINCT app_id FROM app_data_reset_receipts WHERE state = 'pending' ORDER BY app_id",
            )?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
    }

    /// Restricted data reset, fully separate from `uninstall_with_data`
    /// (plan §4.3): only the confirmed scope inside the module's private
    /// directory (plus opt-in logs/Keychain) is removed; program files,
    /// packages receipts, registration, activation, product identity and
    /// display/enable/order preferences are preserved, no `removed` intent
    /// is written, and `apps/<appId>/` itself is never deleted. The receipt
    /// lives in natives.db, outside every cleaned path: a completed
    /// requestId replays its original result, a pending one can be retried
    /// with the same confirmation.
    pub fn clear_module_data(
        &self,
        app_id: &str,
        request_id: &str,
        scope: ClearDataScope,
    ) -> Result<ClearDataReceipt, AppError> {
        app_install::validate_identifier(app_id, "app id")?;
        let request_id = request_id.trim();
        if request_id.is_empty() || request_id.len() > 128 {
            return Err(AppError::InvalidState(
                "requestId must be 1..=128 bytes".into(),
            ));
        }
        let scope_json = serde_json::to_string(&scope)
            .map_err(|_| AppError::InvalidState("invalid clear scope".into()))?;
        // Intent-first: record the reset before any teardown (AC-04 pattern).
        let stored: Option<(String, String, String)> = self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO app_data_reset_receipts (request_id, app_id, scope_json, state, cleared_json, created_at)
                 VALUES (?1, ?2, ?3, 'pending', '[]', ?4)
                 ON CONFLICT(request_id) DO NOTHING",
                params![request_id, app_id, scope_json, now_millis()],
            )
            .map_err(AppError::Sql)?;
            conn.query_row(
                "SELECT app_id, state, cleared_json FROM app_data_reset_receipts WHERE request_id = ?1",
                [request_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)),
            )
            .map(Some)
            .map_err(AppError::Sql)
        })?;
        if let Some((stored_app, state, cleared_json)) = stored {
            if stored_app != app_id {
                return Err(AppError::Conflict(format!(
                    "requestId {request_id} belongs to another app"
                )));
            }
            if state == "completed" {
                let stored_scope: Option<ClearDataScope> = self.with_conn(|conn| {
                    conn.query_row(
                        "SELECT scope_json FROM app_data_reset_receipts WHERE request_id = ?1",
                        [request_id],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()
                    .map_err(AppError::Sql)?
                    .map(|json| serde_json::from_str(&json))
                    .transpose()
                    .map_err(|_| AppError::InvalidState("invalid stored receipt scope".into()))
                })?;
                if stored_scope.is_some_and(|stored| {
                    stored.imports != scope.imports
                        || stored.cache != scope.cache
                        || stored.logs != scope.logs
                        || stored.credentials != scope.credentials
                }) {
                    return Err(AppError::Conflict(
                        "requestId was confirmed with a different scope".into(),
                    ));
                }
                // Completed replay: return the original result without
                // deleting anything recorded after the reset.
                let cleared: Vec<String> = serde_json::from_str(&cleared_json)
                    .map_err(|_| AppError::InvalidState("invalid stored receipt".into()))?;
                return Ok(ClearDataReceipt {
                    request_id: request_id.into(),
                    app_id: app_id.into(),
                    state,
                    cleared,
                    data_preserved: true,
                });
            }
        }

        let _operation = self.operation.lock().map_err(lock_error)?;
        let _install = app_install::acquire_app_lock(self.app_root(), app_id, false)?;
        let recovery_pending = self.with_conn(|conn| {
            Ok(query::app(conn, app_id, Some(self.app_root()))
                .map(|app| app.recovery_pending)
                .unwrap_or(false))
        })?;
        if recovery_pending {
            return Err(AppError::InvalidState(
                "APP_RECOVERY_PENDING: finish app recovery before clearing data".into(),
            ));
        }
        // Business in use is never force-stopped or deleted under way: the
        // runtime lock stays held by the owner, so this fails with the
        // documented APP_BUSY/APP_RUNNING_ELSEWHERE and the receipt stays
        // pending for a retry after the user closes the module.
        let _runtime = app_install::acquire_app_lock(self.app_root(), app_id, true)?;

        let base = self.app_root().join(app_id);
        app_install::validate_app_path(self.app_root(), &base)?;
        let natives = self
            .app_root()
            .parent()
            .ok_or_else(|| AppError::InvalidState("invalid app root".into()))?;
        let mut targets: Vec<(&str, std::path::PathBuf)> = vec![("data", base.join("data"))];
        if scope.imports {
            targets.push(("imports", base.join("imports")));
        }
        if scope.cache {
            targets.push(("cache", base.join("cache")));
        }
        if scope.logs {
            let logs = natives.join("logs/apps").join(app_id);
            app_install::validate_app_path(natives, &logs)?;
            targets.push(("logs", logs));
        }
        let mut cleared = Vec::new();
        for (label, path) in &targets {
            if path.exists() {
                app_install::remove_staging_dir(path)?;
                cleared.push((*label).into());
            }
        }
        if scope.credentials {
            let namespace = if app_id.starts_with("com.natives.app.") {
                app_id.to_owned()
            } else {
                format!("com.natives.app.{app_id}")
            };
            let granted = self.with_conn(|conn| {
                Ok(query::permissions(conn, app_id)?
                    .into_iter()
                    .any(|p| p.permission == format!("keychain:{namespace}")))
            })?;
            if granted {
                crate::app_secrets::purge_namespace(&namespace)?;
                cleared.push("credentials".into());
            }
        }

        let cleared_json = serde_json::to_string(&cleared)
            .map_err(|_| AppError::InvalidState("invalid cleared scope".into()))?;
        self.with_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            tx.execute(
                "UPDATE app_data_reset_receipts SET state = 'completed', cleared_json = ?2, completed_at = ?3
                 WHERE request_id = ?1 AND state = 'pending'",
                params![request_id, cleared_json, now_millis()],
            )
            .map_err(AppError::Sql)?;
            bump_revision(&tx)?;
            tx.commit()?;
            Ok(())
        })?;
        Ok(ClearDataReceipt {
            request_id: request_id.into(),
            app_id: app_id.into(),
            state: "completed".into(),
            cleared,
            data_preserved: true,
        })
    }
}
