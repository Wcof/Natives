//! Provider 持久化操作。

use rusqlite::{params, Connection as DbConn};
use uuid::Uuid;

use crate::ai::model::{DeleteImpact, Provider};
use crate::{Error, Result};

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

pub fn list_providers(conn: &DbConn) -> Result<Vec<Provider>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, preset_key, name, website_url, icon_key, enabled, created_at, updated_at
             FROM ai_providers ORDER BY created_at ASC",
        )
        .map_err(Error::Database)?;

    let rows = stmt
        .query_map([], |row| {
            Ok(Provider {
                id: row.get(0)?,
                preset_key: row.get(1)?,
                name: row.get(2)?,
                website_url: row.get(3)?,
                icon_key: row.get(4)?,
                enabled: row.get::<_, i64>(5)? != 0,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
            })
        })
        .map_err(Error::Database)?;

    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Error::Database)
}

pub fn get_provider(conn: &DbConn, id: &str) -> Result<Option<Provider>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, preset_key, name, website_url, icon_key, enabled, created_at, updated_at
             FROM ai_providers WHERE id = ?1",
        )
        .map_err(Error::Database)?;

    let mut rows = stmt
        .query_map([id], |row| {
            Ok(Provider {
                id: row.get(0)?,
                preset_key: row.get(1)?,
                name: row.get(2)?,
                website_url: row.get(3)?,
                icon_key: row.get(4)?,
                enabled: row.get::<_, i64>(5)? != 0,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
            })
        })
        .map_err(Error::Database)?;

    rows.next().transpose().map_err(Error::Database)
}

pub fn create_provider(
    conn: &DbConn,
    id: Option<&str>,
    preset_key: Option<&str>,
    name: &str,
    website_url: &str,
    icon_key: Option<&str>,
    enabled: bool,
) -> Result<Provider> {
    let provider_id = id
        .map(str::to_string)
        .unwrap_or_else(|| format!("provider-{}", Uuid::new_v4()));
    let now = now_rfc3339();

    conn.execute(
        "INSERT INTO ai_providers (id, preset_key, name, website_url, icon_key, enabled, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
        params![
            provider_id,
            preset_key,
            name,
            website_url,
            icon_key,
            if enabled { 1 } else { 0 },
            now,
        ],
    )
    .map_err(Error::Database)?;

    get_provider(conn, &provider_id)?
        .ok_or_else(|| Error::Internal("Failed to read back created provider".into()))
}

pub fn update_provider(
    conn: &DbConn,
    id: &str,
    name: Option<&str>,
    website_url: Option<&str>,
    icon_key: Option<&str>,
    enabled: Option<bool>,
) -> Result<Provider> {
    let existing = get_provider(conn, id)?
        .ok_or_else(|| Error::NotFound(format!("Provider {id} not found")))?;
    let now = now_rfc3339();

    let new_name = name.unwrap_or(&existing.name);
    let new_website = website_url.unwrap_or(&existing.website_url);
    let new_icon = icon_key.or(existing.icon_key.as_deref());
    let new_enabled = enabled.unwrap_or(existing.enabled);

    conn.execute(
        "UPDATE ai_providers SET name = ?1, website_url = ?2, icon_key = ?3, enabled = ?4, updated_at = ?5
         WHERE id = ?6",
        params![
            new_name,
            new_website,
            new_icon,
            if new_enabled { 1 } else { 0 },
            now,
            id,
        ],
    )
    .map_err(Error::Database)?;

    get_provider(conn, id)?
        .ok_or_else(|| Error::Internal("Failed to read back updated provider".into()))
}

pub fn get_provider_delete_impact(conn: &DbConn, provider_id: &str) -> Result<DeleteImpact> {
    let connection_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM ai_connections WHERE provider_id = ?1",
            [provider_id],
            |r| r.get(0),
        )
        .map_err(Error::Database)?;

    let credential_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM ai_credentials WHERE provider_id = ?1",
            [provider_id],
            |r| r.get(0),
        )
        .map_err(Error::Database)?;

    let model_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM ai_models WHERE provider_id = ?1",
            [provider_id],
            |r| r.get(0),
        )
        .map_err(Error::Database)?;

    let affected_route_count: i64 = conn
        .query_row(
            "SELECT COUNT(DISTINCT route_id) FROM proxy_route_targets
             WHERE connection_id IN (SELECT id FROM ai_connections WHERE provider_id = ?1)",
            [provider_id],
            |r| r.get(0),
        )
        .map_err(Error::Database)?;

    Ok(DeleteImpact {
        connection_count: connection_count as usize,
        credential_count: credential_count as usize,
        model_count: model_count as usize,
        affected_route_count: affected_route_count as usize,
    })
}

pub fn delete_provider(conn: &DbConn, id: &str) -> Result<bool> {
    let count = conn
        .execute("DELETE FROM ai_providers WHERE id = ?1", [id])
        .map_err(Error::Database)?;
    Ok(count > 0)
}
