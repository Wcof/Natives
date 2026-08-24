//! Local Proxy Repository & Store（DAT-002 / plan3 03-data-security-contracts §1）。
//!
//! 统一管理 `proxy_settings`, `proxy_routes`, `proxy_route_targets`, `proxy_usage_records` 的持久化操作。

use rusqlite::{params, Connection as DbConn, OptionalExtension};
use uuid::Uuid;

use super::model::{
    CredentialSelector, PortMode, ProxySettings, ProxyUsageRecord, Route, RouteTarget,
};
use crate::{Error, Result};

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

// ── Proxy Settings ──

pub fn get_proxy_settings(conn: &DbConn) -> Result<ProxySettings> {
    let mut stmt = conn
        .prepare(
            "SELECT id, enabled_intent, bind_host, port_mode, configured_port, effective_port,
                    access_secret_ref, grace_timeout_ms, max_concurrency, max_request_body_bytes, updated_at
             FROM proxy_settings WHERE id = 'default'",
        )
        .map_err(Error::Database)?;

    let opt = stmt
        .query_row([], |row| {
            let port_mode_str: String = row.get(3)?;
            Ok(ProxySettings {
                id: row.get(0)?,
                enabled_intent: row.get::<_, i64>(1)? != 0,
                bind_host: row.get(2)?,
                port_mode: PortMode::from_str(&port_mode_str),
                configured_port: row.get::<_, i64>(4)? as u16,
                effective_port: row.get::<_, i64>(5)? as u16,
                access_secret_ref: row.get(6)?,
                grace_timeout_ms: row.get::<_, i64>(7)? as u64,
                max_concurrency: row.get::<_, i64>(8)? as u32,
                max_request_body_bytes: row.get::<_, i64>(9)? as usize,
                updated_at: row.get(10)?,
            })
        })
        .optional()
        .map_err(Error::Database)?;

    match opt {
        Some(settings) => Ok(settings),
        None => {
            let default_settings = ProxySettings::default();
            save_proxy_settings(conn, &default_settings)?;
            Ok(default_settings)
        }
    }
}

