//! W8 bounded full-conversation message search (never the Sidebar title).
//!
//! Split out of `conversation_store` to keep each file under 1000 lines (W10).
//! Shared helpers (`store`, `required_str`) come from the parent via `super::*`.

use super::*;

/// W8: bounded full-conversation message search (never the Sidebar title).
///
/// Searches the typed transcript (`message` text blocks) of one conversation
/// with `LIKE ?` on the escaped query, ordered by `(created_at, id)` so the
/// result is stable for cursor paging. Returns at most `limit` rows with a
/// text snippet; the caller uses `message_id` to locate/scroll/focus.
///
/// Params: `conversation_id`, `q`, `limit` (default 20, clamp 1..=100),
/// `cursor` (optional `last_message_id` for the next page).
pub fn search_messages(params: Value) -> Result<Value, String> {
    let conversation_id = required_str(&params, "conversation_id")?;
    let q = params
        .get("q")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "q is required".to_string())?;
    let limit = params
        .get("limit")
        .and_then(Value::as_i64)
        .unwrap_or(20)
        .clamp(1, 100);
    let cursor = params
        .get("cursor")
        .and_then(Value::as_str)
        .filter(|c| !c.is_empty());

    let store = store()?;
    let conn = store.conn()?;
    // Escape LIKE wildcards so user input is matched literally.
    let escaped = q
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    let pattern = format!("%{escaped}%");

    let mut stmt = conn
        .prepare(
            "SELECT m.id, m.role, m.created_at,
                    COALESCE((SELECT block_json FROM message_block mb
                              WHERE mb.message_id = m.id AND mb.block_type = 'text'
                              ORDER BY mb.sort_order ASC, mb.id ASC LIMIT 1), '')
             FROM message m
             WHERE m.conversation_id = ?1
               AND EXISTS (
                 SELECT 1 FROM message_block mb
                 WHERE mb.message_id = m.id
                   AND mb.block_type = 'text'
                   AND mb.block_json LIKE ?2 ESCAPE '\\'
               )
               AND (?3 IS NULL OR (m.created_at, m.id) > (
                   SELECT created_at, id FROM message
                   WHERE id = ?3
                 ))
             ORDER BY m.created_at ASC, m.id ASC
             LIMIT ?4",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(
            params![conversation_id, pattern, cursor, limit + 1],
            |row| {
                Ok(serde_json::json!({
                    "message_id": row.get::<_, String>(0)?,
                    "role": row.get::<_, String>(1)?,
                    "created_at": row.get::<_, String>(2)?,
                    "snippet": row.get::<_, String>(3)?,
                }))
            },
        )
        .map_err(|e| e.to_string())?;
    let mut results: Vec<Value> = rows.filter_map(Result::ok).collect();
    let has_more = results.len() > limit as usize;
    results.truncate(limit as usize);
    Ok(serde_json::json!({
        "results": results,
        "hasMore": has_more,
        "nextCursor": has_more
            .then(|| results.last())
            .flatten()
            .and_then(|r| r.get("message_id"))
            .and_then(Value::as_str)
            .map(str::to_string),
    }))
}
