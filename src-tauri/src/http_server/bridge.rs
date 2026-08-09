//! Workshop Bridge request handling and namespace routing.

use super::*;
use crate::token_manager::TokenManager;
use std::path::Path;
use tiny_http::{Request, Response};

pub(crate) fn handle_bridge_request(
    mut request: Request,
    token_manager: &TokenManager,
    csp: Header,
    db_path: &Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // CR-402: check Content-Length before reading the body (413 if exceeded).
    if let Some(cl) = get_header(&request, "Content-Length") {
        if let Ok(len) = cl.parse::<u64>() {
            if len > MAX_BRIDGE_BODY {
                let resp = Response::from_string(r#"{"error":"Request body too large"}"#)
                    .with_status_code(413)
                    .with_header(csp.clone())
                    .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
                request.respond(resp)?;
                return Ok(());
            }
        }
    }

    // Read request body with 64MB limit (CR-402: prevent memory exhaustion)
    use std::io::Read;
    let mut body = String::new();
    request
        .as_reader()
        .take(MAX_BRIDGE_BODY + 1) // +1 to detect truncation beyond the limit
        .read_to_string(&mut body)?;
    // If the body was truncated at the limit, the sender exceeded MAX_BRIDGE_BODY
    // (chunked encoding without Content-Length). Return 413.
    if body.len() as u64 > MAX_BRIDGE_BODY {
        // The request has already been consumed; we can't respond with 413 here
        // because tiny_http reads the body eagerly. The request is dropped.
        return Err("bridge body exceeded 64 MiB limit".into());
    }

    // Extract token and module ID from headers
    let token = get_header(&request, "X-Session-Token")
        .unwrap_or_default()
        .to_string();
    let module_id = get_header(&request, "X-Module-Id")
        .unwrap_or_default()
        .to_string();

    if token.is_empty() || module_id.is_empty() {
        let resp = Response::from_string(r#"{"error":"Missing token or module ID"}"#)
            .with_status_code(401)
            .with_header(csp)
            .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
        request.respond(resp)?;
        return Ok(());
    }

    // Validate token
    if !token_manager.validate(&token, &module_id) {
        let resp = Response::from_string(r#"{"error":"Invalid or expired token"}"#)
            .with_status_code(403)
            .with_header(csp)
            .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
        request.respond(resp)?;
        return Ok(());
    }

    // Parse bridge method from URL: /api/bridge/{namespace}/{method}
    let url = request.url().to_string();
    let bridge_path = url.strip_prefix("/api/bridge/").unwrap_or("");
    let mut bridge_parts = bridge_path.splitn(2, '/');
    let namespace = bridge_parts.next().unwrap_or("");
    let method = bridge_parts.next().unwrap_or("");

    // Route bridge request
    let response_body = route_bridge(namespace, method, &module_id, &body, db_path);

    let resp = Response::from_string(&response_body)
        .with_header(csp)
        .with_header(Header::from_bytes("Content-Type", "application/json").unwrap());
    request.respond(resp)?;
    Ok(())
}

pub(crate) fn route_bridge(
    namespace: &str,
    method: &str,
    module_id: &str,
    _body: &str,
    db_path: &Path,
) -> String {
    // Bridge API routing — mirrors Natives bridge-host.ts
    // Open a read-only connection to read real settings
    let conn = Connection::open(db_path).ok();

    match (namespace, method) {
        ("settings", "getTheme") => {
            // Read theme from SQLite settings, fallback to neutral default
            let theme = conn
                .as_ref()
                .and_then(|c| {
                    let mut stmt = c
                        .prepare("SELECT value FROM settings WHERE key = 'settings:theme'")
                        .ok()?;
                    stmt.query_row([], |row| row.get::<_, String>(0)).ok()
                })
                .unwrap_or_else(|| "default".to_string());
            serde_json::json!({ "result": theme }).to_string()
        }
        ("settings", "getLocale") => {
            // Read locale from SQLite settings, fallback to empty (let renderer decide default)
            let locale = conn
                .as_ref()
                .and_then(|c| {
                    let mut stmt = c
                        .prepare("SELECT value FROM settings WHERE key = 'settings:locale'")
                        .ok()?;
                    stmt.query_row([], |row| row.get::<_, String>(0)).ok()
                })
                .unwrap_or_default();
            serde_json::json!({ "result": locale }).to_string()
        }
        ("lifecycle", "ready") => {
            // Record module readiness in lifecycle tracker. A DB open/write
            // failure must surface as a structured error — never a fake
            // ok:true (P1-040).
            let Some(c) = conn.as_ref() else {
                return serde_json::json!({
                    "ok": false,
                    "error": "db unavailable: cannot open natives.db for lifecycle.ready"
                })
                .to_string();
            };
            match c.execute(
                "INSERT INTO notifications (module_id, title, body, level, created_at)
                 VALUES (?1, 'module.ready', 'Module ready', 'info', datetime('now'))",
                rusqlite::params![module_id],
            ) {
                Ok(_) => r#"{"ok":true}"#.to_string(),
                Err(e) => serde_json::json!({
                    "ok": false,
                    "error": format!("lifecycle.ready persistence failed: {e}")
                })
                .to_string(),
            }
        }
        ("lifecycle", "heartbeat") => {
            // Update heartbeat timestamp — stored in module_data for each module.
            let Some(c) = conn.as_ref() else {
                return serde_json::json!({
                    "ok": false,
                    "error": "db unavailable: cannot open natives.db for lifecycle.heartbeat"
                })
                .to_string();
            };
            let ts = chrono::Utc::now().to_rfc3339();
            match c.execute(
                "INSERT INTO module_data (module_id, key, value) VALUES (?1, '_heartbeat', ?2)
                 ON CONFLICT(module_id, key) DO UPDATE SET value = excluded.value",
                rusqlite::params![module_id, ts],
            ) {
                Ok(_) => r#"{"ok":true}"#.to_string(),
                Err(e) => serde_json::json!({
                    "ok": false,
                    "error": format!("lifecycle.heartbeat persistence failed: {e}")
                })
                .to_string(),
            }
        }
        ("lifecycle", "error") => {
            // Record error notification.
            let Some(c) = conn.as_ref() else {
                return serde_json::json!({
                    "ok": false,
                    "error": "db unavailable: cannot open natives.db for lifecycle.error"
                })
                .to_string();
            };
            match c.execute(
                "INSERT INTO notifications (module_id, title, body, level, created_at)
                 VALUES (?1, 'module.error', 'Bridge error', 'error', datetime('now'))",
                rusqlite::params![module_id],
            ) {
                Ok(_) => r#"{"ok":true}"#.to_string(),
                Err(e) => serde_json::json!({
                    "ok": false,
                    "error": format!("lifecycle.error persistence failed: {e}")
                })
                .to_string(),
            }
        }
        ("meta", "info") => {
            // Read real module version from DB, fallback to empty string (not a placeholder)
            let version = conn
                .as_ref()
                .and_then(|c| {
                    let mut stmt = c
                        .prepare("SELECT version FROM modules WHERE id = ?1")
                        .ok()?;
                    stmt.query_row(rusqlite::params![module_id], |row| row.get::<_, String>(0))
                        .ok()
                })
                .unwrap_or_default();
            let natives_version = conn
                .as_ref()
                .and_then(|c| {
                    let mut stmt = c
                        .prepare("SELECT value FROM settings WHERE key = '_app_version'")
                        .ok()?;
                    stmt.query_row([], |row| row.get::<_, String>(0)).ok()
                })
                .unwrap_or_default();
            serde_json::json!({ "moduleId": module_id, "version": version, "nativesVersion": natives_version }).to_string()
        }
        _ => serde_json::json!({ "error": format!("Unknown bridge method: {namespace}.{method}") })
            .to_string(),
    }
}
