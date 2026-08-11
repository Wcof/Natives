//! Conversation CRUD mutations: create / rename / update model / update
//! permission / archive. Split out of `conversation_store` to keep each file
//! under 1000 lines (W10). Shared helpers (`store`, `required_str`, `id_param`)
//! come from the parent via `super::*`.

use super::*;

pub(crate) fn create(params: Value) -> Result<Value, String> {
    let mode = params
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("agent");
    if !matches!(mode, "chat" | "agent" | "goal") {
        return Err("mode must be chat, agent, or goal".into());
    }
    let title = required_str(&params, "title")?;
    let provider_id = required_str(&params, "provider_id")?;
    let model_id = required_str(&params, "model_id")?;
    let permission = params
        .get("permission_profile_id")
        .and_then(Value::as_str)
        .unwrap_or("ask");
    if !matches!(permission, "readonly" | "ask" | "full_access") {
        return Err("permission_profile_id must be readonly, ask, or full_access".into());
    }
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let project_id = required_str(&params, "project_id")?;
    let store = store()?;
    let conn = store.conn()?;
    conn.execute(
        "INSERT INTO conversation (id, mode, project_id, title, provider_id, model_id, permission_profile_id, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
        params![id, mode, project_id, title, provider_id, model_id, permission, now],
    )
    .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "id": id,
        "mode": mode,
        "project_id": project_id,
        "title": title,
        "provider_id": provider_id,
        "model_id": model_id,
        "permission_profile_id": permission,
        "created_at": now,
        "updated_at": now,
        "archived_at": null,
    }))
}

pub(crate) fn rename(params: Value) -> Result<Value, String> {
    let id = id_param(&params)?;
    let title = required_str(&params, "title")?;
    let now = chrono::Utc::now().to_rfc3339();
    exec_update(
        "UPDATE conversation SET title = ?1, updated_at = ?2 WHERE id = ?3",
        params![title, now, id],
    )?;
    Ok(serde_json::json!({ "id": id, "title": title }))
}

pub(crate) fn update_model(params: Value) -> Result<Value, String> {
    let id = id_param(&params)?;
    let provider_id = required_str(&params, "provider_id")?;
    let model_id = required_str(&params, "model_id")?;
    let now = chrono::Utc::now().to_rfc3339();
    exec_update(
        "UPDATE conversation SET provider_id = ?1, model_id = ?2, updated_at = ?3 WHERE id = ?4",
        params![provider_id, model_id, now, id],
    )?;
    Ok(serde_json::json!({ "id": id, "updated_at": now }))
}

pub(crate) fn update_permission(params: Value) -> Result<Value, String> {
    let id = id_param(&params)?;
    let profile = required_str(&params, "permission_profile_id")?;
    if !matches!(profile, "readonly" | "ask" | "full_access") {
        return Err("permission_profile_id must be readonly, ask, or full_access".into());
    }
    let now = chrono::Utc::now().to_rfc3339();
    exec_update(
        "UPDATE conversation SET permission_profile_id = ?1, updated_at = ?2 WHERE id = ?3",
        params![profile, now, id],
    )?;
    Ok(serde_json::json!({ "id": id, "permission_profile_id": profile, "updated_at": now }))
}

pub(crate) fn archive(params: Value) -> Result<Value, String> {
    let id = id_param(&params)?;
    let now = chrono::Utc::now().to_rfc3339();
    exec_update(
        "UPDATE conversation SET archived_at = ?1, updated_at = ?1 WHERE id = ?2",
        params![now, id],
    )?;
    Ok(serde_json::json!({ "id": id, "archived_at": true }))
}

fn exec_update(sql: &str, params: impl rusqlite::Params) -> Result<(), String> {
    let store = store()?;
    let conn = store.conn()?;
    match conn.execute(sql, params).map_err(|e| e.to_string())? {
        0 => Err("conversation not found".into()),
        _ => Ok(()),
    }
}
