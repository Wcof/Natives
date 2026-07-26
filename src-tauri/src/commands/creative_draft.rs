//! Creative draft commands — the host side of the creation loop.
//!
//! Publishing is deliberately a host command rather than a model tool
//! (ADR-0014 section 9). The user clicking "save as personal creation" *is* the
//! authorization; the model never gets a path to the real module directory.

use crate::creative_draft::store;
use crate::{emit_db_state_changed, AppState, Error, Result};
use serde_json::Value as JsonValue;
use tauri::State;

fn natives_dir() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".natives")
}

fn modules_dir() -> std::path::PathBuf {
    natives_dir().join("modules")
}

/// Fresh draft ids are host-generated so they always satisfy `validate_draft_id`.
fn new_draft_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let salt: u32 = rand::random();
    format!("draft-{millis:x}-{salt:x}")
}

#[tauri::command]
pub fn create_creative_draft(
    name: String,
    intent: String,
    conversation_id: Option<String>,
    origin_module_id: Option<String>,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<JsonValue> {
    let pool_conn = state
        .db
        .get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &*pool_conn;

    // 种子读取放在 INSERT 之前：读失败时不得留下无事件、UI 不可见的孤儿草稿行。
    // 同时校验 origin_module_id 形状——它拼进文件路径，不能包含路径分隔符。
    let seed_html: Option<String> = match origin_module_id.as_deref() {
        Some(module_id) => {
            let valid = !module_id.is_empty()
                && module_id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
            if !valid {
                return Err(Error::InvalidInput(format!(
                    "invalid origin module id: {module_id}"
                )));
            }
            let seed = modules_dir().join(module_id).join("index.html");
            Some(std::fs::read_to_string(&seed).map_err(|e| {
                Error::NotFound(format!("cannot seed draft from module {module_id}: {e}"))
            })?)
        }
        None => None,
    };

    let draft_id = new_draft_id();
    let draft = store::create_draft(
        conn,
        &draft_id,
        &name,
        &intent,
        conversation_id.as_deref(),
        origin_module_id.as_deref(),
    )?;

    // Continuing an existing module seeds rev-1 with what is currently live, so
    // the model edits the real thing instead of regenerating from scratch.
    if let Some(html) = seed_html {
        store::append_revision(conn, &natives_dir(), &draft_id, &html)?;
    }

    emit_db_state_changed(
        &app_handle,
        "creative-draft",
        serde_json::json!({ "action": "created", "draftId": draft_id }),
    );
    Ok(serde_json::to_value(store::get_draft(conn, &draft.draft_id)?)?)
}

#[tauri::command]
pub fn list_creative_drafts(state: State<'_, AppState>) -> Result<JsonValue> {
    let pool_conn = state
        .db
        .get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    Ok(serde_json::to_value(store::list_drafts(&pool_conn)?)?)
}

#[tauri::command]
pub fn get_creative_draft(draft_id: String, state: State<'_, AppState>) -> Result<JsonValue> {
    let pool_conn = state
        .db
        .get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    Ok(serde_json::to_value(store::get_draft(&pool_conn, &draft_id)?)?)
}

/// Link a draft to the conversation editing it, once that conversation exists.
#[tauri::command]
pub fn bind_creative_draft_conversation(
    draft_id: String,
    conversation_id: String,
    state: State<'_, AppState>,
) -> Result<JsonValue> {
    let pool_conn = state
        .db
        .get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    store::bind_conversation(&pool_conn, &draft_id, &conversation_id)?;
    Ok(serde_json::json!({ "ok": true, "draftId": draft_id }))
}

/// Read the revision the draft currently points at — used by the preview pane
/// and by "continue creating" to show what is on screen.
#[tauri::command]
pub fn read_creative_draft(draft_id: String, state: State<'_, AppState>) -> Result<JsonValue> {
    let pool_conn = state
        .db
        .get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let html = store::read_current(&pool_conn, &natives_dir(), &draft_id)?;
    Ok(serde_json::json!({ "draftId": draft_id, "html": html }))
}

/// Step the revision pointer back one — the user-facing "undo last change".
#[tauri::command]
pub fn rollback_creative_draft(
    draft_id: String,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<JsonValue> {
    let pool_conn = state
        .db
        .get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let revision = store::rollback(&pool_conn, &draft_id)?;
    emit_db_state_changed(
        &app_handle,
        "creative-draft",
        serde_json::json!({ "action": "rolled_back", "draftId": draft_id }),
    );
    Ok(serde_json::json!({ "draftId": draft_id, "revision": revision }))
}

/// Publish a draft as a real Workshop module.
///
/// The publish logic itself lives in `store::publish` so its failure path stays
/// testable; this command only unwraps state and broadcasts.
#[tauri::command]
pub fn publish_creative_draft(
    draft_id: String,
    module_id: String,
    name: String,
    permissions: Vec<String>,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<JsonValue> {
    let pool_conn = state
        .db
        .get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &*pool_conn;

    let outcome = store::publish(
        conn,
        &natives_dir(),
        &modules_dir(),
        &draft_id,
        &module_id,
        &name,
        &permissions,
    )?;

    emit_db_state_changed(
        &app_handle,
        "module",
        serde_json::json!({ "action": "generated", "moduleId": module_id }),
    );
    emit_db_state_changed(
        &app_handle,
        "creative-draft",
        serde_json::json!({ "action": "published", "draftId": draft_id, "moduleId": module_id }),
    );

    Ok(serde_json::json!({
        "ok": true,
        "draftId": draft_id,
        "moduleId": module_id,
        "contractId": outcome.contract_id,
        "contentHash": outcome.content_hash,
        "oldContent": outcome.old_content,
    }))
}

#[tauri::command]
pub fn delete_creative_draft(
    draft_id: String,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<JsonValue> {
    let pool_conn = state
        .db
        .get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    store::delete_draft(&pool_conn, &natives_dir(), &draft_id)?;
    emit_db_state_changed(
        &app_handle,
        "creative-draft",
        serde_json::json!({ "action": "deleted", "draftId": draft_id }),
    );
    Ok(serde_json::json!({ "ok": true, "draftId": draft_id }))
}
