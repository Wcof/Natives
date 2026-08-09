//! Tauri commands for multi-source Creative Apps (ADR-0013 + local_project).
//!
//! All async commands open rusqlite connections only in short synchronous
//! scopes (or inside `spawn_blocking`) so `Connection` is never held across
//! `.await` — Connection is !Send.

use crate::creative_app::adapters::{self, LifecycleCtx, ResolvedSource};
use crate::creative_app::browser::{self, BrowserStateHandle};
use crate::creative_app::docker;
use crate::creative_app::grant_store;
use crate::creative_app::install;
use crate::creative_app::local::{self, LocalRuntimeHandle};
use crate::creative_app::model::*;
use crate::creative_app::operation as op;
use crate::creative_app::profile_store::{self, ProfileBinding};
use crate::creative_app::runtime_store;
use crate::creative_app::service::{self, MutationLock};
use crate::creative_app::store;
use crate::creative_app::surface_store;
use crate::creative_app::window;
use crate::db::DbPool;
use crate::emit_db_state_changed;
use crate::{Error, Result};
use tauri::{Manager, State};
use tauri_plugin_dialog::DialogExt;

use crate::AppState;

mod browsing;
mod github;
mod lifecycle;
mod local_project;
mod proposals;
mod remote;
mod windows;

pub use browsing::*;
pub use github::*;
pub use lifecycle::*;
pub use local_project::*;
pub use proposals::*;
pub use remote::*;
pub use windows::*;

fn host_http_port(state: &AppState) -> u16 {
    *state.http_port.lock().unwrap_or_else(|e| e.into_inner())
}

fn modules_dir() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".natives")
        .join("modules")
}

fn conn(pool: &DbPool) -> Result<r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>> {
    pool.get().map_err(|e| Error::Internal(format!("db: {e}")))
}

fn lifecycle_ctx(
    app: tauri::AppHandle,
    local_runtime: LocalRuntimeHandle,
    host_http_port: u16,
) -> LifecycleCtx {
    LifecycleCtx::new(app, modules_dir(), Some(local_runtime), host_http_port)
}

/// Resolve which per-runtime log to read for a caller-supplied id (CR-301).
///
/// `id` may be a source id (app-scoped: read the active runtime; aggregate when
/// stopped) or a runtime instance id (runtime-scoped). Returns
/// `(source, runtime_id_to_read, app_id_for_dir)`.
fn resolve_log_scope(
    conn: &rusqlite::Connection,
    id: &str,
) -> Result<(ResolvedSource, Option<String>, String)> {
    if let Ok(ResolvedSource::LocalProject) = adapters::resolve(conn, id) {
        let app_id = runtime_store::application_id_for(conn, CreativeAppSource::LocalProject, id)?
            .unwrap_or_default();
        let active = if app_id.is_empty() {
            None
        } else {
            runtime_store::active_instance_id(conn, &app_id)?
        };
        return Ok((ResolvedSource::LocalProject, active, id.to_string()));
    }
    if let Ok(Some(app_id)) = runtime_store::instance_application_id(conn, id) {
        // `id` is a runtime instance id belonging to a local process source.
        let src = runtime_store::source_id_for_application(conn, &app_id)?
            .unwrap_or_else(|| id.to_string());
        return Ok((ResolvedSource::LocalProject, Some(id.to_string()), src));
    }
    let source = adapters::resolve(conn, id)?;
    Ok((source, None, id.to_string()))
}

fn format_local_log_lines(mem: &[crate::creative_app::local::logs::LogLine]) -> String {
    mem.iter()
        .map(|l| format!("[{}] {}: {}", l.ts_ms, l.stream.as_str(), l.text))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Unified application id for a source row, read-only (never fabricates a row).
fn operation_application_id(conn: &rusqlite::Connection, id: &str) -> Option<String> {
    adapters::resolve(conn, id).ok().and_then(|s| {
        runtime_store::application_id_for(conn, s.as_source(), id)
            .ok()
            .flatten()
    })
}

/// Resolve the unified application identity for a source row (read-only).
/// Returns a typed error when the app is not registered.
fn resolve_application_id(conn: &rusqlite::Connection, app_id: &str) -> Result<String> {
    let source = adapters::resolve(conn, app_id)?;
    runtime_store::application_id_for(conn, source.as_source(), app_id)?
        .ok_or_else(|| Error::InvalidInput("app has no registered identity".into()))
}

/// WebView labels of every non-closed window of an application (window-id labels).
fn open_window_labels(conn: &rusqlite::Connection, application_id: &str) -> Result<Vec<String>> {
    let mut labels = Vec::new();
    for w in surface_store::list_windows(conn, application_id)? {
        if w.state != WindowInstance::STATE_CLOSED {
            labels.push(browser::window_label(&w.id));
        }
    }
    Ok(labels)
}

/// Redacted input snapshot for the journal. Never stores env values or secrets.
fn redacted_for(kind: &str, id: &str) -> String {
    serde_json::json!({ "kind": kind, "appId": id }).to_string()
}

fn emit_operation(app: &tauri::AppHandle, conn: &rusqlite::Connection, op_id: i64) -> Result<()> {
    if let Some(operation) = op::get_operation(conn, op_id)? {
        emit_db_state_changed(
            app,
            "creative-operation",
            serde_json::to_value(&operation).unwrap_or_else(|_| serde_json::json!({ "id": op_id })),
        );
    }
    Ok(())
}

/// Guarded phase change + emit (called from the DB scopes of each mutation).
fn journal(
    app: &tauri::AppHandle,
    conn: &rusqlite::Connection,
    op_id: i64,
    from: &[&str],
    to: &str,
) -> Result<()> {
    op::transition(conn, op_id, from, to)?;
    emit_operation(app, conn, op_id)
}

fn settle_success(app: &tauri::AppHandle, conn: &rusqlite::Connection, op_id: i64) -> Result<()> {
    op::finish_success(conn, op_id)?;
    emit_operation(app, conn, op_id)
}

fn settle_failure(
    app: &tauri::AppHandle,
    conn: &rusqlite::Connection,
    op_id: i64,
    code: &str,
    message: &str,
) -> Result<()> {
    op::finish_failure(conn, op_id, Some(code), message)?;
    emit_operation(app, conn, op_id)
}

fn settle_cancelled(
    app: &tauri::AppHandle,
    conn: &rusqlite::Connection,
    op_id: i64,
    reason: &str,
) -> Result<()> {
    op::finish_cancelled(conn, op_id, Some(reason))?;
    emit_operation(app, conn, op_id)
}
