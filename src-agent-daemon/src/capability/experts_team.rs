//! Expert team CRUD (W9 split from capability/experts.rs).
//! `super` here is the `experts` module; helpers are re-exported from it.

#[cfg_attr(not(test), allow(unused_imports))]
use super::{
    ensure_expert_exists, expert_row_to_json, expert_tool_ids, insert_expert, insert_members,
    now_iso, parse_members, required_str, str_field, validate_team_settings,
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

    // 审计收口 #11：fail-closed——coordinator 必须属于 roster；
    // lead 的有效工具必须包含 `task`（否则配置可保存成功，但模型根本看不到
    // task schema，Team 运行必失败）。成员有效且无重复。
    let member_ids: std::collections::HashSet<&str> =
        members.iter().map(|(id, _, _)| id.as_str()).collect();
    if member_ids.len() != members.len() {
        return Err("team members must be unique".into());
    }
    if let Some(c) = &coordinator {
        if !member_ids.contains(c.as_str()) {
            return Err(format!(
                "coordinatorExpertId {c} must be one of the team members (roster)"
            ));
        }
        // lead 的有效工具必须包含 task（真 subagent primitive，不造 DAG 引擎）。
        let lead_tools = expert_tool_ids(c)?;
        if !lead_tools.iter().any(|t| t == "task") {
            return Err(format!(
                "coordinator expert {c} must have the `task` tool in its effective tools"
            ));
        }
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    // insert_expert and now_iso are private in the parent experts module;
    // super::* brings them in through the `use super::{...}` above.

    fn with_temp_db<F: FnOnce()>(f: F) {
        let _guard = crate::storage::DataStore::env_test_lock();
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join(format!("team-{}.db", uuid::Uuid::new_v4()));
        let art = dir.path().join("artifacts");
        crate::storage::set_test_db_override(Some(db.clone()), Some(art.clone()));
        let _warm = crate::storage::DataStore::new(&db, &art).expect("team temp db migrate");
        f();
        crate::storage::set_test_db_override(None, None);
    }

    fn make_experts() {
        // 审计收口 #11：coordinator 必须属于 roster 且有效工具含 `task`——
        // fixture 的 lead expert 需显式声明 task 工具。
        insert_expert(
            &serde_json::json!({
                "id": "lead", "name": "Lead", "systemPrompt": "Lead.",
                "tools": ["task"]
            }),
            "manual",
        )
        .unwrap();
        insert_expert(
            &serde_json::json!({"id": "member", "name": "Member", "systemPrompt": "Member."}),
            "manual",
        )
        .unwrap();
    }

    /// §19.3: the team JSON never advertises `strategy` or `taskTemplate` —
    /// both are retired from the API contract (columns kept for history).
    #[test]
    fn team_json_hides_strategy_and_task_template() {
        with_temp_db(|| {
            make_experts();
            let created = team_create(&serde_json::json!({
                "id": "team-hidden",
                "name": "Hidden",
                "strategy": "sequential",
                "failurePolicy": "isolate",
                "maxConcurrent": 2,
                "coordinatorExpertId": "lead",
                "members": [
                    {"expertId": "lead", "roleHint": "coordinator"},
                    {"expertId": "member", "roleHint": "builds", "taskTemplate": "Do the thing."},
                ],
            }))
            .unwrap();
            let team = &created["team"];
            assert!(team.get("strategy").is_none(), "strategy retired from API");
            assert!(
                team["members"][0].get("taskTemplate").is_none(),
                "taskTemplate retired from API"
            );
            // The real contract fields are present.
            assert_eq!(team["failurePolicy"], "isolate");
            assert_eq!(team["maxConcurrent"], 2);
        });
    }

    /// §19.3: `failurePolicy` and `maxConcurrent` are the real runtime contract
    /// fields — they survive round-trip through create → update → get.
    #[test]
    fn team_contract_round_trips_failure_policy_and_max_concurrent() {
        with_temp_db(|| {
            make_experts();
            let created = team_create(&serde_json::json!({
                "id": "team-rt",
                "name": "Round",
                "failurePolicy": "require_all",
                "maxConcurrent": 5,
                "coordinatorExpertId": "lead",
                "members": [{"expertId": "lead"}, {"expertId": "member"}],
            }))
            .unwrap();
            assert_eq!(created["team"]["failurePolicy"], "require_all");
            assert_eq!(created["team"]["maxConcurrent"], 5);

            let updated = team_update(&serde_json::json!({
                "id": "team-rt",
                "failurePolicy": "fail_fast",
                "maxConcurrent": 3,
            }))
            .unwrap();
            assert_eq!(updated["team"]["failurePolicy"], "fail_fast");
            assert_eq!(updated["team"]["maxConcurrent"], 3);

            let fetched = team_get(&serde_json::json!({"id": "team-rt"})).unwrap();
            assert_eq!(fetched["team"]["failurePolicy"], "fail_fast");
            assert_eq!(fetched["team"]["maxConcurrent"], 3);
        });
    }

    /// §19.3: team_create rejects an unknown failurePolicy (fail-closed).
    #[test]
    fn team_create_rejects_unknown_failure_policy() {
        with_temp_db(|| {
            make_experts();
            let result = team_create(&serde_json::json!({
                "id": "bad-team",
                "name": "Bad",
                "failurePolicy": "explode",
                "maxConcurrent": 2,
                "members": [{"expertId": "member"}],
            }));
            assert!(result.is_err());
        });
    }

    /// §19.3: team_create rejects maxConcurrent outside 1..=8 (fail-closed).
    #[test]
    fn team_create_rejects_max_concurrent_out_of_range() {
        with_temp_db(|| {
            make_experts();
            let too_many = team_create(&serde_json::json!({
                "id": "bad-conc",
                "name": "Bad",
                "failurePolicy": "isolate",
                "maxConcurrent": 99,
                "members": [{"expertId": "member"}],
            }));
            assert!(too_many.is_err(), "maxConcurrent > 8 must fail");

            let zero = team_create(&serde_json::json!({
                "id": "bad-zero",
                "name": "Bad",
                "failurePolicy": "isolate",
                "maxConcurrent": 0,
                "members": [{"expertId": "member"}],
            }));
            assert!(zero.is_err(), "maxConcurrent 0 must fail");
        });
    }

    /// §19.3: team_create requires at least one member (fail-closed).
    #[test]
    fn team_create_requires_members() {
        with_temp_db(|| {
            make_experts();
            let no_members = team_create(&serde_json::json!({
                "id": "empty-team",
                "name": "Empty",
                "failurePolicy": "isolate",
                "maxConcurrent": 1,
                "coordinatorExpertId": "lead",
                "members": [],
            }));
            assert!(no_members.is_err(), "team with no members must fail");
        });
    }

    /// §19.3: team_delete removes the team and its members.
    #[test]
    fn team_delete_removes_team() {
        with_temp_db(|| {
            make_experts();
            team_create(&serde_json::json!({
                "id": "del-team",
                "name": "Delete",
                "failurePolicy": "isolate",
                "maxConcurrent": 1,
                "coordinatorExpertId": "lead",
                "members": [{"expertId": "lead"}, {"expertId": "member"}],
            }))
            .unwrap();
            let deleted = team_delete(&serde_json::json!({"id": "del-team"})).unwrap();
            assert_eq!(deleted["deleted"], "del-team");
            // Fetching the deleted team fails.
            assert!(team_get(&serde_json::json!({"id": "del-team"})).is_err());
        });
    }
}
