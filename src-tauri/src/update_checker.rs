use crate::{db, Error, Result};
use reqwest::{Client, StatusCode};
use rusqlite::Connection;
use semver::Version;
use serde::Deserialize;
use std::time::Duration;

// Import AppState for check_for_updates
use crate::AppState;

const MUTED_VERSIONS_KEY: &str = "update:muted_versions";
const DISMISSED_VERSIONS_KEY: &str = "update:dismissed_versions";
const GITHUB_REPO: &str = "Wcof/Natives";
const UPDATE_CHECK_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Deserialize)]
struct GithubReleaseDto {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    body: Option<String>,
}

#[derive(Debug)]
struct GithubRelease {
    version: String,
    semantic_version: Version,
    html_url: String,
    published_at: Option<String>,
    body: Option<String>,
}

/// Fetch latest GitHub release via REST API.
/// Uses unauthenticated request (60 req/hour limit).
async fn fetch_latest_github_release(client: &Client, url: &str) -> Result<Option<GithubRelease>> {
    let resp = client
        .get(url)
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await
        .map_err(|e| Error::Internal(format!("github api request failed: {e}")))?;

    if resp.status() == StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !resp.status().is_success() {
        return Err(Error::Internal(format!(
            "github api request failed with status {}",
            resp.status()
        )));
    }

    let body = resp
        .text()
        .await
        .map_err(|e| Error::Internal(format!("failed to read github response: {e}")))?;
    let body: GithubReleaseDto = serde_json::from_str(&body)
        .map_err(|e| Error::Internal(format!("failed to parse github response: {e}")))?;

    validate_release(body).map(Some)
}

fn validate_release(release: GithubReleaseDto) -> Result<GithubRelease> {
    let version = release
        .tag_name
        .strip_prefix('v')
        .unwrap_or(&release.tag_name)
        .to_string();
    let semantic_version = Version::parse(&version).map_err(|_| {
        Error::Internal(format!(
            "invalid github release tag_name: expected semantic version, got {:?}",
            release.tag_name
        ))
    })?;
    let release_url = reqwest::Url::parse(&release.html_url)
        .map_err(|e| Error::Internal(format!("invalid github release html_url: {e}")))?;
    if !matches!(release_url.scheme(), "http" | "https") {
        return Err(Error::Internal(format!(
            "invalid github release html_url scheme: {}",
            release_url.scheme()
        )));
    }

    Ok(GithubRelease {
        version,
        semantic_version,
        html_url: release.html_url,
        published_at: release.published_at,
        body: release.body,
    })
}

fn build_http_client() -> Result<Client> {
    Client::builder()
        .user_agent("Natives-Desktop-App")
        .timeout(UPDATE_CHECK_TIMEOUT)
        .build()
        .map_err(|e| Error::Internal(format!("failed to build http client: {e}")))
}

/// Check for updates by querying GitHub Releases API.
///
/// Returns update info if a newer version is available and not muted/dismissed.
pub async fn check_for_updates(state: &AppState) -> Result<serde_json::Value> {
    let current_version = env!("CARGO_PKG_VERSION").to_string();
    let current_semantic_version = Version::parse(&current_version).map_err(|_| {
        Error::Internal(format!(
            "invalid application version: expected semantic version, got {current_version:?}"
        ))
    })?;
    let url = format!("https://api.github.com/repos/{GITHUB_REPO}/releases/latest");
    let client = build_http_client()?;

    // Complete network IO before acquiring a pooled SQLite connection. The
    // connection guard below must never be held across an await point.
    let latest = match fetch_latest_github_release(&client, &url).await? {
        Some(release) => release,
        None => {
            return Ok(build_no_release_result(&current_version));
        }
    };

    // The pool guard is acquired after the last await and released immediately
    // after the two settings reads, before response construction.
    let (muted, dismissed) = {
        let pool_conn = state
            .db
            .get()
            .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
        let conn: &rusqlite::Connection = &pool_conn;
        (get_muted_versions(conn)?, get_dismissed_versions(conn)?)
    };

    Ok(build_update_result(
        &current_version,
        &current_semantic_version,
        &latest,
        &muted,
        &dismissed,
    ))
}

fn build_no_release_result(current_version: &str) -> serde_json::Value {
    serde_json::json!({
        "currentVersion": current_version,
        "latestVersion": null,
        "updateAvailable": false,
        "releaseUrl": null,
        "publishedAt": null,
        "body": null,
        "sourceConfigured": true,
        "message": "No releases found on GitHub"
    })
}

