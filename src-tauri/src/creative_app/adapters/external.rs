//! External GitHub container adapter — Docker CLI only, no SDK.
//!
//! Docker actual state is the authority (reconciled by install::); DB holds
//! expected state + last op. Open surface is child WebView on 127.0.0.1 only.
//! Workshop Bridge is never attached.

use crate::creative_app::install;
use crate::creative_app::model::*;
use crate::creative_app::service::validate_local_url;
use crate::creative_app::store;
use crate::{Error, Result};
use rusqlite::Connection;
use tauri::AppHandle;

pub fn list(conn: &Connection) -> Result<Vec<CreativeAppSummary>> {
    let recs = store::list_apps(conn)?;
    Ok(recs.iter().map(install::summary_from_external).collect())
}

pub fn get(conn: &Connection, id: &str) -> Result<CreativeAppSummary> {
    let rec = store::get_app(conn, id)?.ok_or_else(|| Error::NotFound(id.into()))?;
    Ok(install::summary_from_external(&rec))
}

pub async fn start(conn: &Connection, app: &AppHandle, id: &str) -> Result<CreativeAppSummary> {
    install::start_app(conn, app, id).await
}

pub async fn stop(conn: &Connection, app: &AppHandle, id: &str) -> Result<CreativeAppSummary> {
    install::stop_app(conn, app, id).await
}

pub async fn delete(
    conn: &Connection,
    app: &AppHandle,
    id: &str,
    opts: DeleteOptions,
) -> Result<DeleteResult> {
    install::delete_app(conn, app, id, opts).await
}

pub fn open_target(conn: &Connection, id: &str) -> Result<OpenTarget> {
    let rec = store::get_app(conn, id)?.ok_or_else(|| Error::NotFound(id.into()))?;
    if rec.state != CreativeAppState::Running {
        return Err(Error::InvalidInput("external app is not running".into()));
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
