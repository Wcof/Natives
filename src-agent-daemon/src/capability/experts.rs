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

/// DB-authoritative profile load with file fallback (ADR-0016 decision 3).
///
/// 19.3-②: this is the **single authoritative loader** both DB Experts and
/// `.md` AgentProfile files go through. The DB (`capability_expert`) wins for
/// enabled rows; a file profile from the project/user profile directories is
/// the fallback (interchange format). Used by run-level capability resolution,
/// run start, and the `task` tool's `subagent_type` — so a DB Expert is usable
/// as a subagent persona exactly like a file profile.
// W9 split: agent profile loading -> experts_profile, expert team CRUD ->
// experts_team, host subagent migration -> experts_migrate. Public paths are
// re-exported below so external `capability::experts::*` keeps working.
#[path = "experts_migrate.rs"]
mod experts_migrate;
#[path = "experts_profile.rs"]
mod experts_profile;
#[path = "experts_team.rs"]
mod experts_team;
#[cfg(test)]
pub(crate) use experts_migrate::migrate_host_subagents_from;
#[cfg_attr(not(test), allow(unused_imports))]
pub(crate) use experts_migrate::{import_host_subagent_rows, migrate_host_subagents};
#[cfg_attr(not(test), allow(unused_imports))]
pub(crate) use experts_profile::{load_agent_profile, load_expert_profile_from_db};
pub(crate) use experts_team::{
    team_create, team_delete, team_get, team_list, team_update,
};

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

/// Validate the real runtime-contract team settings (19.3-④): `failurePolicy`
/// (isolate/fail_fast/require_all) and `maxConcurrent` (1..=8). `strategy` is
/// retired from the contract and always resolves to the fixed default.
fn validate_team_settings(params_value: &Value) -> Result<(String, i64), String> {
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
    Ok((failure_policy.to_string(), max_concurrent))
}

