//! Provider/model catalog from Settings SoT (natives.db) with assistant.db fallback.
use crate::daemon::data::DataStore;
use serde_json::Value;
use std::sync::Arc;

use super::{error_response, success_response, RpcResponse};

pub(crate) async fn handle_provider_list(data_store: &Arc<DataStore>, _params: &Value) -> RpcResponse {
    // Prefer natives.db (Settings SoT). Fall back to assistant.db mirror only when
    // the main pool is unavailable (e.g. unit tests with :memory: DataStore).
    match list_providers_from_natives_db() {
        Ok(providers) => return success_response(serde_json::json!({ "providers": providers })),
        Err(err) => {
            // Soft fallback keeps fixture/unit tests working without a main pool.
            eprintln!("provider.list natives.db unavailable, falling back to mirror: {err}");
        }
    }

    // Fallback: assistant.db mirror (legacy / test paths). Still filter to
    // providers that currently have an active key so deleted settings rows
    // mirrored earlier do not reappear as selectable ghosts.
    let _ = data_store.migrate_legacy_provider_keys();
    let conn = data_store.conn();
    let mut stmt = match conn.prepare(
        "SELECT id, provider_type, display_name, api_base_url, health_status, default_model, created_at, updated_at
         FROM assistant_provider_configs ORDER BY display_name ASC",
    ) {
        Ok(s) => s,
        Err(e) => return error_response("DB_ERROR", &e.to_string()),
    };

    let provider_rows: Vec<(
        String,
        String,
        String,
        String,
        String,
        Option<String>,
        String,
        String,
    )> = match stmt.query_map([], |row| {
        Ok((
            row.get(0)?,
            row.get(1)?,
            row.get(2)?,
            row.get(3)?,
            row.get(4)?,
            row.get(5)?,
            row.get(6)?,
            row.get(7)?,
        ))
    }) {
        Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
        Err(e) => return error_response("DB_QUERY_ERROR", &e.to_string()),
    };

    let mut providers = Vec::with_capacity(provider_rows.len());
    for (
        id,
        provider_type,
        display_name,
        api_base_url,
        health_status,
        default_model,
        created_at,
        updated_at,
    ) in provider_rows
    {
        let has_active_key = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM assistant_provider_keys WHERE provider_id = ?1 AND is_active = 1)",
                rusqlite::params![id],
                |row| row.get::<_, bool>(0),
            )
            .unwrap_or(false);
        // Stale mirror rows without a live key must not appear in the picker.
        if !has_active_key {
            continue;
        }

        let mut model_stmt = match conn.prepare(
            "SELECT model_id, display_name, capabilities, context_window, max_output, source, discovered_at
             FROM assistant_model_cache
             WHERE provider_id = ?1 ORDER BY model_id ASC",
        ) {
            Ok(stmt) => stmt,
            Err(e) => return error_response("DB_ERROR", &e.to_string()),
        };
        let mut models: Vec<Value> = match model_stmt.query_map(rusqlite::params![id], |row| {
            let capabilities = row.get::<_, String>(2)?;
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "display_name": row.get::<_, Option<String>>(1)?,
                "capabilities": serde_json::from_str::<Value>(&capabilities)
                    .unwrap_or_else(|_| serde_json::json!({})),
                "context_window": row.get::<_, i64>(3)?,
                "max_output": row.get::<_, i64>(4)?,
                "source": row.get::<_, String>(5)?,
                "discovered_at": row.get::<_, String>(6)?,
            }))
        }) {
            Ok(rows) => rows.filter_map(|row| row.ok()).collect(),
            Err(e) => return error_response("DB_QUERY_ERROR", &e.to_string()),
        };

        // Model cache empty → surface default_model only (never invent catalog entries).
        if models.is_empty() {
            if let Some(dm) = default_model
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                models.push(serde_json::json!({
                    "id": dm,
                    "display_name": dm,
                    "capabilities": {},
                    "context_window": 0,
                    "max_output": 0,
                    "source": "default_model",
                    "discovered_at": created_at,
                }));
            }
        }

        providers.push(serde_json::json!({
            "id": id,
            "provider_type": provider_type,
            "display_name": display_name,
            "api_base_url": api_base_url,
            "health_status": health_status,
            "default_model": default_model,
            "has_active_key": true,
            "models": models,
            "created_at": created_at,
            "updated_at": updated_at,
        }));
    }

    success_response(serde_json::json!({ "providers": providers }))
}

