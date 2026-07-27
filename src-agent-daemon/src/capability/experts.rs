//! Expert / expert team CRUD + markdown import/export (ADR-0016).
//!
//! The DB is authoritative; `.md` AgentProfile files are an interchange format
//! (import via the existing agent-core parser, export for external CLI
//! engines). Only provider/key/model IDs are stored — never credentials
//! (precedent: subagent_store).

use super::store;
use agent_core::profile::parse_agent_profile_markdown;
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use uuid::Uuid;

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

const EXPERT_COLS: &str = "id, name, description, system_prompt, tools_json, \
     disallowed_tools_json, permission_mode, skills_json, provider_id, key_id, model_id, \
     params_json, enabled, source, source_path, content_hash, created_at, updated_at";

fn expert_row_to_json(row: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    let tools: String = row.get("tools_json")?;
    let disallowed: String = row.get("disallowed_tools_json")?;
    let skills: String = row.get("skills_json")?;
    let params_json: String = row.get("params_json")?;
    Ok(json!({
        "id": row.get::<_, String>("id")?,
        "name": row.get::<_, String>("name")?,
        "description": row.get::<_, String>("description")?,
        "systemPrompt": row.get::<_, String>("system_prompt")?,
        "tools": serde_json::from_str::<Value>(&tools).unwrap_or_else(|_| json!([])),
        "disallowedTools": serde_json::from_str::<Value>(&disallowed).unwrap_or_else(|_| json!([])),
        "permissionMode": row.get::<_, Option<String>>("permission_mode")?,
        "skills": serde_json::from_str::<Value>(&skills).unwrap_or_else(|_| json!([])),
        "providerId": row.get::<_, Option<String>>("provider_id")?,
        "keyId": row.get::<_, Option<String>>("key_id")?,
        "modelId": row.get::<_, Option<String>>("model_id")?,
        "params": serde_json::from_str::<Value>(&params_json).unwrap_or_else(|_| json!({})),
        "enabled": row.get::<_, i64>("enabled")? != 0,
        "source": row.get::<_, String>("source")?,
        "sourcePath": row.get::<_, Option<String>>("source_path")?,
        "contentHash": row.get::<_, Option<String>>("content_hash")?,
        "createdAt": row.get::<_, String>("created_at")?,
        "updatedAt": row.get::<_, String>("updated_at")?,
    }))
}

