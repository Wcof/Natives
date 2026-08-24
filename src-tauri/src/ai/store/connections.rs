//! Connection 持久化操作。

use rusqlite::{params, Connection as DbConn};
use uuid::Uuid;

use crate::ai::model::{Connection, ConnectionHealthStatus, UpstreamProtocol};
use crate::{Error, Result};

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

pub fn list_connections(conn: &DbConn, provider_id: Option<&str>) -> Result<Vec<Connection>> {
    let query = match provider_id {
        Some(_) => {
            "SELECT id, provider_id, name, base_url, upstream_protocol, models_url, proxy_url, headers_json, enabled, health_status, last_checked_at, created_at, updated_at
             FROM ai_connections WHERE provider_id = ?1 ORDER BY created_at ASC"
        }
        None => {
            "SELECT id, provider_id, name, base_url, upstream_protocol, models_url, proxy_url, headers_json, enabled, health_status, last_checked_at, created_at, updated_at
             FROM ai_connections ORDER BY created_at ASC"
        }
    };

    let mut stmt = conn.prepare(query).map_err(Error::Database)?;

    let map_row = |row: &rusqlite::Row| {
        let proto_str: String = row.get(4)?;
        let health_str: String = row.get(9)?;
        Ok(Connection {
            id: row.get(0)?,
            provider_id: row.get(1)?,
            name: row.get(2)?,
            base_url: row.get(3)?,
            upstream_protocol: UpstreamProtocol::from_str(&proto_str)
                .unwrap_or(UpstreamProtocol::OpenaiChatCompletions),
            models_url: row.get(5)?,
            proxy_url: row.get(6)?,
            headers_json: row.get(7)?,
            enabled: row.get::<_, i64>(8)? != 0,
            health_status: ConnectionHealthStatus::from_str(&health_str),
            last_checked_at: row.get(10)?,
            created_at: row.get(11)?,
            updated_at: row.get(12)?,
        })
    };

    let rows = match provider_id {
        Some(pid) => stmt.query_map([pid], map_row).map_err(Error::Database)?,
        None => stmt.query_map([], map_row).map_err(Error::Database)?,
    };

    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Error::Database)
}

pub fn get_connection(conn: &DbConn, id: &str) -> Result<Option<Connection>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, provider_id, name, base_url, upstream_protocol, models_url, proxy_url, headers_json, enabled, health_status, last_checked_at, created_at, updated_at
             FROM ai_connections WHERE id = ?1",
        )
        .map_err(Error::Database)?;

    let mut rows = stmt
        .query_map([id], |row| {
            let proto_str: String = row.get(4)?;
            let health_str: String = row.get(9)?;
            Ok(Connection {
                id: row.get(0)?,
                provider_id: row.get(1)?,
                name: row.get(2)?,
                base_url: row.get(3)?,
                upstream_protocol: UpstreamProtocol::from_str(&proto_str)
                    .unwrap_or(UpstreamProtocol::OpenaiChatCompletions),
                models_url: row.get(5)?,
                proxy_url: row.get(6)?,
                headers_json: row.get(7)?,
                enabled: row.get::<_, i64>(8)? != 0,
                health_status: ConnectionHealthStatus::from_str(&health_str),
                last_checked_at: row.get(10)?,
                created_at: row.get(11)?,
                updated_at: row.get(12)?,
            })
        })
        .map_err(Error::Database)?;

    rows.next().transpose().map_err(Error::Database)
}

pub fn create_connection(
    conn: &DbConn,
    id: Option<&str>,
    provider_id: &str,
    name: &str,
    base_url: &str,
    upstream_protocol: UpstreamProtocol,
    models_url: Option<&str>,
    proxy_url: Option<&str>,
    headers_json: Option<&str>,
    enabled: bool,
) -> Result<Connection> {
    let conn_id = id
        .map(str::to_string)
        .unwrap_or_else(|| format!("conn-{}", Uuid::new_v4()));
    let now = now_rfc3339();

    conn.execute(
        "INSERT INTO ai_connections (id, provider_id, name, base_url, upstream_protocol, models_url, proxy_url, headers_json, enabled, health_status, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'unknown', ?10, ?10)",
        params![
            conn_id,
            provider_id,
            name,
            base_url,
            upstream_protocol.as_str(),
            models_url,
            proxy_url,
            headers_json,
            if enabled { 1 } else { 0 },
            now,
        ],
    )
    .map_err(Error::Database)?;

    get_connection(conn, &conn_id)?
        .ok_or_else(|| Error::Internal("Failed to read back created connection".into()))
}

pub fn update_connection(
    conn: &DbConn,
    id: &str,
    name: Option<&str>,
    base_url: Option<&str>,
    upstream_protocol: Option<UpstreamProtocol>,
    models_url: Option<Option<&str>>,
    proxy_url: Option<Option<&str>>,
    headers_json: Option<Option<&str>>,
    enabled: Option<bool>,
    health_status: Option<ConnectionHealthStatus>,
    last_checked_at: Option<Option<&str>>,
) -> Result<Connection> {
    let existing = get_connection(conn, id)?
        .ok_or_else(|| Error::NotFound(format!("Connection {id} not found")))?;
    let now = now_rfc3339();

    let new_name = name.unwrap_or(&existing.name);
    let new_base_url = base_url.unwrap_or(&existing.base_url);
    let new_protocol = upstream_protocol.unwrap_or(existing.upstream_protocol);
    let new_models_url = models_url.unwrap_or(existing.models_url.as_deref());
    let new_proxy_url = proxy_url.unwrap_or(existing.proxy_url.as_deref());
    let new_headers = headers_json.unwrap_or(existing.headers_json.as_deref());
    let new_enabled = enabled.unwrap_or(existing.enabled);
    let new_health = health_status.unwrap_or(existing.health_status);
    let new_last_checked = last_checked_at.unwrap_or(existing.last_checked_at.as_deref());

    conn.execute(
        "UPDATE ai_connections SET
            name = ?1, base_url = ?2, upstream_protocol = ?3, models_url = ?4,
            proxy_url = ?5, headers_json = ?6, enabled = ?7, health_status = ?8,
            last_checked_at = ?9, updated_at = ?10
         WHERE id = ?11",
        params![
            new_name,
            new_base_url,
            new_protocol.as_str(),
            new_models_url,
            new_proxy_url,
            new_headers,
            if new_enabled { 1 } else { 0 },
            new_health.as_str(),
            new_last_checked,
            now,
            id,
        ],
    )
    .map_err(Error::Database)?;

    get_connection(conn, id)?
        .ok_or_else(|| Error::Internal("Failed to read back updated connection".into()))
}

pub fn delete_connection(conn: &DbConn, id: &str) -> Result<bool> {
    let count = conn
        .execute("DELETE FROM ai_connections WHERE id = ?1", [id])
        .map_err(Error::Database)?;
    Ok(count > 0)
}
