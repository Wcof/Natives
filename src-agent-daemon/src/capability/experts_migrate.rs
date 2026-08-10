//! Host subagent migration (W9 split from capability/experts.rs).
//! `super` here is the `experts` module; helper fns are re-exported from it.

use super::{ensure_expert_exists, insert_expert, now_iso};
use crate::capability::store as capability_store;
use assistant_protocol::v2::credential::HostSubagentRow;
use rusqlite::params;
use serde_json::{json, Value};
use std::path::Path;

pub fn migrate_host_subagents() -> Result<u32, String> {
    {
        let data = capability_store()?;
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
    let resp =
        crate::natives_db_broker::NativesDbBroker::open_default()?.host_subagents("daemon-boot")?;
import_host_subagent_rows(resp.rows)
}

/// Import one batch of legacy Host subagent rows into the capability library.
/// Shared by the production broker lease path and the test fixture reader.
pub(crate) fn import_host_subagent_rows(
    rows: Vec<HostSubagentRow>,
) -> Result<u32, String> {
    let mut migrated = 0u32;
    for row in rows {
        let id = row.id;
        let name = row.name;
        let role = row.role.unwrap_or_default();
        let instructions = row.instructions.unwrap_or_default();
        let tools_raw = row.tools.unwrap_or_default();
        let provider_id = row.provider_id.unwrap_or_default();
        let key_id = row.provider_key_id;
        let model_id = row.model_id.unwrap_or_default();
        let enabled = row.enabled;
        let system_prompt = if !instructions.trim().is_empty() {
            instructions
        } else if !role.trim().is_empty() {
            role.clone()
        } else {
            format!("You are {name}.")
        };
        let tools: Vec<String> = tools_raw
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
                "providerId": if provider_id.is_empty() { Value::Null } else { json!(provider_id) },
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

/// Test-only fixture reader: reads a temp `subagents` table directly from a
/// test-created natives.db file. Never used by production — the production
/// path goes through the broker lease (`migrate_host_subagents`).
#[cfg(test)]
pub fn migrate_host_subagents_from(natives_db: &Path) -> Result<u32, String> {
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

    import_host_subagent_rows(
        rows.into_iter()
            .map(
                |(id, name, role, instructions, tools, provider_id, key_id, model_id, enabled)| {
                    HostSubagentRow {
                        id,
                        name,
                        role: Some(role),
                        instructions: Some(instructions),
                        tools: Some(tools),
                        provider_id,
                        provider_key_id: key_id,
                        model_id: Some(model_id),
                        enabled,
                    }
                },
            )
            .collect(),
    )
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------
