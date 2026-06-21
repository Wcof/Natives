use crate::{db, Error, Result};
use rusqlite::Connection;
use std::time::Duration;

// Import AppState for check_for_updates
use crate::AppState;

const MUTED_VERSIONS_KEY: &str = "update:muted_versions";
const DISMISSED_VERSIONS_KEY: &str = "update:dismissed_versions";
const GITHUB_REPO: &str = "Wcof/Natives";

/// Semantic version comparison.
/// Returns: -1 if a < b, 0 if a == b, 1 if a > b
pub fn compare_versions(a: &str, b: &str) -> i8 {
    let a_parts: Vec<u64> = a
        .split('.')
        .filter_map(|s| s.parse().ok())
        .collect();
    let b_parts: Vec<u64> = b
        .split('.')
        .filter_map(|s| s.parse().ok())
        .collect();

    let max_len = a_parts.len().max(b_parts.len());
    for i in 0..max_len {
        let va = a_parts.get(i).copied().unwrap_or(0);
        let vb = b_parts.get(i).copied().unwrap_or(0);
        if va < vb {
            return -1;
        }
        if va > vb {
            return 1;
        }
    }
    0
}

/// Fetch latest GitHub release via REST API.
/// Uses unauthenticated request (60 req/hour limit).
fn fetch_latest_github_release() -> Result<Option<serde_json::Value>> {
    let url = format!("https://api.github.com/repos/{GITHUB_REPO}/releases/latest");
    let client = reqwest::blocking::Client::builder()
        .user_agent("Natives-Desktop-App")
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| Error::Internal(format!("failed to build http client: {e}")))?;

    let resp = client
        .get(&url)
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .map_err(|e| Error::Internal(format!("github api request failed: {e}")))?;

    if !resp.status().is_success() {
        return Ok(None);
    }

    let body: serde_json::Value = resp
        .json()
        .map_err(|e| Error::Internal(format!("failed to parse github response: {e}")))?;

    Ok(Some(body))
}

/// Check for updates by querying GitHub Releases API.
///
/// Returns update info if a newer version is available and not muted/dismissed.
pub fn check_for_updates(state: &AppState) -> Result<serde_json::Value> {
    let current_version = env!("CARGO_PKG_VERSION").to_string();

    // Fetch latest release from GitHub
    let latest = match fetch_latest_github_release() {
        Ok(Some(release)) => release,
        Ok(None) => {
            // No release found or API returned empty
            return Ok(serde_json::json!({
                "currentVersion": current_version,
                "latestVersion": null,
                "updateAvailable": false,
                "releaseUrl": null,
                "publishedAt": null,
                "body": null,
                "sourceConfigured": true,
                "message": "No releases found on GitHub"
            }));
        }
        Err(_) => {
            // API error — don't fake an update, be honest
            return Ok(serde_json::json!({
                "currentVersion": current_version,
                "latestVersion": null,
                "updateAvailable": false,
                "releaseUrl": null,
                "publishedAt": null,
                "body": null,
                "sourceConfigured": true,
                "message": "Failed to check for updates (API error)"
            }));
        }
    };

    // Extract release info
    let tag_name = latest
        .get("tag_name")
        .and_then(|v| v.as_str())
        .map(|s| s.trim_start_matches('v').to_string())
        .unwrap_or_default();

    let _release_name = latest
        .get("name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("v{}", tag_name));

    let html_url = latest
        .get("html_url")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_default();

    let published_at = latest
        .get("published_at")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_default();

    let body = latest
        .get("body")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_default();

    // Check if version is newer
    let is_newer = if tag_name.is_empty() {
        false
    } else {
        compare_versions(&current_version, &tag_name) < 0
    };

    // Check if version is muted or dismissed
    let pool_conn = state.db.get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &*pool_conn;

    let muted = get_muted_versions(conn)?;
    let dismissed = get_dismissed_versions(conn)?;

    if muted.contains(&tag_name) || dismissed.contains(&tag_name) {
        return Ok(serde_json::json!({
            "currentVersion": current_version,
            "latestVersion": tag_name,
            "updateAvailable": false,
            "releaseUrl": html_url,
            "publishedAt": published_at,
            "body": body,
            "sourceConfigured": true,
            "message": "Update available but muted/dismissed"
        }));
    }

    if is_newer {
        Ok(serde_json::json!({
            "currentVersion": current_version,
            "latestVersion": tag_name,
            "updateAvailable": true,
            "releaseUrl": html_url,
            "publishedAt": published_at,
            "body": body,
            "sourceConfigured": true,
            "message": format!("New version v{} available", tag_name)
        }))
    } else {
        Ok(serde_json::json!({
            "currentVersion": current_version,
            "latestVersion": tag_name,
            "updateAvailable": false,
            "releaseUrl": html_url,
            "publishedAt": published_at,
            "body": body,
            "sourceConfigured": true,
            "message": "Already on the latest version"
        }))
    }
}

/// Mute a specific version (prevent update notification)
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

/// Dismiss a specific version (close the notification permanently)
pub fn dismiss_version(conn: &Connection, version: &str) -> Result<()> {
    let mut dismissed = get_dismissed_versions(conn)?;
    if !dismissed.contains(&version.to_string()) {
        dismissed.push(version.to_string());
    }
    let serialized = serde_json::to_string(&dismissed).map_err(Error::Json)?;
    db::set_setting(conn, DISMISSED_VERSIONS_KEY, &serialized)
}

/// Get list of dismissed versions
pub fn get_dismissed_versions(conn: &Connection) -> Result<Vec<String>> {
    match db::get_setting(conn, DISMISSED_VERSIONS_KEY)? {
        Some(s) => {
            serde_json::from_str(&s).map_err(|e| Error::Internal(format!("parse error: {e}")))
        }
        None => Ok(Vec::new()),
    }
}
