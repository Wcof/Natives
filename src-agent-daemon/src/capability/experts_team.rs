//! Expert team CRUD (W9 split from capability/experts.rs).
//! `super` here is the `experts` module; helpers are re-exported from it.

use super::{
    ensure_expert_exists, expert_row_to_json, insert_expert, insert_members, now_iso,
    parse_members, required_str, str_field, validate_team_settings,
    validate_team_settings_with_defaults, EXPERT_COLS,
};
use crate::capability::store;
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use uuid::Uuid;

pub(crate) fn team_to_json(conn: &rusqlite::Connection, id: &str) -> Result<Option<Value>, String> {
    // 19.3-④: `strategy` is dead configuration (no runtime honours it) and is
    // no longer part of the contract — it is not advertised in the API. The
    // column stays in the DB untouched for historical rows.
    let team = conn
        .query_row(
            "SELECT id, name, description, failure_policy, max_concurrent,
                    coordinator_expert_id, enabled, created_at, updated_at
               FROM capability_expert_team WHERE id = ?1",
            params![id],
            |row| {
                Ok(json!({
                    "id": row.get::<_, String>(0)?,
                    "name": row.get::<_, String>(1)?,
                    "description": row.get::<_, String>(2)?,
                    "failurePolicy": row.get::<_, String>(3)?,
                    "maxConcurrent": row.get::<_, i64>(4)?,
                    "coordinatorExpertId": row.get::<_, Option<String>>(5)?,
                    "enabled": row.get::<_, i64>(6)? != 0,
                    "createdAt": row.get::<_, String>(7)?,
                    "updatedAt": row.get::<_, String>(8)?,
                }))
            },
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let Some(mut team) = team else {
        return Ok(None);
    };
    let mut stmt = conn
        .prepare(
            "SELECT expert_id, position, role_hint, task_template
               FROM capability_expert_team_member WHERE team_id = ?1 ORDER BY position",
        )
        .map_err(|e| e.to_string())?;
    let members: Vec<Value> = stmt
        .query_map(params![id], |row| {
            Ok(json!({
                "expertId": row.get::<_, String>(0)?,
                "position": row.get::<_, i64>(1)?,
                "roleHint": row.get::<_, String>(2)?,
                // `task_template` is not honoured by the runtime contract
                // (19.3-④): retained in the column for history, never advertised.
            }))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    team["members"] = json!(members);
    Ok(Some(team))
}

pub fn team_list(_params_value: &Value) -> Result<Value, String> {
    let data = store()?;
    let conn = data.conn()?;
    let mut stmt = conn
        .prepare("SELECT id FROM capability_expert_team ORDER BY name")
        .map_err(|e| e.to_string())?;
    let ids: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    drop(stmt);
    let mut teams = Vec::new();
    for id in ids {
        if let Some(team) = team_to_json(&conn, &id)? {
            teams.push(team);
        }
    }
    Ok(json!({ "teams": teams }))
}

pub fn team_get(params_value: &Value) -> Result<Value, String> {
    let id = required_str(params_value, "id")?;
    let data = store()?;
    let conn = data.conn()?;
    let team = team_to_json(&conn, id)?.ok_or_else(|| format!("team not found: {id}"))?;
    Ok(json!({ "team": team }))
}

pub fn team_create(params_value: &Value) -> Result<Value, String> {
    let name = required_str(params_value, "name")?.to_string();
    let id = params_value
        .get("id")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    // 19.3-④: strategy is fixed at the default — no runtime honours it and it
    // is no longer a configurable contract field.
    let (failure_policy, max_concurrent) = validate_team_settings(params_value)?;
    let coordinator = str_field(params_value, "coordinatorExpertId");
    let members = parse_members(params_value)?;
    if members.is_empty() {
        return Err("team needs at least one member".into());
    }
    let coordinator = match coordinator {
        Some(c) => Some(c),
        // Default lead = first member.
        None => Some(members[0].0.clone()),
    };

    let data = store()?;
    let mut conn = data.conn()?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    for (expert_id, _, _) in &members {
        ensure_expert_exists(&tx, expert_id)?;
    }
    if let Some(c) = &coordinator {
        ensure_expert_exists(&tx, c)?;
    }
    let now = now_iso();
    tx.execute(
        "INSERT INTO capability_expert_team
            (id, name, description, strategy, failure_policy, max_concurrent,
             coordinator_expert_id, enabled, created_at, updated_at)
         VALUES (?1, ?2, ?3, 'parallel', ?4, ?5, ?6, ?7, ?8, ?8)",
        params![
            id,
            name,
            str_field(params_value, "description").unwrap_or_default(),
            failure_policy,
            max_concurrent,
            coordinator,
            params_value
                .get("enabled")
                .and_then(Value::as_bool)
                .unwrap_or(true) as i64,
            now,
        ],
    )
    .map_err(|e| {
        if e.to_string().contains("UNIQUE") {
            format!("team id already exists: {id}")
        } else {
            e.to_string()
        }
    })?;
    insert_members(&tx, &id, &members)?;
    tx.commit().map_err(|e| e.to_string())?;
    let team = team_to_json(&conn, &id)?.ok_or("team vanished after insert")?;
    Ok(json!({ "team": team }))
}

pub fn team_update(params_value: &Value) -> Result<Value, String> {
    let id = required_str(params_value, "id")?.to_string();
    let data = store()?;
    let mut conn = data.conn()?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let exists: bool = tx
        .query_row(
            "SELECT 1 FROM capability_expert_team WHERE id = ?1",
            params![id],
            |_| Ok(true),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .unwrap_or(false);
    if !exists {
        return Err(format!("team not found: {id}"));
    }
    if let Some(name) = params_value.get("name").and_then(Value::as_str) {
        tx.execute(
            "UPDATE capability_expert_team SET name = ?2 WHERE id = ?1",
            params![id, name],
        )
        .map_err(|e| e.to_string())?;
    }
    if let Some(desc) = params_value.get("description").and_then(Value::as_str) {
        tx.execute(
            "UPDATE capability_expert_team SET description = ?2 WHERE id = ?1",
            params![id, desc],
        )
        .map_err(|e| e.to_string())?;
    }
    // 19.3-④: `failurePolicy`/`maxConcurrent` are the real runtime contract
    // fields; `strategy` is retired from the contract (kept at default).
    if params_value.get("failurePolicy").is_some() || params_value.get("maxConcurrent").is_some() {
        let (failure_policy, max_concurrent) =
            validate_team_settings_with_defaults(params_value, &tx, &id)?;
        tx.execute(
            "UPDATE capability_expert_team
                SET failure_policy = ?2, max_concurrent = ?3 WHERE id = ?1",
            params![id, failure_policy, max_concurrent],
        )
        .map_err(|e| e.to_string())?;
    }
    if let Some(coordinator) = params_value.get("coordinatorExpertId") {
        match coordinator.as_str() {
            Some(c) if !c.trim().is_empty() => {
                ensure_expert_exists(&tx, c)?;
                tx.execute(
                    "UPDATE capability_expert_team SET coordinator_expert_id = ?2 WHERE id = ?1",
                    params![id, c],
                )
                .map_err(|e| e.to_string())?;
            }
            _ => {
                tx.execute(
                    "UPDATE capability_expert_team SET coordinator_expert_id = NULL WHERE id = ?1",
                    params![id],
                )
                .map_err(|e| e.to_string())?;
            }
        }
    }
    if let Some(enabled) = params_value.get("enabled").and_then(Value::as_bool) {
        tx.execute(
            "UPDATE capability_expert_team SET enabled = ?2 WHERE id = ?1",
            params![id, enabled as i64],
        )
        .map_err(|e| e.to_string())?;
    }
    if params_value.get("members").is_some() {
        let members = parse_members(params_value)?;
        if members.is_empty() {
            return Err("team needs at least one member".into());
        }
        for (expert_id, _, _) in &members {
            ensure_expert_exists(&tx, expert_id)?;
        }
        tx.execute(
            "DELETE FROM capability_expert_team_member WHERE team_id = ?1",
            params![id],
        )
        .map_err(|e| e.to_string())?;
        insert_members(&tx, &id, &members)?;
    }
    tx.execute(
        "UPDATE capability_expert_team SET updated_at = ?2 WHERE id = ?1",
        params![id, now_iso()],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    let team = team_to_json(&conn, &id)?.ok_or("team vanished after update")?;
    Ok(json!({ "team": team }))
}

pub fn team_delete(params_value: &Value) -> Result<Value, String> {
    let id = required_str(params_value, "id")?;
    let data = store()?;
    let conn = data.conn()?;
    let changed = conn
        .execute(
            "DELETE FROM capability_expert_team WHERE id = ?1",
            params![id],
        )
        .map_err(|e| e.to_string())?;
    if changed == 0 {
        return Err(format!("team not found: {id}"));
    }
    Ok(json!({ "deleted": id }))
}
