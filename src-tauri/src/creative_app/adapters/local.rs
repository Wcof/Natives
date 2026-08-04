//! Local Project adapter — Local Process / static HTTP under host Unique Origin.
//!
//! Source directory is reference-only (never delete project files).
//! Stop / crash / port conflict recovery lives in local::lifecycle.
//! Workshop Bridge is never attached.

use crate::creative_app::local::{self, LocalRuntimeManager};
use crate::creative_app::model::*;
use crate::creative_app::service::validate_local_url;
use crate::{Error, Result};
use rusqlite::Connection;
use tauri::AppHandle;

pub fn list(conn: &Connection) -> Result<Vec<CreativeAppSummary>> {
    let recs = local::list_apps(conn)?;
    Ok(recs.iter().map(local::summary_from_local).collect())
}

pub fn get(conn: &Connection, id: &str) -> Result<CreativeAppSummary> {
    let rec = local::get_app(conn, id)?.ok_or_else(|| Error::NotFound(id.into()))?;
    Ok(local::summary_from_local(&rec))
}

pub async fn start(
    conn: &Connection,
    app: &AppHandle,
    runtime: &LocalRuntimeManager,
    host_http_port: u16,
    id: &str,
) -> Result<CreativeAppSummary> {
    local::start_app(conn, app, runtime, host_http_port, id).await
}

/// Health phase of a local start, run without the mutation lock (batch 2).
pub async fn await_start_ready(
    conn: &Connection,
    app: &AppHandle,
    runtime: &LocalRuntimeManager,
    id: &str,
) -> Result<CreativeAppSummary> {
    local::await_start_ready(conn, app, runtime, id).await
}

pub async fn stop(
    conn: &Connection,
    app: &AppHandle,
    runtime: &LocalRuntimeManager,
    id: &str,
) -> Result<CreativeAppSummary> {
    local::stop_app(conn, app, runtime, id).await
}

pub async fn delete(
    conn: &Connection,
    app: &AppHandle,
    runtime: &LocalRuntimeManager,
    id: &str,
) -> Result<DeleteResult> {
    // Never delete the source project directory — only Natives metadata + logs.
    local::delete_running_app(conn, app, runtime, id).await
}

pub async fn restart(
    conn: &Connection,
    app: &AppHandle,
    runtime: &LocalRuntimeManager,
    host_http_port: u16,
    id: &str,
) -> Result<CreativeAppSummary> {
    local::restart_app(conn, app, runtime, host_http_port, id).await
}

pub fn open_target(conn: &Connection, id: &str) -> Result<OpenTarget> {
    let rec = local::get_app(conn, id)?.ok_or_else(|| Error::NotFound(id.into()))?;
    if rec.state != CreativeAppState::Running {
        return Err(Error::InvalidInput(
            "local creative app is not running".into(),
        ));
    }
    let url = rec
        .open_url
        .ok_or_else(|| Error::InvalidInput("missing openUrl".into()))?;
    validate_local_url(&url)?;
    Ok(OpenTarget::LocalUrl {
        url,
        app_id: id.to_string(),
    })
}
