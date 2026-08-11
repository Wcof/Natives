//! Conversation listing/queries: `list`, `listPage`, `get`, permission profile.
//!
//! Split out of `conversation_store` to keep each file under 1000 lines (W10).
//! Shared helpers (`store`, `id_param`) come from the parent via `super::*`.

use super::*;

fn row_to_conversation(row: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    Ok(serde_json::json!({
        "id": row.get::<_, String>(0)?,
        "mode": row.get::<_, String>(1)?,
        "project_id": row.get::<_, Option<String>>(2)?,
        "title": row.get::<_, String>(3)?,
        "provider_id": row.get::<_, String>(4)?,
        "model_id": row.get::<_, String>(5)?,
        "permission_profile_id": row.get::<_, Option<String>>(6)?.unwrap_or_else(|| "ask".into()),
        "created_at": row.get::<_, String>(7)?,
        "updated_at": row.get::<_, String>(8)?,
        "archived_at": row.get::<_, Option<String>>(9)?,
        "parent_conversation_id": row.get::<_, Option<String>>(10)?,
    }))
}

/// List conversations. Default hides child (subagent) conversations
/// (`parent_conversation_id IS NULL`). Pass `include_children: true` to include them.
pub(crate) fn list(params: Value) -> Result<Value, String> {
    let include_children = params
        .get("include_children")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let store = store()?;
    let conn = store.conn()?;
    let sql = if include_children {
        "SELECT id, mode, project_id, title, provider_id, model_id, permission_profile_id,
                created_at, updated_at, archived_at, parent_conversation_id
         FROM conversation ORDER BY updated_at DESC"
    } else {
        "SELECT id, mode, project_id, title, provider_id, model_id, permission_profile_id,
                created_at, updated_at, archived_at, parent_conversation_id
         FROM conversation
         WHERE parent_conversation_id IS NULL
         ORDER BY updated_at DESC"
    };
    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], row_to_conversation)
        .map_err(|e| e.to_string())?;
    Ok(Value::Array(rows.filter_map(Result::ok).collect()))
}

pub(crate) fn list_page(params: Value) -> Result<Value, String> {
    let limit = params
        .get("limit")
        .and_then(Value::as_i64)
        .unwrap_or(100)
        .clamp(20, 200);
    let cursor = params.get("cursor");
    let cursor_updated = cursor
        .and_then(|v| v.get("updatedAt").or_else(|| v.get("updated_at")))
        .and_then(Value::as_str);
    let cursor_id = cursor.and_then(|v| v.get("id")).and_then(Value::as_str);
    let store = store()?;
    let conn = store.conn()?;
    let mut stmt = conn
        .prepare(
            "SELECT c.id, c.mode, c.project_id, c.title, c.provider_id, c.model_id,
                    c.permission_profile_id, c.created_at, c.updated_at, c.archived_at,
                    c.parent_conversation_id,
                    (SELECT r.status FROM run r
                      WHERE r.conversation_id = c.id
                      ORDER BY r.created_at DESC, r.id DESC LIMIT 1) AS last_run_status
             FROM conversation c
             WHERE c.parent_conversation_id IS NULL
               AND (?1 IS NULL OR c.updated_at < ?1 OR (c.updated_at = ?1 AND c.id < ?2))
             ORDER BY c.updated_at DESC, c.id DESC LIMIT ?3",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(
            params![cursor_updated, cursor_id, limit + 1],
            |row| {
                Ok(serde_json::json!({
                    "id": row.get::<_, String>(0)?,
                    "mode": row.get::<_, String>(1)?,
                    "project_id": row.get::<_, Option<String>>(2)?,
                    "title": row.get::<_, String>(3)?,
                    "provider_id": row.get::<_, String>(4)?,
                    "model_id": row.get::<_, String>(5)?,
                    "permission_profile_id": row.get::<_, Option<String>>(6)?.unwrap_or_else(|| "ask".into()),
                    "created_at": row.get::<_, String>(7)?,
                    "updated_at": row.get::<_, String>(8)?,
                    "archived_at": row.get::<_, Option<String>>(9)?,
                    "parent_conversation_id": row.get::<_, Option<String>>(10)?,
                    // W8: low-frequency activity projection — the most recent
                    // run status per conversation (never the live stream).
                    "last_run_status": row.get::<_, Option<String>>(11)?,
                }))
            },
        )
        .map_err(|e| e.to_string())?;
    let mut conversations: Vec<Value> = rows.filter_map(Result::ok).collect();
    let has_more = conversations.len() > limit as usize;
    conversations.truncate(limit as usize);
    let next_cursor = has_more
        .then(|| conversations.last())
        .flatten()
        .and_then(|row| {
            Some(serde_json::json!({
                "updatedAt": row.get("updated_at")?, "id": row.get("id")?
            }))
        });
    Ok(serde_json::json!({ "conversations": conversations, "nextCursor": next_cursor }))
}

pub(crate) fn get(params: Value) -> Result<Value, String> {
    let id = id_param(&params)?;
    let store = store()?;
    let conn = store.conn()?;
    conn.query_row(
        "SELECT id, mode, project_id, title, provider_id, model_id, permission_profile_id,
                created_at, updated_at, archived_at, parent_conversation_id
         FROM conversation WHERE id = ?1",
        params![id],
        row_to_conversation,
    )
    .optional()
    .map_err(|e| e.to_string())?
    .ok_or_else(|| "conversation not found".into())
}

pub fn permission_profile(conversation_id: &str) -> Result<String, String> {
    let store = store()?;
    let conn = store.conn()?;
    conn.query_row(
        "SELECT COALESCE(permission_profile_id, 'ask') FROM conversation WHERE id = ?1",
        params![conversation_id],
        |row| row.get(0),
    )
    .optional()
    .map_err(|e| e.to_string())?
    .ok_or_else(|| "conversation not found".into())
}
