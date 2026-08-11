//! Host-mediated conversation stub for FK integrity (`ensure_conversation_stub`).
//!
//! Split out of `conversation_store` to keep each file under 1000 lines (W10).
//! Shared helpers (`store`) come from the parent via `super::*`.

use super::*;

/// Ensure a conversation row exists in the daemon DB for FK integrity.
/// Host owns conversation CRUD (assistant.db); daemon only needs a stub so
/// `run` / `message` foreign keys succeed when host-mediated runs land here.
pub fn ensure_conversation_stub(
    conversation_id: &str,
    provider_id: &str,
    model_id: &str,
    permission_profile: Option<&str>,
    project_id: Option<&str>,
) -> Result<(), String> {
    let id = conversation_id.trim();
    if id.is_empty() {
        return Err("conversation_id is required".into());
    }
    let store = store()?;
    let conn = store.conn()?;
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM conversation WHERE id = ?1)",
            params![id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if exists {
        return Ok(());
    }
    let now = chrono::Utc::now().to_rfc3339();
    let permission = permission_profile
        .filter(|p| matches!(*p, "readonly" | "ask" | "full_access"))
        .unwrap_or("ask");
    let provider = if provider_id.trim().is_empty() {
        "unknown"
    } else {
        provider_id.trim()
    };
    let model = if model_id.trim().is_empty() {
        "unknown"
    } else {
        model_id.trim()
    };
    conn.execute(
        "INSERT INTO conversation (id, mode, project_id, title, provider_id, model_id, permission_profile_id, created_at, updated_at)
         VALUES (?1, 'agent', ?2, ?3, ?4, ?5, ?6, ?7, ?7)
         ON CONFLICT(id) DO NOTHING",
        params![
            id,
            project_id,
            "Host-mediated conversation",
            provider,
            model,
            permission,
            now,
        ],
    )
    .map_err(|e| {
        let path = std::env::var("NATIVES_ASSISTANT_DB_PATH")
            .or_else(|_| std::env::var("NATIVES_DB_PATH"))
            .unwrap_or_else(|_| "<unset>".into());
        format!("ensure_conversation_stub failed: {e} (db={path})")
    })?;
    Ok(())
}