pub fn save_proxy_settings(conn: &DbConn, settings: &ProxySettings) -> Result<()> {
    let now = now_rfc3339();
    conn.execute(
        "INSERT INTO proxy_settings
            (id, enabled_intent, bind_host, port_mode, configured_port, effective_port,
             access_secret_ref, grace_timeout_ms, max_concurrency, max_request_body_bytes, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
         ON CONFLICT(id) DO UPDATE SET
             enabled_intent = excluded.enabled_intent,
             bind_host = excluded.bind_host,
             port_mode = excluded.port_mode,
             configured_port = excluded.configured_port,
             effective_port = excluded.effective_port,
             access_secret_ref = excluded.access_secret_ref,
             grace_timeout_ms = excluded.grace_timeout_ms,
             max_concurrency = excluded.max_concurrency,
             max_request_body_bytes = excluded.max_request_body_bytes,
             updated_at = excluded.updated_at",
        params![
            "default",
            if settings.enabled_intent { 1 } else { 0 },
            &settings.bind_host,
            settings.port_mode.as_str(),
            settings.configured_port as i64,
            settings.effective_port as i64,
            &settings.access_secret_ref,
            settings.grace_timeout_ms as i64,
            settings.max_concurrency as i64,
            settings.max_request_body_bytes as i64,
            now,
        ],
    )
    .map_err(Error::Database)?;
    Ok(())
}

pub fn set_proxy_effective_port(conn: &DbConn, port: u16) -> Result<()> {
    let now = now_rfc3339();
    conn.execute(
        "UPDATE proxy_settings SET effective_port = ?1, updated_at = ?2 WHERE id = 'default'",
        params![port as i64, now],
    )
    .map_err(Error::Database)?;
    Ok(())
}

// ── Routes & Targets ──

pub fn list_routes(conn: &DbConn) -> Result<Vec<Route>> {
    let mut stmt = conn
        .prepare("SELECT id, local_model, enabled, strategy, created_at, updated_at FROM proxy_routes ORDER BY local_model ASC")
        .map_err(Error::Database)?;

    let route_rows = stmt
        .query_map([], |row| {
            Ok(Route {
                id: row.get(0)?,
                local_model: row.get(1)?,
                enabled: row.get::<_, i64>(2)? != 0,
                strategy: row.get(3)?,
                targets: Vec::new(),
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })
        .map_err(Error::Database)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Error::Database)?;

    let mut routes = Vec::new();
    let mut target_stmt = conn
        .prepare(
            "SELECT id, route_id, position, connection_id, model_id, credential_selector_json, priority, enabled
             FROM proxy_route_targets WHERE route_id = ?1 ORDER BY position ASC",
        )
        .map_err(Error::Database)?;

    for mut route in route_rows {
        let targets = target_stmt
            .query_map([&route.id], |row| {
                let sel_json: String = row.get(5)?;
                let selector: CredentialSelector =
                    serde_json::from_str(&sel_json).unwrap_or(CredentialSelector::Pool {
                        policy: super::model::PoolPolicy::PriorityRoundRobin,
                    });
                Ok(RouteTarget {
                    id: row.get(0)?,
                    route_id: row.get(1)?,
                    position: row.get::<_, i64>(2)? as u32,
                    connection_id: row.get(3)?,
                    model_id: row.get(4)?,
                    credential_selector: selector,
                    priority: row.get::<_, i64>(6)? as u32,
                    enabled: row.get::<_, i64>(7)? != 0,
                })
            })
            .map_err(Error::Database)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Error::Database)?;

        route.targets = targets;
        routes.push(route);
    }

    Ok(routes)
}

pub fn get_route(conn: &DbConn, id: &str) -> Result<Option<Route>> {
    let mut stmt = conn
        .prepare("SELECT id, local_model, enabled, strategy, created_at, updated_at FROM proxy_routes WHERE id = ?1")
        .map_err(Error::Database)?;

    let route_opt = stmt
        .query_row([id], |row| {
            Ok(Route {
                id: row.get(0)?,
                local_model: row.get(1)?,
                enabled: row.get::<_, i64>(2)? != 0,
                strategy: row.get(3)?,
                targets: Vec::new(),
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })
        .optional()
        .map_err(Error::Database)?;

    let Some(mut route) = route_opt else {
        return Ok(None);
    };

    let mut target_stmt = conn
        .prepare(
            "SELECT id, route_id, position, connection_id, model_id, credential_selector_json, priority, enabled
             FROM proxy_route_targets WHERE route_id = ?1 ORDER BY position ASC",
        )
        .map_err(Error::Database)?;

    let targets = target_stmt
        .query_map([&route.id], |row| {
            let sel_json: String = row.get(5)?;
            let selector: CredentialSelector =
                serde_json::from_str(&sel_json).unwrap_or(CredentialSelector::Pool {
                    policy: super::model::PoolPolicy::PriorityRoundRobin,
                });
            Ok(RouteTarget {
                id: row.get(0)?,
                route_id: row.get(1)?,
                position: row.get::<_, i64>(2)? as u32,
                connection_id: row.get(3)?,
                model_id: row.get(4)?,
                credential_selector: selector,
                priority: row.get::<_, i64>(6)? as u32,
                enabled: row.get::<_, i64>(7)? != 0,
            })
        })
        .map_err(Error::Database)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Error::Database)?;

    route.targets = targets;
    Ok(Some(route))
}

pub fn get_route_by_model(conn: &DbConn, local_model: &str) -> Result<Option<Route>> {
    let mut stmt = conn
        .prepare("SELECT id FROM proxy_routes WHERE local_model = ?1 AND enabled = 1")
        .map_err(Error::Database)?;
    let id_opt: Option<String> = stmt
        .query_row([local_model], |r| r.get(0))
        .optional()
        .map_err(Error::Database)?;

    match id_opt {
        Some(id) => get_route(conn, &id),
        None => Ok(None),
    }
}

pub fn save_route(conn: &DbConn, route: &Route) -> Result<Route> {
    let now = now_rfc3339();
    let tx = conn.unchecked_transaction().map_err(Error::Database)?;

    tx.execute(
        "INSERT INTO proxy_routes (id, local_model, enabled, strategy, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5)
         ON CONFLICT(id) DO UPDATE SET
             local_model = excluded.local_model,
             enabled = excluded.enabled,
             strategy = excluded.strategy,
             updated_at = excluded.updated_at",
        params![
            &route.id,
            &route.local_model,
            if route.enabled { 1 } else { 0 },
            &route.strategy,
            now,
        ],
    )
    .map_err(Error::Database)?;

    tx.execute(
        "DELETE FROM proxy_route_targets WHERE route_id = ?1",
        [&route.id],
    )
    .map_err(Error::Database)?;

    for (pos, target) in route.targets.iter().enumerate() {
        let sel_json = serde_json::to_string(&target.credential_selector)
            .map_err(|e| Error::Internal(e.to_string()))?;
        let target_id = if target.id.is_empty() {
            format!("target-{}", Uuid::new_v4())
        } else {
            target.id.clone()
        };

        tx.execute(
            "INSERT INTO proxy_route_targets
                (id, route_id, position, connection_id, model_id, credential_selector_json, priority, enabled)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                target_id,
                &route.id,
                pos as i64,
                &target.connection_id,
                &target.model_id,
                sel_json,
                target.priority as i64,
                if target.enabled { 1 } else { 0 },
            ],
        )
        .map_err(Error::Database)?;
    }

    tx.commit().map_err(Error::Database)?;
    get_route(conn, &route.id)?
        .ok_or_else(|| Error::Internal("Failed to read back saved route".into()))
}

pub fn delete_route(conn: &DbConn, id: &str) -> Result<bool> {
    let count = conn
        .execute("DELETE FROM proxy_routes WHERE id = ?1", [id])
        .map_err(Error::Database)?;
    Ok(count > 0)
}

// ── Usage Records ──

pub fn insert_usage_record(conn: &DbConn, record: &ProxyUsageRecord) -> Result<()> {
    conn.execute(
        "INSERT INTO proxy_usage_records
            (id, route_id, connection_id, credential_id, inbound_protocol, upstream_protocol,
             local_model, upstream_model, prompt_tokens, completion_tokens, total_tokens,
             reasoning_tokens, cached_tokens, latency_ms, status, error_code, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
        params![
            record.id,
            record.route_id,
            record.connection_id,
            record.credential_id,
            record.inbound_protocol,
            record.upstream_protocol,
            record.local_model,
            record.upstream_model,
            record.prompt_tokens as i64,
            record.completion_tokens as i64,
            record.total_tokens as i64,
            record.reasoning_tokens.map(|v| v as i64),
            record.cached_tokens.map(|v| v as i64),
            record.latency_ms as i64,
            record.status,
            record.error_code,
            record.created_at,
        ],
    )
    .map_err(Error::Database)?;
    Ok(())
}

pub fn list_usage_records(
    conn: &DbConn,
    limit: usize,
    offset: usize,
) -> Result<Vec<ProxyUsageRecord>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, route_id, connection_id, credential_id, inbound_protocol, upstream_protocol,
                    local_model, upstream_model, prompt_tokens, completion_tokens, total_tokens,
                    reasoning_tokens, cached_tokens, latency_ms, status, error_code, created_at
             FROM proxy_usage_records ORDER BY created_at DESC LIMIT ?1 OFFSET ?2",
        )
        .map_err(Error::Database)?;

    let rows = stmt
        .query_map(params![limit as i64, offset as i64], |row| {
            Ok(ProxyUsageRecord {
                id: row.get(0)?,
                route_id: row.get(1)?,
                connection_id: row.get(2)?,
                credential_id: row.get(3)?,
                inbound_protocol: row.get(4)?,
                upstream_protocol: row.get(5)?,
                local_model: row.get(6)?,
                upstream_model: row.get(7)?,
                prompt_tokens: row.get::<_, i64>(8)? as u64,
                completion_tokens: row.get::<_, i64>(9)? as u64,
                total_tokens: row.get::<_, i64>(10)? as u64,
                reasoning_tokens: row.get::<_, Option<i64>>(11)?.map(|v| v as u64),
                cached_tokens: row.get::<_, Option<i64>>(12)?.map(|v| v as u64),
                latency_ms: row.get::<_, i64>(13)? as u64,
                status: row.get(14)?,
                error_code: row.get(15)?,
                created_at: row.get(16)?,
            })
        })
        .map_err(Error::Database)?;

    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Error::Database)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{apply_migrations, create_tables};
    use crate::proxy::model::PoolPolicy;

    fn setup_test_db() -> DbConn {
        let conn = DbConn::open_in_memory().unwrap();
        create_tables(&conn).unwrap();
        apply_migrations(&conn).unwrap();
        conn
    }

    #[test]
    fn proxy_settings_default_and_update() {
        let conn = setup_test_db();
        let settings = get_proxy_settings(&conn).unwrap();
        assert_eq!(settings.bind_host, "127.0.0.1");
        assert_eq!(settings.configured_port, 15721);
        assert!(!settings.enabled_intent);

        let mut updated = settings;
        updated.enabled_intent = true;
        updated.effective_port = 15721;
        save_proxy_settings(&conn, &updated).unwrap();

        let read_back = get_proxy_settings(&conn).unwrap();
        assert!(read_back.enabled_intent);
        assert_eq!(read_back.effective_port, 15721);
    }

    #[test]
    fn proxy_routes_and_targets_crud() {
        let conn = setup_test_db();
        // Insert a prerequisite provider and connection
        conn.execute(
            "INSERT INTO ai_providers (id, name, created_at, updated_at) VALUES ('p1', 'OpenAI', 't0', 't0')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO ai_connections (id, provider_id, name, base_url, created_at, updated_at)
             VALUES ('c1', 'p1', 'Official', 'https://api.openai.com/v1', 't0', 't0')",
            [],
        )
        .unwrap();

        let route = Route {
            id: "route-gpt4".into(),
            local_model: "gpt-4o".into(),
            enabled: true,
            strategy: "ordered".into(),
            targets: vec![RouteTarget {
                id: "target-1".into(),
                route_id: "route-gpt4".into(),
                position: 0,
                connection_id: "c1".into(),
                model_id: "gpt-4o-2024-08-06".into(),
                credential_selector: CredentialSelector::Pool {
                    policy: PoolPolicy::PriorityRoundRobin,
                },
                priority: 0,
                enabled: true,
            }],
            created_at: now_rfc3339(),
            updated_at: now_rfc3339(),
        };

        save_route(&conn, &route).unwrap();

        let routes = list_routes(&conn).unwrap();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].local_model, "gpt-4o");
        assert_eq!(routes[0].targets.len(), 1);

        let by_model = get_route_by_model(&conn, "gpt-4o").unwrap().unwrap();
        assert_eq!(by_model.id, "route-gpt4");

        let deleted = delete_route(&conn, "route-gpt4").unwrap();
        assert!(deleted);
        assert!(get_route(&conn, "route-gpt4").unwrap().is_none());
    }

    #[test]
    fn usage_records_insert_and_list() {
        let conn = setup_test_db();
        let record = ProxyUsageRecord {
            id: format!("usage-{}", Uuid::new_v4()),
            route_id: Some("route-1".into()),
            connection_id: Some("c1".into()),
            credential_id: Some("cred-1".into()),
            inbound_protocol: "openai_chat_completions".into(),
            upstream_protocol: "anthropic_messages".into(),
            local_model: "coding".into(),
            upstream_model: "claude-3-7-sonnet".into(),
            prompt_tokens: 150,
            completion_tokens: 50,
            total_tokens: 200,
            reasoning_tokens: Some(20),
            cached_tokens: Some(30),
            latency_ms: 1200,
            status: "success".into(),
            error_code: None,
            created_at: now_rfc3339(),
        };

        insert_usage_record(&conn, &record).unwrap();

        let records = list_usage_records(&conn, 10, 0).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].prompt_tokens, 150);
        assert_eq!(records[0].completion_tokens, 50);
        assert_eq!(records[0].reasoning_tokens, Some(20));
    }
}
