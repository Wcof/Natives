use crate::{Error, Result};
use rusqlite::Connection;

pub fn list_builtin_tools(conn: &Connection) -> Result<Vec<serde_json::Value>> {
    let mut stmt = conn
        .prepare("SELECT id, enabled, driver FROM builtin_tools")
        .map_err(Error::Database)?;
    let rows = stmt
        .query_map([], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "enabled": row.get::<_, i64>(1)? != 0,
                "driver": row.get::<_, String>(2)?,
            }))
        })
        .map_err(Error::Database)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Error::Database)
}

/// Update a builtin tool's enabled/driver state
pub fn update_builtin_tool(conn: &Connection, id: &str, enabled: bool, driver: &str) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO builtin_tools (id, enabled, driver, updated_at) VALUES (?1, ?2, ?3, datetime('now'))",
        rusqlite::params![id, enabled as i64, driver],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// Ensure a builtin tool row exists (seed from frontend registry)
pub fn seed_builtin_tool(conn: &Connection, id: &str, driver: &str) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO builtin_tools (id, enabled, driver) VALUES (?1, 0, ?2)",
        rusqlite::params![id, driver],
    )
    .map_err(Error::Database)?;
    Ok(())
}

// ──────────────────────────────────────────────
// Usage stats CRUD (structured per-model daily stats)
// ──────────────────────────────────────────────

/// Upsert a daily model usage stat row with source breadcrumb.
#[allow(clippy::too_many_arguments)] // pre-existing parameter list
pub fn upsert_usage_stat(
    conn: &Connection,
    date: &str,
    source: &str,
    source_path: Option<&str>,
    model: &str,
    input_tokens: u64,
    output_tokens: u64,
    cache_creation_tokens: u64,
    cache_read_tokens: u64,
    request_count: u64,
    cost_usd: f64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO usage_stats (date, source, source_path, model, input_tokens, output_tokens,
            cache_creation_tokens, cache_read_tokens, request_count, cost_usd)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(date, source, model) DO UPDATE SET
            input_tokens = excluded.input_tokens,
            output_tokens = excluded.output_tokens,
            cache_creation_tokens = excluded.cache_creation_tokens,
            cache_read_tokens = excluded.cache_read_tokens,
            request_count = excluded.request_count,
            cost_usd = excluded.cost_usd,
            source_path = excluded.source_path",
        rusqlite::params![
            date,
            source,
            source_path,
            model,
            input_tokens,
            output_tokens,
            cache_creation_tokens,
            cache_read_tokens,
            request_count,
            cost_usd
        ],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// Query usage stats, optionally filtered by source and date range.
/// Returns array of JSON objects with all columns.
#[allow(dead_code)]
pub fn query_usage_stats(
    conn: &Connection,
    source: Option<&str>,
    from_date: Option<&str>,
    to_date: Option<&str>,
) -> Result<Vec<serde_json::Value>> {
    let mut sql = "SELECT date, source, source_path, model, input_tokens, output_tokens, cache_creation_tokens, cache_read_tokens, request_count, cost_usd FROM usage_stats WHERE 1=1".to_string();
    if source.is_some() {
        sql.push_str(" AND source = ?");
    }
    if from_date.is_some() {
        sql.push_str(" AND date >= ?");
    }
    if to_date.is_some() {
        sql.push_str(" AND date <= ?");
    }
    sql.push_str(" ORDER BY date DESC, model");

    let mut stmt = conn.prepare(&sql).map_err(Error::Database)?;
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    if let Some(s) = source {
        params.push(Box::new(s.to_string()));
    }
    if let Some(d) = from_date {
        params.push(Box::new(d.to_string()));
    }
    if let Some(d) = to_date {
        params.push(Box::new(d.to_string()));
    }

    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let rows = stmt
        .query_map(param_refs.as_slice(), |row| {
            Ok(serde_json::json!({
                "date": row.get::<_, String>(0)?,
                "source": row.get::<_, String>(1)?,
                "sourcePath": row.get::<_, Option<String>>(2)?,
                "model": row.get::<_, String>(3)?,
                "inputTokens": row.get::<_, u64>(4)?,
                "outputTokens": row.get::<_, u64>(5)?,
                "cacheCreationTokens": row.get::<_, u64>(6)?,
                "cacheReadTokens": row.get::<_, u64>(7)?,
                "requestCount": row.get::<_, u64>(8)?,
                "costUsd": row.get::<_, f64>(9)?,
            }))
        })
        .map_err(Error::Database)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Error::Database)
}

// ──────────────────────────────────────────────
// Skill usage CRUD (skill invocation tracking)
// ──────────────────────────────────────────────

/// Upsert a daily skill usage row with source breadcrumb.
pub fn upsert_skill_usage(
    conn: &Connection,
    date: &str,
    source: &str,
    source_path: Option<&str>,
    skill_name: &str,
    trigger_count: u64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO skill_usage (date, source, source_path, skill_name, trigger_count)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(date, source, skill_name) DO UPDATE SET
            trigger_count = excluded.trigger_count,
            source_path = excluded.source_path",
        rusqlite::params![date, source, source_path, skill_name, trigger_count],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// Query skill usage stats, optionally filtered by skill name and date range.
#[allow(dead_code)]
pub fn query_skill_usage(
    conn: &Connection,
    skill_name: Option<&str>,
    from_date: Option<&str>,
    to_date: Option<&str>,
) -> Result<Vec<serde_json::Value>> {
    let mut sql =
        "SELECT date, source, source_path, skill_name, trigger_count FROM skill_usage WHERE 1=1"
            .to_string();
    if skill_name.is_some() {
        sql.push_str(" AND skill_name = ?");
    }
    if from_date.is_some() {
        sql.push_str(" AND date >= ?");
    }
    if to_date.is_some() {
        sql.push_str(" AND date <= ?");
    }
    sql.push_str(" ORDER BY date DESC, skill_name");

    let mut stmt = conn.prepare(&sql).map_err(Error::Database)?;
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    if let Some(s) = skill_name {
        params.push(Box::new(s.to_string()));
    }
    if let Some(d) = from_date {
        params.push(Box::new(d.to_string()));
    }
    if let Some(d) = to_date {
        params.push(Box::new(d.to_string()));
    }

    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let rows = stmt
        .query_map(param_refs.as_slice(), |row| {
            Ok(serde_json::json!({
                "date": row.get::<_, String>(0)?,
                "source": row.get::<_, String>(1)?,
                "sourcePath": row.get::<_, Option<String>>(2)?,
                "skillName": row.get::<_, String>(3)?,
                "triggerCount": row.get::<_, u64>(4)?,
            }))
        })
        .map_err(Error::Database)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Error::Database)
}
