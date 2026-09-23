//! Built-in module projections and confirmed per-user data reset.

use super::mutation::{bump_revision, lock_error, now_millis, AppStore};
use super::types::AppError;
use crate::app_files;
use rusqlite::{params, OptionalExtension};
use serde::Serialize;

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
    /// Fixed built-in module cards (plan §3.4): the definition comes from the
    /// compile-time product manifest, overlaid with the user's stored
    /// preferences when a record exists, and honest availability from the
    /// actual payload on disk. A read-only projection — never a write, never
    /// a module registration record.
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

    /// Restricted data reset: only the confirmed scope inside the module's private
    /// directory (plus opt-in logs/Keychain) is removed; program files,
    /// product files, registration, activation, product identity and
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
        app_files::validate_identifier(app_id, "app id")?;
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
        let _operation_lock = app_files::acquire_app_lock(self.app_root(), app_id, false)?;
        // Business in use is never force-stopped or deleted under way: the
        // runtime lock stays held by the owner, so this fails with the
        // documented APP_BUSY/APP_RUNNING_ELSEWHERE and the receipt stays
        // pending for a retry after the user closes the module.
        let _runtime = app_files::acquire_app_lock(self.app_root(), app_id, true)?;

        let base = self.app_root().join(app_id);
        app_files::validate_app_path(self.app_root(), &base)?;
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
            app_files::validate_app_path(natives, &logs)?;
            targets.push(("logs", logs));
        }
        let mut cleared = Vec::new();
        for (label, path) in &targets {
            if path.exists() {
                app_files::remove_staging_dir(path)?;
                cleared.push((*label).into());
            }
        }
        if scope.credentials {
            let namespace = if app_id.starts_with("com.natives.app.") {
                app_id.to_owned()
            } else {
                format!("com.natives.app.{app_id}")
            };
            crate::app_secrets::purge_namespace(&namespace)?;
            cleared.push("credentials".into());
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
