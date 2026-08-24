//! Model 持久化操作。

use rusqlite::{params, Connection as DbConn};

use crate::ai::model::{Model, ModelAvailability, ModelSource};
use crate::{Error, Result};

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

pub fn list_models(
    conn: &DbConn,
    connection_id: Option<&str>,
    credential_id: Option<&str>,
) -> Result<Vec<Model>> {
    let (query, param): (&str, Option<&str>) = match (connection_id, credential_id) {
        (Some(cid), _) => (
            "SELECT id, provider_id, connection_id, source_credential_id, model_id, display_name, source, capabilities_json, availability, discovered_at, last_seen_at
             FROM ai_models WHERE connection_id = ?1 ORDER BY model_id ASC",
            Some(cid),
        ),
        (None, Some(crid)) => (
            "SELECT id, provider_id, connection_id, source_credential_id, model_id, display_name, source, capabilities_json, availability, discovered_at, last_seen_at
             FROM ai_models WHERE source_credential_id = ?1 ORDER BY model_id ASC",
            Some(crid),
        ),
        (None, None) => (
            "SELECT id, provider_id, connection_id, source_credential_id, model_id, display_name, source, capabilities_json, availability, discovered_at, last_seen_at
             FROM ai_models ORDER BY model_id ASC",
            None,
        ),
    };

    let mut stmt = conn.prepare(query).map_err(Error::Database)?;

    let map_row = |row: &rusqlite::Row| {
        let source_str: String = row.get(6)?;
        let avail_str: String = row.get(8)?;
        Ok(Model {
            id: row.get(0)?,
            provider_id: row.get(1)?,
            connection_id: row.get(2)?,
            source_credential_id: row.get(3)?,
            model_id: row.get(4)?,
            display_name: row.get(5)?,
            source: ModelSource::from_str(&source_str),
            capabilities_json: row.get(7)?,
            availability: ModelAvailability::from_str(&avail_str),
            discovered_at: row.get(9)?,
            last_seen_at: row.get(10)?,
        })
    };

    let rows = match param {
        Some(p) => stmt.query_map([p], map_row).map_err(Error::Database)?,
        None => stmt.query_map([], map_row).map_err(Error::Database)?,
    };

    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Error::Database)
}

pub fn upsert_model(
    conn: &DbConn,
    provider_id: Option<&str>,
    connection_id: Option<&str>,
    source_credential_id: Option<&str>,
    model_id: &str,
    display_name: &str,
    source: ModelSource,
    capabilities_json: Option<&str>,
    availability: ModelAvailability,
) -> Result<Model> {
    let now = now_rfc3339();
    let id = format!(
        "model:{}:{}:{}",
        connection_id.unwrap_or("none"),
        source_credential_id.unwrap_or("none"),
        model_id
    );

    conn.execute(
        "INSERT INTO ai_models (id, provider_id, connection_id, source_credential_id, model_id, display_name, source, capabilities_json, availability, discovered_at, last_seen_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)
         ON CONFLICT(id) DO UPDATE SET
             display_name = excluded.display_name,
             capabilities_json = excluded.capabilities_json,
             availability = excluded.availability,
             last_seen_at = excluded.last_seen_at",
        params![
            id,
            provider_id,
            connection_id,
            source_credential_id,
            model_id,
            display_name,
            source.as_str(),
            capabilities_json,
            availability.as_str(),
            now,
        ],
    )
    .map_err(Error::Database)?;

    let mut stmt = conn
        .prepare(
            "SELECT id, provider_id, connection_id, source_credential_id, model_id, display_name, source, capabilities_json, availability, discovered_at, last_seen_at
             FROM ai_models WHERE id = ?1",
        )
        .map_err(Error::Database)?;

    stmt.query_row([&id], |row| {
        let source_str: String = row.get(6)?;
        let avail_str: String = row.get(8)?;
        Ok(Model {
            id: row.get(0)?,
            provider_id: row.get(1)?,
            connection_id: row.get(2)?,
            source_credential_id: row.get(3)?,
            model_id: row.get(4)?,
            display_name: row.get(5)?,
            source: ModelSource::from_str(&source_str),
            capabilities_json: row.get(7)?,
            availability: ModelAvailability::from_str(&avail_str),
            discovered_at: row.get(9)?,
            last_seen_at: row.get(10)?,
        })
    })
    .map_err(Error::Database)
}

pub fn delete_model(conn: &DbConn, id: &str) -> Result<bool> {
    let count = conn
        .execute("DELETE FROM ai_models WHERE id = ?1", [id])
        .map_err(Error::Database)?;
    Ok(count > 0)
}