pub fn list(params_value: &Value) -> Result<Value, String> {
    let enabled_only = params_value
        .get("enabledOnly")
        .or_else(|| params_value.get("enabled_only"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let query = params_value.get("query").and_then(Value::as_str);
    let data = store()?;
    let conn = data.conn()?;
    let mut sql = format!("SELECT {EXPERT_COLS} FROM capability_expert WHERE 1=1");
    let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if enabled_only {
        sql.push_str(" AND enabled = 1");
    }
    if let Some(q) = query.filter(|s| !s.is_empty()) {
        sql.push_str(" AND (name LIKE ? OR description LIKE ?)");
        let like = format!("%{q}%");
        args.push(Box::new(like.clone()));
        args.push(Box::new(like));
    }
    sql.push_str(" ORDER BY name");
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(
            rusqlite::params_from_iter(args.iter().map(|a| a.as_ref())),
            expert_row_to_json,
        )
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(json!({ "experts": rows }))
}

pub fn get(params_value: &Value) -> Result<Value, String> {
    let id = required_str(params_value, "id")?;
    let data = store()?;
    let conn = data.conn()?;
    let expert = conn
        .query_row(
            &format!("SELECT {EXPERT_COLS} FROM capability_expert WHERE id = ?1"),
            params![id],
            expert_row_to_json,
        )
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("expert not found: {id}"))?;
    Ok(json!({ "expert": expert }))
}

pub fn create(params_value: &Value) -> Result<Value, String> {
    insert_expert(params_value, "manual")
}

fn insert_expert(params_value: &Value, default_source: &str) -> Result<Value, String> {
    let name = required_str(params_value, "name")?.to_string();
    let system_prompt = required_str(params_value, "systemPrompt")
        .or_else(|_| required_str(params_value, "system_prompt"))?
        .to_string();
    validate_route_ids(params_value)?;
    let id = params_value
        .get("id")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    if id.contains('/') || id.contains('\\') || id.contains("..") {
        return Err("invalid expert id".into());
    }
    let source = params_value
        .get("source")
        .and_then(Value::as_str)
        .unwrap_or(default_source);
    if !matches!(source, "manual" | "import_md" | "host_migration") {
        return Err(format!("invalid source: {source}"));
    }
    let now = now_iso();
    let data = store()?;
    let conn = data.conn()?;
    conn.execute(
        "INSERT INTO capability_expert
            (id, name, description, system_prompt, tools_json, disallowed_tools_json,
             permission_mode, skills_json, provider_id, key_id, model_id, params_json,
             enabled, source, source_path, content_hash, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?17)",
        params![
            id,
            name,
            str_field(params_value, "description").unwrap_or_default(),
            system_prompt,
            list_json(params_value, "tools"),
            list_json(params_value, "disallowedTools"),
            str_field(params_value, "permissionMode"),
            list_json(params_value, "skills"),
            str_field(params_value, "providerId"),
            str_field(params_value, "keyId"),
            str_field(params_value, "modelId"),
            params_value
                .get("params")
                .map(|v| v.to_string())
                .unwrap_or_else(|| "{}".into()),
            params_value
                .get("enabled")
                .and_then(Value::as_bool)
                .unwrap_or(true) as i64,
            source,
            str_field(params_value, "sourcePath"),
            str_field(params_value, "contentHash"),
            now,
        ],
    )
    .map_err(|e| {
        if e.to_string().contains("UNIQUE") {
            format!("expert id already exists: {id}")
        } else {
            e.to_string()
        }
    })?;
    drop(conn);
    get(&json!({ "id": id }))
}

pub fn update(params_value: &Value) -> Result<Value, String> {
    let id = required_str(params_value, "id")?;
    validate_route_ids(params_value)?;
    let data = store()?;
    let conn = data.conn()?;

    let mut sets: Vec<String> = Vec::new();
    let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    let push = |sets: &mut Vec<String>,
                args: &mut Vec<Box<dyn rusqlite::ToSql>>,
                col: &str,
                v: Box<dyn rusqlite::ToSql>| {
        sets.push(format!("{col} = ?"));
        args.push(v);
    };
    if let Some(v) = params_value.get("name").and_then(Value::as_str) {
        if v.trim().is_empty() {
            return Err("name must not be empty".into());
        }
        push(&mut sets, &mut args, "name", Box::new(v.to_string()));
    }
    if let Some(v) = params_value.get("description").and_then(Value::as_str) {
        push(&mut sets, &mut args, "description", Box::new(v.to_string()));
    }
    if let Some(v) = params_value
        .get("systemPrompt")
        .or_else(|| params_value.get("system_prompt"))
        .and_then(Value::as_str)
    {
        if v.trim().is_empty() {
            return Err("systemPrompt must not be empty".into());
        }
        push(
            &mut sets,
            &mut args,
            "system_prompt",
            Box::new(v.to_string()),
        );
    }
    for (key, col) in [
        ("tools", "tools_json"),
        ("disallowedTools", "disallowed_tools_json"),
        ("skills", "skills_json"),
    ] {
        if let Some(v) = params_value.get(key).and_then(Value::as_array) {
            let items: Vec<String> = v
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect();
            push(
                &mut sets,
                &mut args,
                col,
                Box::new(serde_json::to_string(&items).unwrap_or_else(|_| "[]".into())),
            );
        }
    }
    for (key, col) in [
        ("permissionMode", "permission_mode"),
        ("providerId", "provider_id"),
        ("keyId", "key_id"),
        ("modelId", "model_id"),
    ] {
        if let Some(v) = params_value.get(key) {
            match v.as_str() {
                Some(s) if !s.trim().is_empty() => {
                    push(&mut sets, &mut args, col, Box::new(s.to_string()))
                }
                _ => push(&mut sets, &mut args, col, Box::new(None::<String>)),
            }
        }
    }
    if let Some(v) = params_value.get("params") {
        push(&mut sets, &mut args, "params_json", Box::new(v.to_string()));
    }
    if let Some(v) = params_value.get("enabled").and_then(Value::as_bool) {
        push(&mut sets, &mut args, "enabled", Box::new(v as i64));
    }
    if sets.is_empty() {
        return Err("no updatable fields provided".into());
    }
    sets.push("updated_at = ?".into());
    args.push(Box::new(now_iso()));
    args.push(Box::new(id.to_string()));
    let sql = format!(
        "UPDATE capability_expert SET {} WHERE id = ?",
        sets.join(", ")
    );
    let changed = conn
        .execute(
            &sql,
            rusqlite::params_from_iter(args.iter().map(|a| a.as_ref())),
        )
        .map_err(|e| e.to_string())?;
    if changed == 0 {
        return Err(format!("expert not found: {id}"));
    }
    drop(conn);
    get(&json!({ "id": id }))
}

pub fn delete(params_value: &Value) -> Result<Value, String> {
    let id = required_str(params_value, "id")?;
    let force = params_value
        .get("force")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let data = store()?;
    let conn = data.conn()?;
    // Referential honesty: surface team references before cascading.
    let mut stmt = conn
        .prepare(
            "SELECT DISTINCT t.id, t.name FROM capability_expert_team t
              JOIN capability_expert_team_member m ON m.team_id = t.id
             WHERE m.expert_id = ?1
             UNION
             SELECT id, name FROM capability_expert_team WHERE coordinator_expert_id = ?1",
        )
        .map_err(|e| e.to_string())?;
    let referencing: Vec<Value> = stmt
        .query_map(params![id], |r| {
            Ok(json!({ "id": r.get::<_, String>(0)?, "name": r.get::<_, String>(1)? }))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    if !referencing.is_empty() && !force {
        return Err(format!(
            "expert is referenced by teams: {}; pass force=true to delete anyway",
            serde_json::to_string(&referencing).unwrap_or_default()
        ));
    }
    let changed = conn
        .execute("DELETE FROM capability_expert WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    if changed == 0 {
        return Err(format!("expert not found: {id}"));
    }
    Ok(json!({ "deleted": id, "affectedTeams": referencing }))
}

/// Import an AgentProfile markdown (frontmatter + body) as an expert.
pub fn import_md(params_value: &Value) -> Result<Value, String> {
    let content = match params_value.get("content").and_then(Value::as_str) {
        Some(c) => c.to_string(),
        None => {
            let path = required_str(params_value, "path")?;
            std::fs::read_to_string(path).map_err(|e| format!("read {path}: {e}"))?
        }
    };
    let source_path = params_value.get("path").and_then(Value::as_str);
    let profile = parse_agent_profile_markdown(&content, source_path.map(std::path::Path::new))
        .map_err(|e| format!("parse failed: {e}"))?;
    let system_prompt = profile
        .system_prompt
        .clone()
        .filter(|s| !s.trim().is_empty())
        .ok_or("profile has no system prompt body")?;
    let mut hasher = <sha2::Sha256 as sha2::Digest>::new();
    sha2::Digest::update(&mut hasher, content.as_bytes());
    let content_hash = format!("{:x}", sha2::Digest::finalize(hasher));
    let mut params_obj = serde_json::Map::new();
    if let Some(v) = profile.max_steps {
        params_obj.insert("maxSteps".into(), json!(v));
    }
    if let Some(v) = profile.token_budget {
        params_obj.insert("tokenBudget".into(), json!(v));
    }
    if let Some(v) = &profile.context_mode {
        params_obj.insert("contextMode".into(), json!(v));
    }
    if let Some(v) = &profile.isolation_mode {
        params_obj.insert("isolationMode".into(), json!(v));
    }
    insert_expert(
        &json!({
            "id": profile.id,
            "name": profile.name,
            "description": profile.description.unwrap_or_default(),
            "systemPrompt": system_prompt,
            "tools": profile.tools.unwrap_or_default(),
            "disallowedTools": profile.disallowed_tools.unwrap_or_default(),
            "permissionMode": profile.permission_mode,
            "skills": profile.skills.unwrap_or_default(),
            "providerId": profile.provider_id,
            "keyId": profile.key_id,
            "modelId": profile.model_id,
            "params": Value::Object(params_obj),
            "source": "import_md",
            "sourcePath": source_path,
            "contentHash": content_hash,
        }),
        "import_md",
    )
}

/// Export an expert as AgentProfile markdown (frontmatter + body) for external
/// CLI engines (`.claude/agents/`). The Host owns writing it to disk.
pub fn export_md(params_value: &Value) -> Result<Value, String> {
    let id = required_str(params_value, "id")?;
    let expert = get(&json!({ "id": id }))?;
    let expert = &expert["expert"];
    let mut front = String::from("---\n");
    let mut push_kv = |k: &str, v: Option<&str>| {
        if let Some(v) = v.filter(|s| !s.is_empty()) {
            front.push_str(&format!("{k}: {v}\n"));
        }
    };
    push_kv("id", expert["id"].as_str());
    push_kv("name", expert["name"].as_str());
    push_kv("description", expert["description"].as_str());
    push_kv("permissionMode", expert["permissionMode"].as_str());
    push_kv("providerId", expert["providerId"].as_str());
    push_kv("keyId", expert["keyId"].as_str());
    push_kv("modelId", expert["modelId"].as_str());
    let list_line = |v: &Value| -> Option<String> {
        let items: Vec<&str> = v.as_array()?.iter().filter_map(Value::as_str).collect();
        if items.is_empty() {
            None
        } else {
            Some(format!("[{}]", items.join(", ")))
        }
    };
    if let Some(line) = list_line(&expert["tools"]) {
        front.push_str(&format!("tools: {line}\n"));
    }
    if let Some(line) = list_line(&expert["disallowedTools"]) {
        front.push_str(&format!("disallowedTools: {line}\n"));
    }
    if let Some(line) = list_line(&expert["skills"]) {
        front.push_str(&format!("skills: {line}\n"));
    }
    for (param, key) in [
        ("maxSteps", "maxSteps"),
        ("tokenBudget", "tokenBudget"),
        ("contextMode", "contextMode"),
        ("isolationMode", "isolationMode"),
    ] {
        if let Some(v) = expert["params"].get(param) {
            if !v.is_null() {
                let rendered = v
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| v.to_string());
                front.push_str(&format!("{key}: {rendered}\n"));
            }
        }
    }
    front.push_str("---\n\n");
    front.push_str(expert["systemPrompt"].as_str().unwrap_or_default());
    front.push('\n');
    Ok(json!({ "id": id, "content": front }))
}

// ---------------------------------------------------------------------------
// Expert teams
// ---------------------------------------------------------------------------

fn team_to_json(conn: &rusqlite::Connection, id: &str) -> Result<Option<Value>, String> {
    let team = conn
        .query_row(
            "SELECT id, name, description, strategy, failure_policy, max_concurrent,
                    coordinator_expert_id, enabled, created_at, updated_at
               FROM capability_expert_team WHERE id = ?1",
            params![id],
            |row| {
                Ok(json!({
                    "id": row.get::<_, String>(0)?,
                    "name": row.get::<_, String>(1)?,
                    "description": row.get::<_, String>(2)?,
                    "strategy": row.get::<_, String>(3)?,
                    "failurePolicy": row.get::<_, String>(4)?,
                    "maxConcurrent": row.get::<_, i64>(5)?,
                    "coordinatorExpertId": row.get::<_, Option<String>>(6)?,
                    "enabled": row.get::<_, i64>(7)? != 0,
                    "createdAt": row.get::<_, String>(8)?,
                    "updatedAt": row.get::<_, String>(9)?,
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
                "taskTemplate": row.get::<_, String>(3)?,
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
    let (strategy, failure_policy, max_concurrent) = validate_team_settings(params_value)?;
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
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
        params![
            id,
            name,
            str_field(params_value, "description").unwrap_or_default(),
            strategy,
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
    if params_value.get("strategy").is_some()
        || params_value.get("failurePolicy").is_some()
        || params_value.get("maxConcurrent").is_some()
    {
        let (strategy, failure_policy, max_concurrent) =
            validate_team_settings_with_defaults(params_value, &tx, &id)?;
        tx.execute(
            "UPDATE capability_expert_team
                SET strategy = ?2, failure_policy = ?3, max_concurrent = ?4 WHERE id = ?1",
            params![id, strategy, failure_policy, max_concurrent],
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

// ---------------------------------------------------------------------------
// Host legacy line retirement (ADR-0016): one-shot import of the Host-side
// `subagents` table (commands/subagent.rs) into the capability library.
// `subagent_runs` history is NOT migrated — it belongs to a provider-direct
// execution path with different semantics and stays as a read-only archive.
// ---------------------------------------------------------------------------

/// Import Host `subagents` rows once. Idempotent: skipped when any
/// `source='host_migration'` expert exists. Safe to retry on failure.
pub fn migrate_host_subagents() -> Result<u32, String> {
    let natives_path = crate::default_natives_db_path();
    migrate_host_subagents_from(&natives_path)
}

pub fn migrate_host_subagents_from(natives_db: &std::path::Path) -> Result<u32, String> {
    {
        let data = store()?;
        let conn = data.conn()?;
        let already: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM capability_expert WHERE source = 'host_migration')",
                [],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if already {
            return Ok(0);
        }
    }
    if !natives_db.exists() {
        return Ok(0);
    }
    let host = rusqlite::Connection::open_with_flags(
        natives_db,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(|e| format!("open natives.db read-only: {e}"))?;
    let has_table: bool = host
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='subagents')",
            [],
            |r| r.get(0),
        )
        .unwrap_or(false);
    if !has_table {
        return Ok(0);
    }
    let mut stmt = host
        .prepare(
            "SELECT id, name, role, instructions, tools, provider_id, provider_key_id,
                    model_id, enabled FROM subagents",
        )
        .map_err(|e| e.to_string())?;
    type HostRow = (
        String,
        String,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        String,
        i64,
    );
    let rows: Vec<HostRow> = stmt
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
                r.get(8)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    drop(stmt);
    drop(host);

    let mut migrated = 0u32;
    for (id, name, role, instructions, tools, provider_id, key_id, model_id, enabled) in rows {
        let system_prompt = if !instructions.trim().is_empty() {
            instructions
        } else if !role.trim().is_empty() {
            role.clone()
        } else {
            format!("You are {name}.")
        };
        let tools: Vec<String> = tools
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        // Host line allowed 'auto' key routing; capability experts store IDs only.
        let key_id = key_id.filter(|k| !k.eq_ignore_ascii_case("auto"));
        let result = insert_expert(
            &json!({
                "id": id,
                "name": name,
                "description": role,
                "systemPrompt": system_prompt,
                "tools": tools,
                "providerId": provider_id,
                "keyId": key_id,
                "modelId": if model_id.is_empty() { Value::Null } else { json!(model_id) },
                "enabled": enabled != 0,
                "source": "host_migration",
            }),
            "host_migration",
        );
        match result {
            Ok(_) => migrated += 1,
            Err(e) if e.contains("already exists") => {}
            Err(e) => return Err(format!("migrate host subagent failed: {e}")),
        }
    }
    Ok(migrated)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn ensure_expert_exists(conn: &rusqlite::Connection, id: &str) -> Result<(), String> {
    let exists: bool = conn
        .query_row(
            "SELECT 1 FROM capability_expert WHERE id = ?1",
            params![id],
            |_| Ok(true),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .unwrap_or(false);
    if exists {
        Ok(())
    } else {
        Err(format!("expert not found: {id}"))
    }
}

type MemberTuple = (String, String, String);

fn parse_members(params_value: &Value) -> Result<Vec<MemberTuple>, String> {
    let Some(members) = params_value.get("members").and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for m in members {
        let expert_id = m
            .get("expertId")
            .or_else(|| m.get("expert_id"))
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .ok_or("member needs expertId")?;
        out.push((
            expert_id.to_string(),
            m.get("roleHint")
                .or_else(|| m.get("role_hint"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            m.get("taskTemplate")
                .or_else(|| m.get("task_template"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        ));
    }
    Ok(out)
}

fn insert_members(
    tx: &rusqlite::Transaction<'_>,
    team_id: &str,
    members: &[MemberTuple],
) -> Result<(), String> {
    for (position, (expert_id, role_hint, task_template)) in members.iter().enumerate() {
        tx.execute(
            "INSERT INTO capability_expert_team_member
                (team_id, position, expert_id, role_hint, task_template)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                team_id,
                position as i64,
                expert_id,
                role_hint,
                task_template
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn validate_team_settings(params_value: &Value) -> Result<(String, String, i64), String> {
    let strategy = params_value
        .get("strategy")
        .and_then(Value::as_str)
        .unwrap_or("parallel");
    if !matches!(strategy, "parallel" | "sequential" | "coordinator") {
        return Err(format!("invalid strategy: {strategy}"));
    }
    let failure_policy = params_value
        .get("failurePolicy")
        .or_else(|| params_value.get("failure_policy"))
        .and_then(Value::as_str)
        .unwrap_or("isolate");
    if !matches!(failure_policy, "isolate" | "fail_fast" | "require_all") {
        return Err(format!("invalid failurePolicy: {failure_policy}"));
    }
    let max_concurrent = params_value
        .get("maxConcurrent")
        .or_else(|| params_value.get("max_concurrent"))
        .and_then(Value::as_i64)
        .unwrap_or(3);
    if !(1..=8).contains(&max_concurrent) {
        return Err("maxConcurrent must be in 1..=8".into());
    }
    Ok((
        strategy.to_string(),
        failure_policy.to_string(),
        max_concurrent,
    ))
}

fn validate_team_settings_with_defaults(
    params_value: &Value,
    conn: &rusqlite::Connection,
    id: &str,
) -> Result<(String, String, i64), String> {
    let (current_strategy, current_policy, current_max): (String, String, i64) = conn
        .query_row(
            "SELECT strategy, failure_policy, max_concurrent FROM capability_expert_team WHERE id = ?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(|e| e.to_string())?;
    let mut merged = serde_json::Map::new();
    merged.insert(
        "strategy".into(),
        params_value
            .get("strategy")
            .cloned()
            .unwrap_or_else(|| json!(current_strategy)),
    );
    merged.insert(
        "failurePolicy".into(),
        params_value
            .get("failurePolicy")
            .cloned()
            .unwrap_or_else(|| json!(current_policy)),
    );
    merged.insert(
        "maxConcurrent".into(),
        params_value
            .get("maxConcurrent")
            .cloned()
            .unwrap_or_else(|| json!(current_max)),
    );
    validate_team_settings(&Value::Object(merged))
}

fn validate_route_ids(params_value: &Value) -> Result<(), String> {
    if let Some(key_id) = params_value
        .get("keyId")
        .or_else(|| params_value.get("key_id"))
        .and_then(Value::as_str)
    {
        if key_id.eq_ignore_ascii_case("auto") {
            return Err("key_id must not be 'auto' (subagent_store precedent)".into());
        }
    }
    Ok(())
}

fn str_field(params: &Value, key: &str) -> Option<String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .map(str::to_string)
}

fn list_json(params: &Value, key: &str) -> String {
    let items: Vec<String> = params
        .get(key)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    serde_json::to_string(&items).unwrap_or_else(|_| "[]".into())
}

fn required_str<'a>(params: &'a Value, key: &str) -> Result<&'a str, String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| format!("{key} required"))
}
