//! Artifact list and OS open/reveal actions.
//!
//! `open`/`reveal` refuse arbitrary caller paths: the target must canonicalize to a
//! path registered on a Daemon artifact row. The Host no longer falls back to the
//! historical `assistant_artifacts` mirror (MIG-004 / DATA-002). Full
//! ProjectIdentity binding lands after task-10 integration.
use crate::daemon_authority;
use serde_json::Value;
use std::path::{Path, PathBuf};

use super::{error_response, success_response, RpcResponse};

pub(crate) async fn handle_artifact_list(params: &Value) -> RpcResponse {
    match daemon_authority::request("artifact.list", params.clone()).await {
        Ok(data) => success_response(data),
        Err(error) => error_response("DAEMON_ARTIFACT_FAILED", &error),
    }
}

pub(crate) async fn handle_artifact_open(params: &Value) -> RpcResponse {
    let path = match params.get("path").and_then(|v| v.as_str()) {
        Some(p) if !p.trim().is_empty() => p.trim(),
        _ => return error_response("MISSING_PARAM", "path is required"),
    };

    let canonical = match canonicalize_existing(Path::new(path)) {
        Ok(p) => p,
        Err(e) => return error_response("INVALID_PATH", &e),
    };

    if !path_is_registered_artifact(&canonical).await {
        return error_response(
            "ARTIFACT_PATH_DENIED",
            "path is not a registered artifact (arbitrary open refused)",
        );
    }

    if let Err(e) = open::that(&canonical) {
        return error_response("OPEN_ERROR", &e.to_string());
    }
    success_response(serde_json::json!({ "opened": canonical.display().to_string() }))
}

fn canonicalize_existing(path: &Path) -> Result<PathBuf, String> {
    std::fs::canonicalize(path).map_err(|e| format!("cannot resolve path: {e}"))
}

/// Daemon authority is the only registration source for artifact paths.
async fn path_is_registered_artifact(canonical: &Path) -> bool {
    match daemon_authority::request("artifact.list", serde_json::json!({})).await {
        Ok(data) => artifact_json_contains_path(&data, canonical),
        Err(_) => false,
    }
}

fn artifact_json_contains_path(data: &Value, canonical: &Path) -> bool {
    let items = data
        .as_array()
        .cloned()
        .or_else(|| data.get("artifacts").and_then(|v| v.as_array()).cloned())
        .unwrap_or_default();
    for item in items {
        let Some(path) = item.get("path").and_then(|v| v.as_str()) else {
            continue;
        };
        if let Ok(p) = canonicalize_existing(Path::new(path)) {
            if p == canonical {
                return true;
            }
        } else if Path::new(path) == canonical {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod artifact_path_tests {
    use super::*;

    #[tokio::test]
    async fn arbitrary_path_is_denied_when_daemon_unavailable() {
        // No daemon authority configured in this unit test → registration check
        // fails closed, so any path is denied (never a stale mirror read).
        let escape = std::env::temp_dir().join(format!(
            "natives-artifact-escape-{}.txt",
            uuid::Uuid::new_v4()
        ));
        std::fs::write(&escape, "secret").unwrap();
        let resp =
            handle_artifact_open(&serde_json::json!({ "path": escape.to_string_lossy() })).await;
        assert!(!resp.success, "arbitrary path must be denied: {:?}", resp);
        assert_eq!(
            resp.error.as_ref().map(|e| e.code.as_str()),
            Some("ARTIFACT_PATH_DENIED")
        );
        let _ = std::fs::remove_file(escape);
    }

    #[test]
    fn artifact_json_matches_canonical_path() {
        let path = std::env::temp_dir().join(format!(
            "natives-artifact-json-{}.txt",
            uuid::Uuid::new_v4()
        ));
        std::fs::write(&path, "x").unwrap();
        let canon = std::fs::canonicalize(&path).unwrap();
        let data = serde_json::json!([{ "path": canon.display().to_string() }]);
        assert!(artifact_json_contains_path(&data, &canon));
        let other = std::env::temp_dir().join("natives-artifact-missing-xyz");
        assert!(!artifact_json_contains_path(&data, Path::new(&other)));
        let _ = std::fs::remove_file(path);
    }
}