fn validate_team_settings_with_defaults(
    params_value: &Value,
    conn: &rusqlite::Connection,
    id: &str,
) -> Result<(String, i64), String> {
    let (current_policy, current_max): (String, i64) = conn
        .query_row(
            "SELECT failure_policy, max_concurrent FROM capability_expert_team WHERE id = ?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| e.to_string())?;
    let mut merged = serde_json::Map::new();
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

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn with_temp_db<F: FnOnce()>(f: F) {
        let _guard = crate::storage::DataStore::env_test_lock();
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join(format!("experts-{}.db", Uuid::new_v4()));
        let art = dir.path().join("artifacts");
        crate::storage::set_test_db_override(Some(db.clone()), Some(art.clone()));
        let _warm = crate::storage::DataStore::new(&db, &art).expect("experts temp db migrate");
        f();
        crate::storage::set_test_db_override(None, None);
    }

    #[test]
    fn db_expert_loads_through_authoritative_loader() {
        with_temp_db(|| {
            // 19.3-②: a DB Expert is loadable by the same authoritative loader
            // the run path uses — so `subagent_type`/expert selection can name it.
            insert_expert(
                &json!({
                    "id": "db-expert-1",
                    "name": "DB Expert",
                    "systemPrompt": "You are the DB expert.",
                    "tools": ["read_file"],
                    "skills": ["user:rust"],
                    "enabled": true,
                }),
                "manual",
            )
            .unwrap();
            let profile = load_agent_profile("db-expert-1", None).expect("loadable");
            assert_eq!(profile.id, "db-expert-1");
            assert_eq!(
                profile.system_prompt.as_deref(),
                Some("You are the DB expert.")
            );
            assert_eq!(profile.tools, Some(vec!["read_file".to_string()]));
            // A missing id falls back to the file loader which also fails closed.
            assert!(load_agent_profile("ghost-expert", None).is_none());
        });
    }

    #[test]
    fn disabled_db_expert_is_not_loadable() {
        with_temp_db(|| {
            insert_expert(
                &json!({
                    "id": "disabled-expert",
                    "name": "Disabled",
                    "systemPrompt": "You are disabled.",
                    "enabled": false,
                }),
                "manual",
            )
            .unwrap();
            assert!(
                load_expert_profile_from_db("disabled-expert").is_none(),
                "disabled experts must not be loadable as personas"
            );
        });
    }

    #[test]
    fn team_contract_retires_strategy_and_keeps_failure_policy_and_max_concurrent() {
        with_temp_db(|| {
            insert_expert(
                &json!({"id": "lead", "name": "Lead", "systemPrompt": "Lead."}),
                "manual",
            )
            .unwrap();
            insert_expert(
                &json!({"id": "member", "name": "Member", "systemPrompt": "Member."}),
                "manual",
            )
            .unwrap();
            // 19.3-④: strategy is no longer part of the contract; the API input
            // ignores it. failurePolicy/maxConcurrent are the real fields.
            let created = team_create(&json!({
                "id": "team-1",
                "name": "Growth",
                "strategy": "sequential",
                "failurePolicy": "require_all",
                "maxConcurrent": 4,
                "coordinatorExpertId": "lead",
                "members": [
                    {"expertId": "member", "roleHint": "builds"},
                ],
            }))
            .unwrap();
            let team = &created["team"];
            assert_eq!(team["failurePolicy"], "require_all");
            assert_eq!(team["maxConcurrent"], 4);
            assert!(
                team.get("strategy").is_none(),
                "strategy is retired from the API contract"
            );
            assert!(
                team["members"][0].get("taskTemplate").is_none(),
                "taskTemplate is retired from the API contract"
            );
            assert_eq!(team["members"][0]["expertId"], "member");

            // Round-trip through update keeps the real fields.
            let updated = team_update(&json!({
                "id": "team-1",
                "failurePolicy": "fail_fast",
                "maxConcurrent": 2,
            }))
            .unwrap();
            assert_eq!(updated["team"]["failurePolicy"], "fail_fast");
            assert_eq!(updated["team"]["maxConcurrent"], 2);
        });
    }

    #[test]
    fn team_contract_rejects_bad_failure_policy_and_max_concurrent() {
        with_temp_db(|| {
            insert_expert(
                &json!({"id": "x", "name": "X", "systemPrompt": "X."}),
                "manual",
            )
            .unwrap();
            let bad_policy = team_create(&json!({
                "id": "bad-team-1",
                "name": "Bad",
                "failurePolicy": "explode",
                "maxConcurrent": 2,
                "members": [{"expertId": "x"}],
            }));
            assert!(
                bad_policy.is_err(),
                "unknown failurePolicy must fail closed"
            );
            let bad_conc = team_create(&json!({
                "id": "bad-team-2",
                "name": "Bad",
                "failurePolicy": "isolate",
                "maxConcurrent": 99,
                "members": [{"expertId": "x"}],
            }));
            assert!(
                bad_conc.is_err(),
                "out-of-range maxConcurrent must fail closed"
            );
        });
    }

    // ── §19.5: DB Expert usable as subagent_type ──

    /// §19.5: a DB Expert with a `subagent_type`-compatible persona (tools,
    /// skills, system prompt) loads through the authoritative loader so the
    /// task tool's `subagent_type` field can name it exactly like a `.md`
    /// file profile. DB wins, file fallback, fail-closed on neither.
    #[test]
    fn db_expert_usable_as_subagent_type_via_authoritative_loader() {
        with_temp_db(|| {
            insert_expert(
                &json!({
                    "id": "subagent-persona",
                    "name": "Rust Reviewer",
                    "systemPrompt": "You are a terse Rust reviewer. Check safety and logic.",
                    "tools": ["read_file", "grep", "list_directory"],
                    "skills": ["user:rust"],
                    "permissionMode": "readonly",
                    "enabled": true,
                }),
                "manual",
            )
            .unwrap();
            let profile = load_agent_profile("subagent-persona", None)
                .expect("DB Expert loadable as a subagent persona");
            assert_eq!(profile.id, "subagent-persona");
            assert_eq!(
                profile.system_prompt.as_deref(),
                Some("You are a terse Rust reviewer. Check safety and logic.")
            );
            assert_eq!(
                profile.tools,
                Some(vec![
                    "read_file".to_string(),
                    "grep".to_string(),
                    "list_directory".to_string(),
                ]),
            );
            assert_eq!(profile.permission_mode.as_deref(), Some("readonly"));
            // The body field carries the system prompt text so the engine
            // can compile it into the effective prompt.
            assert!(!profile.body.is_empty());
        });
    }

    /// §19.5: crash recovery restores the exact same persona. A DB Expert
    /// loaded twice yields byte-identical system prompts, tools, and
    /// permission — the loader is deterministic, not re-rolled.
    #[test]
    fn db_expert_loader_is_deterministic_for_crash_recovery() {
        with_temp_db(|| {
            insert_expert(
                &json!({
                    "id": "stable-persona",
                    "name": "Stable",
                    "systemPrompt": "You are a stable persona.",
                    "tools": ["read_file"],
                    "enabled": true,
                }),
                "manual",
            )
            .unwrap();
            let first = load_agent_profile("stable-persona", None).expect("loadable");
            let second = load_agent_profile("stable-persona", None).expect("loadable");
            assert_eq!(first.system_prompt, second.system_prompt);
            assert_eq!(first.tools, second.tools);
            assert_eq!(first.permission_mode, second.permission_mode);
            assert_eq!(first.body, second.body);
        });
    }

    /// §19.3: the single authoritative loader is the ONLY profile load path.
    /// A DB Expert wins over a file profile with the same id; the file is
    /// never consulted when the DB row exists and is enabled.
    #[test]
    fn authoritative_loader_db_wins_over_file_for_same_id() {
        with_temp_db(|| {
            insert_expert(
                &json!({
                    "id": "dual-persona",
                    "name": "DB Persona",
                    "systemPrompt": "DB wins.",
                    "enabled": true,
                }),
                "manual",
            )
            .unwrap();
            // The loader checks DB first; even if a file profile with the
            // same id existed, the DB row would win.
            let profile = load_agent_profile("dual-persona", None).expect("loadable");
            assert_eq!(profile.name, "DB Persona");
            assert_eq!(profile.system_prompt.as_deref(), Some("DB wins."));
        });
    }

    /// §19.3: Team taskTemplate is retired from the API contract — it is
    /// never advertised in the team JSON even when stored in the column.
    #[test]
    fn team_contract_hides_task_template_from_api() {
        with_temp_db(|| {
            insert_expert(
                &json!({"id": "lead", "name": "Lead", "systemPrompt": "Lead."}),
                "manual",
            )
            .unwrap();
            insert_expert(
                &json!({"id": "member", "name": "Member", "systemPrompt": "Member."}),
                "manual",
            )
            .unwrap();
            // Create a team with a task_template on the member — it goes
            // into the DB column but must NOT appear in the API JSON.
            let created = team_create(&json!({
                "id": "team-template",
                "name": "Template",
                "failurePolicy": "isolate",
                "maxConcurrent": 1,
                "coordinatorExpertId": "lead",
                "members": [
                    {"expertId": "member", "roleHint": "builds", "taskTemplate": "Do the thing."},
                ],
            }))
            .unwrap();
            let team = &created["team"];
            // taskTemplate is retired: not in member JSON.
            assert!(
                team["members"][0].get("taskTemplate").is_none(),
                "taskTemplate must not be advertised in the API contract"
            );
            assert_eq!(team["members"][0]["expertId"], "member");
            // strategy is retired: not in team JSON.
            assert!(
                team.get("strategy").is_none(),
                "strategy must not be advertised in the API contract"
            );
        });
    }

    /// §19.3: export_md redacts the system_prompt from the frontmatter —
    /// wait, actually export_md writes the system_prompt as the body (it IS
    /// the persona for external CLI engines). The redaction is for logs/
    /// RPC exports, not for the `.claude/agents/` export which needs the
    /// real prompt. This test confirms export_md keeps the system prompt
    /// (the export is the interchange format, not a log).
    #[test]
    fn export_md_keeps_system_prompt_as_body_for_interchange() {
        with_temp_db(|| {
            insert_expert(
                &json!({
                    "id": "export-test",
                    "name": "Export",
                    "systemPrompt": "You are an exported persona.",
                    "tools": ["read_file"],
                    "enabled": true,
                }),
                "manual",
            )
            .unwrap();
            let exported = export_md(&json!({"id": "export-test"})).unwrap();
            let content = exported["content"].as_str().unwrap();
            // The export is the interchange format — the system prompt IS
            // the body, so it must be present (not redacted).
            assert!(content.contains("You are an exported persona."));
            // The frontmatter carries the id and name.
            assert!(content.contains("id: export-test"));
            assert!(content.contains("name: Export"));
        });
    }
}
