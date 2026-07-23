//! Artifact list and OS open/reveal actions.
use crate::daemon::data::DataStore;
use crate::daemon_authority;
use serde_json::Value;
use std::sync::Arc;

use super::{error_response, success_response, RpcResponse};

pub(crate) async fn handle_artifact_list(data_store: &Arc<DataStore>, params: &Value) -> RpcResponse {
    if daemon_authority::authority_mode_label() == "uds" {
        return match daemon_authority::request("artifact.list", params.clone()).await {
            Ok(data) => success_response(data),
            Err(error) => error_response("DAEMON_ARTIFACT_FAILED", &error),
        };
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

pub(crate) async fn handle_artifact_open(_data_store: &Arc<DataStore>, params: &Value) -> RpcResponse {
    let path = match params.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return error_response("MISSING_PARAM", "path is required"),
    };

    // Open artifact with system default application
    if let Err(e) = open::that(path) {
        return error_response("OPEN_ERROR", &e.to_string());
    }
    success_response(serde_json::json!({ "opened": path }))
}