/// Read providers exclusively from natives.db (`user_providers` + active keys).
/// Model list prefers assistant_model_cache when present; otherwise uses `default_model`.
pub(crate) fn list_providers_from_natives_db() -> std::result::Result<Vec<Value>, String> {
    let natives = crate::db::get_main_conn().map_err(|e| e.to_string())?;

    let mut pstmt = natives
        .prepare(
            "SELECT id, preset_name, api_protocol, name, website_url, base_url, default_model, created_at, updated_at
             FROM user_providers ORDER BY name ASC",
        )
        .map_err(|e| e.to_string())?;

    let provider_rows: Vec<(
        String,
        String,
        String,
        String,
        String,
        String,
        Option<String>,
        String,
        String,
    )> = pstmt
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
                row.get(8)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    // Active key presence from natives.db only.
    let mut kstmt = natives
        .prepare(
            "SELECT provider_id FROM provider_api_keys WHERE COALESCE(is_active, 1) = 1",
        )
        .map_err(|e| e.to_string())?;
    let active_providers: std::collections::HashSet<String> = kstmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    // Optional model cache from assistant.db (discovery results); never required.
    let model_rows: std::collections::HashMap<String, Vec<Value>> = match crate::db::get_assistant_db_conn()
    {
        Ok(assistant) => {
            let mut stmt = match assistant.prepare(
                "SELECT provider_id, model_id, display_name, capabilities, context_window, max_output, source, discovered_at
                 FROM assistant_model_cache ORDER BY model_id ASC",
            ) {
                Ok(s) => s,
                Err(_) => {
                    return Ok(assemble_natives_providers(provider_rows, &active_providers, &std::collections::HashMap::new()));
                }
            };
            let mut grouped: std::collections::HashMap<String, Vec<Value>> =
                std::collections::HashMap::new();
            if let Ok(rows) = stmt.query_map([], |row| {
                let capabilities = row.get::<_, String>(3).unwrap_or_else(|_| "{}".into());
                Ok((
                    row.get::<_, String>(0)?,
                    serde_json::json!({
                        "id": row.get::<_, String>(1)?,
                        "display_name": row.get::<_, Option<String>>(2)?,
                        "capabilities": serde_json::from_str::<Value>(&capabilities)
                            .unwrap_or_else(|_| serde_json::json!({})),
                        "context_window": row.get::<_, i64>(4).unwrap_or(0),
                        "max_output": row.get::<_, i64>(5).unwrap_or(0),
                        "source": row.get::<_, String>(6).unwrap_or_else(|_| "cache".into()),
                        "discovered_at": row.get::<_, String>(7).unwrap_or_default(),
                    }),
                ))
            }) {
                for row in rows.flatten() {
                    grouped.entry(row.0).or_default().push(row.1);
                }
            }
            grouped
        }
        Err(_) => std::collections::HashMap::new(),
    };

    Ok(assemble_natives_providers(
        provider_rows,
        &active_providers,
        &model_rows,
    ))
}

pub(crate) fn assemble_natives_providers(
    provider_rows: Vec<(
        String,
        String,
        String,
        String,
        String,
        String,
        Option<String>,
        String,
        String,
    )>,
    active_providers: &std::collections::HashSet<String>,
    model_rows: &std::collections::HashMap<String, Vec<Value>>,
) -> Vec<Value> {
    let mut providers = Vec::new();
    for (
        id,
        preset_name,
        api_protocol,
        name,
        _website_url,
        base_url,
        default_model,
        created_at,
        updated_at,
    ) in provider_rows
    {
        if !active_providers.contains(&id) {
            continue;
        }
        let mut models = model_rows.get(&id).cloned().unwrap_or_default();
        if models.is_empty() {
            if let Some(dm) = default_model
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                models.push(serde_json::json!({
                    "id": dm,
                    "display_name": dm,
                    "capabilities": {},
                    "context_window": 0,
                    "max_output": 0,
                    "source": "default_model",
                    "discovered_at": created_at,
                }));
            }
        }
        let provider_type = if !api_protocol.trim().is_empty() {
            api_protocol
        } else {
            preset_name
        };
        providers.push(serde_json::json!({
            "id": id,
            "provider_type": provider_type,
            "display_name": name,
            "api_base_url": base_url,
            "health_status": "unknown",
            "default_model": default_model,
            "has_active_key": true,
            "models": models,
            "created_at": created_at,
            "updated_at": updated_at,
        }));
    }
    providers
}

/// True when provider has an active key and model is either cached or the provider default.
pub(crate) fn provider_model_pair_available(
    provider_id: &str,
    model_id: &str,
    assistant_conn: &rusqlite::Connection,
) -> bool {
    // Prefer natives.db SoT.
    if let Ok(natives) = crate::db::get_main_conn() {
        let has_key: bool = natives
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM provider_api_keys
                    WHERE provider_id = ?1 AND COALESCE(is_active, 1) = 1
                )",
                rusqlite::params![provider_id],
                |row| row.get(0),
            )
            .unwrap_or(false);
        if !has_key {
            return false;
        }
        let default_model: Option<String> = natives
            .query_row(
                "SELECT default_model FROM user_providers WHERE id = ?1",
                rusqlite::params![provider_id],
                |row| row.get(0),
            )
            .ok()
            .flatten();
        if default_model
            .as_deref()
            .map(str::trim)
            .is_some_and(|dm| dm == model_id)
        {
            return true;
        }
        // Model cache is optional discovery data in assistant.db.
        if let Ok(assistant) = crate::db::get_assistant_db_conn() {
            return assistant
                .query_row(
                    "SELECT EXISTS(
                        SELECT 1 FROM assistant_model_cache
                        WHERE provider_id = ?1 AND model_id = ?2
                    )",
                    rusqlite::params![provider_id, model_id],
                    |row| row.get(0),
                )
                .unwrap_or(false);
        }
        return false;
    }

    // Unit-test / no-main-pool fallback: assistant mirror tables.
    assistant_conn
        .query_row(
            "SELECT EXISTS(
                SELECT 1 FROM assistant_model_cache model
                JOIN assistant_provider_keys key ON key.provider_id = model.provider_id AND key.is_active = 1
                WHERE model.provider_id = ?1 AND model.model_id = ?2
            )",
            rusqlite::params![provider_id, model_id],
            |row| row.get::<_, bool>(0),
        )
        .unwrap_or(false)
        || assistant_conn
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM assistant_provider_configs p
                    JOIN assistant_provider_keys k ON k.provider_id = p.id AND k.is_active = 1
                    WHERE p.id = ?1 AND p.default_model = ?2
                )",
                rusqlite::params![provider_id, model_id],
                |row| row.get::<_, bool>(0),
            )
            .unwrap_or(false)
}

// ─── Run gateway (host preflight + daemon orchestration) ───

