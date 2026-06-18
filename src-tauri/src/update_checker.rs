use crate::{db, Error, Result};
use rusqlite::Connection;

const MUTED_VERSIONS_KEY: &str = "update:muted_versions";

/// Check for updates (stub — returns current version info)
pub fn check_for_updates() -> Result<serde_json::Value> {
    let current_version = env!("CARGO_PKG_VERSION");
    Ok(serde_json::json!({
        "currentVersion": current_version,
        "latestVersion": current_version, // TODO: check GitHub releases
        "updateAvailable": false,
        "releaseUrl": null,
    }))
}

/// Mute a specific version
pub fn mute_version(conn: &Connection, version: &str) -> Result<()> {
    let mut muted = get_muted_versions(conn)?;
    if !muted.contains(&version.to_string()) {
        muted.push(version.to_string());
    }
    let serialized = serde_json::to_string(&muted).map_err(Error::Json)?;
    db::set_setting(conn, MUTED_VERSIONS_KEY, &serialized)
}

/// Get list of muted versions
pub fn get_muted_versions(conn: &Connection) -> Result<Vec<String>> {
    match db::get_setting(conn, MUTED_VERSIONS_KEY)? {
        Some(s) => {
            serde_json::from_str(&s).map_err(|e| Error::Internal(format!("parse error: {e}")))
        }
        None => Ok(Vec::new()),
    }
}
