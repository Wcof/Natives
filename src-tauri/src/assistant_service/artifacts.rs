//! Artifact list and OS open/reveal actions.
//!
//! `open`/`reveal` refuse arbitrary caller paths: the target must canonicalize to a
//! path registered on an artifact row (Daemon authority or host mirror). Full
//! ProjectIdentity binding lands after task-10 integration.
use crate::daemon::data::DataStore;
use crate::daemon_authority;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::{error_response, success_response, RpcResponse};

pub(crate) async fn handle_artifact_list(
    data_store: &Arc<DataStore>,
    params: &Value,
) -> RpcResponse {
    if daemon_authority::authority_mode_label() == "uds" {
        return match daemon_authority::request("artifact.list", params.clone()).await {
            Ok(data) => success_response(data),
            Err(error) => error_response("DAEMON_ARTIFACT_FAILED", &error),
        };
    }

    // Prefer Daemon even in embedded — host mirror is fallback only.
    if let Ok(data) = daemon_authority::request("artifact.list", params.clone()).await {
        return success_response(data);
    }

    let conversation_id = params.get("conversation_id").and_then(|v| v.as_str());
    let run_id = params.get("run_id").and_then(|v| v.as_str());

    if let Some(run_id) = run_id {
        let conn = data_store.conn();
        let mut stmt = match conn.prepare(
            "SELECT id, conversation_id, run_id, path, mime_type, created_at, label, kind, size FROM assistant_artifacts WHERE run_id = ?1 ORDER BY created_at DESC"
        ) {
            Ok(s) => s,
            Err(e) => return error_response("DB_ERROR", &e.to_string()),
        };
        let rows = match stmt.query_map(rusqlite::params![run_id], row_to_artifact) {
            Ok(r) => r,
            Err(e) => return error_response("DB_QUERY_ERROR", &e.to_string()),
        };
        success_response(serde_json::json!(rows
            .filter_map(|row| row.ok())
            .collect::<Vec<Value>>()))
    } else if let Some(cid) = conversation_id {
        let conn = data_store.conn();
        let mut stmt = match conn.prepare(
            "SELECT id, conversation_id, run_id, path, mime_type, created_at, label, kind, size FROM assistant_artifacts WHERE conversation_id = ?1 ORDER BY created_at DESC"
        ) {
            Ok(s) => s,
            Err(e) => return error_response("DB_ERROR", &e.to_string()),
        };
        let rows = match stmt.query_map(rusqlite::params![cid], row_to_artifact) {
            Ok(r) => r,
            Err(e) => return error_response("DB_QUERY_ERROR", &e.to_string()),
        };
        let collected: Vec<Value> = rows.filter_map(|r| r.ok()).collect();
        drop(stmt);
        drop(conn);
        success_response(serde_json::json!(collected))
    } else {
        let conn = data_store.conn();
        let mut stmt = match conn.prepare(
            "SELECT id, conversation_id, run_id, path, mime_type, created_at, label, kind, size FROM assistant_artifacts ORDER BY created_at DESC"
        ) {
            Ok(s) => s,
            Err(e) => return error_response("DB_ERROR", &e.to_string()),
        };
        let rows = match stmt.query_map([], row_to_artifact) {
            Ok(r) => r,
            Err(e) => return error_response("DB_QUERY_ERROR", &e.to_string()),
        };
        let collected: Vec<Value> = rows.filter_map(|r| r.ok()).collect();
        drop(stmt);
        drop(conn);
        success_response(serde_json::json!(collected))
    }
}

fn row_to_artifact(row: &rusqlite::Row) -> rusqlite::Result<Value> {
    Ok(serde_json::json!({
        "id": row.get::<_, String>(0)?,
        "conversation_id": row.get::<_, String>(1)?,
        "run_id": row.get::<_, String>(2)?,
        "path": row.get::<_, String>(3)?,
        "mime_type": row.get::<_, String>(4)?,
        "created_at": row.get::<_, String>(5)?,
        "label": row.get::<_, Option<String>>(6)?,
        "kind": row.get::<_, String>(7)?,
        "size": row.get::<_, i64>(8)?
    }))
}

pub(crate) async fn handle_artifact_open(
    data_store: &Arc<DataStore>,
    params: &Value,
) -> RpcResponse {
    let path = match params.get("path").and_then(|v| v.as_str()) {
        Some(p) if !p.trim().is_empty() => p.trim(),
        _ => return error_response("MISSING_PARAM", "path is required"),
    };

    let canonical = match canonicalize_existing(Path::new(path)) {
        Ok(p) => p,
        Err(e) => return error_response("INVALID_PATH", &e),
    };

    if !path_is_registered_artifact(data_store, &canonical).await {
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

async fn path_is_registered_artifact(data_store: &Arc<DataStore>, canonical: &Path) -> bool {
    // 1) Daemon artifact.list
    if let Ok(data) = daemon_authority::request("artifact.list", serde_json::json!({})).await {
        if artifact_json_contains_path(&data, canonical) {
            return true;
        }
    }
    // 2) Host mirror fallback
    let conn = data_store.conn();
    let mut stmt = match conn.prepare("SELECT path FROM assistant_artifacts") {
        Ok(s) => s,
        Err(_) => return false,
    };
    let rows = match stmt.query_map([], |row| row.get::<_, String>(0)) {
        Ok(r) => r,
        Err(_) => return false,
    };
    for path in rows.flatten() {
        if let Ok(p) = canonicalize_existing(Path::new(&path)) {
            if p == canonical {
                return true;
            }
        } else if Path::new(&path) == canonical {
            return true;
        }
    }
    false
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
    use std::sync::Arc;

    #[tokio::test]
    async fn arbitrary_path_is_denied() {
        let store = Arc::new(DataStore::new(":memory:").unwrap());
        let escape = std::env::temp_dir().join(format!(
            "natives-artifact-escape-{}.txt",
            uuid::Uuid::new_v4()
        ));
        std::fs::write(&escape, "secret").unwrap();
        let resp = handle_artifact_open(
            &store,
            &serde_json::json!({ "path": escape.to_string_lossy() }),
        )
        .await;
        assert!(!resp.success, "arbitrary path must be denied: {:?}", resp);
        assert_eq!(
            resp.error.as_ref().map(|e| e.code.as_str()),
            Some("ARTIFACT_PATH_DENIED")
        );
        let _ = std::fs::remove_file(escape);
    }

    #[tokio::test]
    async fn registered_mirror_path_is_allowed_to_resolve() {
        let store = Arc::new(DataStore::new(":memory:").unwrap());
        store
            .conn()
            .execute_batch("PRAGMA foreign_keys = OFF;")
            .unwrap();
        let path =
            std::env::temp_dir().join(format!("natives-artifact-ok-{}.txt", uuid::Uuid::new_v4()));
        std::fs::write(&path, "ok").unwrap();
        let canon = std::fs::canonicalize(&path).unwrap();
        store
            .conn()
            .execute(
                "INSERT INTO assistant_artifacts
                 (id, conversation_id, run_id, source_tool, path, sha256, size, mime_type, label, kind, created_at)
                 VALUES ('a1', 'c1', 'r1', 'test', ?1, 'deadbeef', 2, 'text/plain', NULL, 'file', datetime('now'))",
                rusqlite::params![canon.display().to_string()],
            )
            .unwrap();
        assert!(path_is_registered_artifact(&store, &canon).await);
        let _ = std::fs::remove_file(path);
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