fn build_update_result(
    current_version: &str,
    current_semantic_version: &Version,
    latest: &GithubRelease,
    muted: &[String],
    dismissed: &[String],
) -> serde_json::Value {
    let tag_name = &latest.version;
    let is_newer = current_semantic_version
        .cmp_precedence(&latest.semantic_version)
        .is_lt();

    if muted.contains(tag_name) || dismissed.contains(tag_name) {
        return serde_json::json!({
            "currentVersion": current_version,
            "latestVersion": tag_name,
            "updateAvailable": false,
            "releaseUrl": latest.html_url.as_str(),
            "publishedAt": latest.published_at.as_deref(),
            "body": latest.body.as_deref(),
            "sourceConfigured": true,
            "message": "Update available but muted/dismissed"
        });
    }

    if is_newer {
        serde_json::json!({
            "currentVersion": current_version,
            "latestVersion": tag_name,
            "updateAvailable": true,
            "releaseUrl": latest.html_url.as_str(),
            "publishedAt": latest.published_at.as_deref(),
            "body": latest.body.as_deref(),
            "sourceConfigured": true,
            "message": format!("New version v{} available", tag_name)
        })
    } else {
        serde_json::json!({
            "currentVersion": current_version,
            "latestVersion": tag_name,
            "updateAvailable": false,
            "releaseUrl": latest.html_url.as_str(),
            "publishedAt": latest.published_at.as_deref(),
            "body": latest.body.as_deref(),
            "sourceConfigured": true,
            "message": "Already on the latest version"
        })
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

#[cfg(test)]
mod tests {
    use super::*;
    use tiny_http::{Response, Server};

    fn release(version: &str) -> GithubRelease {
        validate_release(GithubReleaseDto {
            tag_name: format!("v{version}"),
            html_url: format!("https://example.com/releases/{version}"),
            published_at: Some("2026-08-13T00:00:00Z".to_string()),
            body: Some("Release notes".to_string()),
        })
        .unwrap()
    }

    fn version(version: &str) -> Version {
        Version::parse(version).unwrap()
    }

    fn spawn_response(
        status: u16,
        body: impl Into<String>,
    ) -> (String, std::thread::JoinHandle<()>) {
        let body = body.into();
        let server = Server::http("127.0.0.1:0").expect("bind update mock server");
        let addr = server.server_addr().to_ip().expect("mock server ip addr");
        let url = format!("http://{}:{}/releases/latest", addr.ip(), addr.port());
        let handle = std::thread::spawn(move || {
            let request = server
                .recv_timeout(Duration::from_secs(10))
                .expect("mock server recv failed")
                .expect("mock server never received the update request");
            assert_eq!(request.url(), "/releases/latest");
            request
                .respond(Response::from_string(body).with_status_code(status))
                .expect("respond to update request");
        });
        (url, handle)
    }

    #[tokio::test]
    async fn fetch_release_parses_success_response() {
        let (url, handle) = spawn_response(
            200,
            r#"{"tag_name":"v2.0.0","html_url":"https://example.com/v2"}"#,
        );

        let result = fetch_latest_github_release(&build_http_client().unwrap(), &url)
            .await
            .unwrap();
        handle.join().unwrap();

        assert_eq!(result.unwrap().version, "2.0.0");
    }

    #[tokio::test]
    async fn fetch_release_treats_only_not_found_as_no_release() {
        let (url, handle) = spawn_response(404, "not found");

        let result = fetch_latest_github_release(&build_http_client().unwrap(), &url)
            .await
            .unwrap();
        handle.join().unwrap();

        assert!(result.is_none());
    }

    #[tokio::test]
    async fn fetch_release_surfaces_non_success_status() {
        for status in [403, 429, 500] {
            let (url, handle) = spawn_response(status, "request rejected");

            let error = fetch_latest_github_release(&build_http_client().unwrap(), &url)
                .await
                .unwrap_err();
            handle.join().unwrap();

            assert!(error.to_string().contains(&format!("status {status}")));
        }
    }

    #[tokio::test]
    async fn fetch_release_surfaces_parse_error() {
        let (url, handle) = spawn_response(200, "not json");

        let error = fetch_latest_github_release(&build_http_client().unwrap(), &url)
            .await
            .unwrap_err();
        handle.join().unwrap();

        assert!(error
            .to_string()
            .contains("failed to parse github response"));
    }

    #[tokio::test]
    async fn fetch_release_rejects_missing_required_schema() {
        let (url, handle) = spawn_response(200, r#"{"token":"not-a-real-token"}"#);

        let error = fetch_latest_github_release(&build_http_client().unwrap(), &url)
            .await
            .unwrap_err();
        handle.join().unwrap();

        assert!(error
            .to_string()
            .contains("failed to parse github response"));
        assert!(error.to_string().contains("tag_name"));
        assert!(!error.to_string().contains("not-a-real-token"));
    }

    #[tokio::test]
    async fn fetch_release_rejects_wrong_field_type() {
        let (url, handle) = spawn_response(
            200,
            r#"{"tag_name":200,"html_url":"https://example.com/release"}"#,
        );

        let error = fetch_latest_github_release(&build_http_client().unwrap(), &url)
            .await
            .unwrap_err();
        handle.join().unwrap();

        assert!(error
            .to_string()
            .contains("failed to parse github response"));
    }

    #[tokio::test]
    async fn fetch_release_rejects_invalid_semver_tag() {
        for tag_name in ["", "latest", "1.2", "01.2.3", "1.2.3.4"] {
            let body =
                format!(r#"{{"tag_name":"{tag_name}","html_url":"https://example.com/release"}}"#);
            let (url, handle) = spawn_response(200, body);

            let error = fetch_latest_github_release(&build_http_client().unwrap(), &url)
                .await
                .unwrap_err();
            handle.join().unwrap();

            assert!(
                error
                    .to_string()
                    .contains("invalid github release tag_name"),
                "tag {tag_name:?} unexpectedly produced {error}"
            );
        }
    }

    #[tokio::test]
    async fn fetch_release_surfaces_request_error() {
        let error = fetch_latest_github_release(&build_http_client().unwrap(), "not a valid URL")
            .await
            .unwrap_err();

        assert!(error.to_string().contains("github api request failed"));
    }

    #[test]
    fn update_result_reports_new_release() {
        let result = build_update_result("1.0.0", &version("1.0.0"), &release("2.0.0"), &[], &[]);

        assert_eq!(result["updateAvailable"], true);
        assert_eq!(result["latestVersion"], "2.0.0");
    }

    #[test]
    fn no_release_result_preserves_success_contract() {
        let result = build_no_release_result("1.0.0");

        assert_eq!(result["currentVersion"], "1.0.0");
        assert!(result["latestVersion"].is_null());
        assert_eq!(result["updateAvailable"], false);
        assert!(result["releaseUrl"].is_null());
        assert!(result["publishedAt"].is_null());
        assert!(result["body"].is_null());
        assert_eq!(result["sourceConfigured"], true);
        assert_eq!(result["message"], "No releases found on GitHub");
    }

    #[test]
    fn update_result_reports_current_release() {
        let result = build_update_result("2.0.0", &version("2.0.0"), &release("2.0.0"), &[], &[]);

        assert_eq!(result["updateAvailable"], false);
        assert_eq!(result["message"], "Already on the latest version");
    }

    #[test]
    fn update_result_honors_muted_version() {
        let result = build_update_result(
            "1.0.0",
            &version("1.0.0"),
            &release("2.0.0"),
            &["2.0.0".to_string()],
            &[],
        );

        assert_eq!(result["updateAvailable"], false);
        assert_eq!(result["message"], "Update available but muted/dismissed");
    }

    #[test]
    fn update_result_honors_dismissed_version() {
        let result = build_update_result(
            "1.0.0",
            &version("1.0.0"),
            &release("2.0.0"),
            &[],
            &["2.0.0".to_string()],
        );

        assert_eq!(result["updateAvailable"], false);
        assert_eq!(result["message"], "Update available but muted/dismissed");
    }

    #[test]
    fn update_result_preserves_optional_metadata_as_null() {
        let latest = validate_release(GithubReleaseDto {
            tag_name: "v2.0.0".to_string(),
            html_url: "https://example.com/releases/2.0.0".to_string(),
            published_at: None,
            body: None,
        })
        .unwrap();
        let result = build_update_result("1.0.0", &version("1.0.0"), &latest, &[], &[]);

        assert_eq!(result["releaseUrl"], "https://example.com/releases/2.0.0");
        assert!(result["publishedAt"].is_null());
        assert!(result["body"].is_null());
    }

    #[test]
    fn semantic_version_comparison_handles_prerelease_and_build_metadata() {
        assert!(version("1.0.0-alpha") < version("1.0.0"));
        assert!(version("1.0.0-alpha.2") < version("1.0.0-alpha.10"));
        assert_eq!(
            version("1.0.0+build.1").cmp_precedence(&version("1.0.0+build.2")),
            std::cmp::Ordering::Equal
        );

        let result = build_update_result(
            "1.0.0+build.1",
            &version("1.0.0+build.1"),
            &release("1.0.0+build.2"),
            &[],
            &[],
        );

        assert_eq!(result["updateAvailable"], false);
    }

    #[test]
    fn update_checker_source_contract_remains_async() {
        let checker_source = include_str!("update_checker.rs");
        let command_source = include_str!("commands/update.rs");
        let blocking_client = ["reqwest", "::blocking"].concat();
        let defaulting_schema_field = ["unwrap_or", "_default"].concat();

        assert!(!checker_source.contains(&blocking_client));
        assert!(!checker_source.contains(&defaulting_schema_field));
        assert!(checker_source.contains(".send()\n        .await"));
        assert!(command_source.contains("pub async fn update_check"));
        assert!(command_source.contains("check_for_updates(&state).await"));
    }
}
