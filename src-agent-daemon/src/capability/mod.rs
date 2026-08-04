//! Capability library store (ADR-0016).
//!
//! Authoritative CRUD surface for skills metadata, MCP connectors and experts
//! / expert teams, backed by `assistant.db` (migration 021). This module is
//! also the "trusted configuration source" that `rpc.rs` mcp.start requires:
//! [`bootstrap`] registers enabled connector configs into the global MCP
//! runtime at daemon startup.
//!
//! Secrets never live here: connector `env` values may carry `secret:<id>`
//! references resolved by the Host-side encrypted store at spawn time, and
//! plaintext Authorization headers are rejected at the validation boundary.

pub mod experts;
pub mod hub;
pub mod mcp;
pub mod skills;

#[cfg(test)]
mod tests;

use crate::storage::DataStore;
use serde_json::Value;
use std::path::PathBuf;

pub(crate) fn store() -> Result<DataStore, String> {
    #[cfg(test)]
    let _env_guard = crate::storage::DataStore::env_test_lock();
    #[cfg(test)]
    if let Some((db_path, artifact_dir)) = crate::storage::test_db_override() {
        if let Some(parent) = db_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        return DataStore::new(&db_path, &artifact_dir);
    }
    let db_path = std::env::var("NATIVES_ASSISTANT_DB_PATH")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var("NATIVES_DB_PATH")
                .ok()
                .filter(|s| !s.trim().is_empty())
                .map(PathBuf::from)
        });
    #[cfg(test)]
    let db_path = db_path.ok_or_else(|| {
        "test store() requires NATIVES_ASSISTANT_DB_PATH or NATIVES_DB_PATH (refusing ~/.natives default)".to_string()
    })?;
    #[cfg(not(test))]
    let db_path = db_path.unwrap_or_else(crate::default_assistant_db_path);
    if let Some(parent) = db_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let artifact_dir = std::env::var("NATIVES_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            PathBuf::from(home).join(".natives").join("runtime")
        })
        .join("artifacts");
    DataStore::new(&db_path, &artifact_dir)
}

/// RPC dispatch for the `capability.*` configuration surface.
pub async fn request(method: &str, params: Value) -> Result<Value, String> {
    use assistant_protocol::v2::methods::names;
    match method {
        names::CAPABILITY_SKILL_LIST => skills::list(&params),
        names::CAPABILITY_SKILL_GET => skills::get(&params),
        names::CAPABILITY_SKILL_UPDATE => skills::update(&params),
        names::CAPABILITY_SKILL_DELETE => skills::delete(&params),
        names::CAPABILITY_SKILL_RESCAN => skills::rescan(&params),
        names::CAPABILITY_SKILL_IMPORT => skills::import(&params),
        names::CAPABILITY_MCP_LIST => mcp::list(&params),
        names::CAPABILITY_MCP_GET => mcp::get(&params),
        names::CAPABILITY_MCP_CREATE => mcp::create(&params),
        names::CAPABILITY_MCP_UPDATE => mcp::update(&params),
        names::CAPABILITY_MCP_DELETE => mcp::delete(&params),
        names::CAPABILITY_MCP_IMPORT_JSON => mcp::import_json(&params),
        names::CAPABILITY_MCP_HUB_SEARCH => hub::search(&params).await,
        names::CAPABILITY_MCP_HUB_GET => hub::get(&params).await,
        names::CAPABILITY_MCP_HUB_INSTALL => hub::install(&params).await,
        names::CAPABILITY_EXPERT_LIST => experts::list(&params),
        names::CAPABILITY_EXPERT_GET => experts::get(&params),
        names::CAPABILITY_EXPERT_CREATE => experts::create(&params),
        names::CAPABILITY_EXPERT_UPDATE => experts::update(&params),
        names::CAPABILITY_EXPERT_DELETE => experts::delete(&params),
        names::CAPABILITY_EXPERT_IMPORT_MD => experts::import_md(&params),
        names::CAPABILITY_EXPERT_EXPORT_MD => experts::export_md(&params),
        names::CAPABILITY_TEAM_LIST => experts::team_list(&params),
        names::CAPABILITY_TEAM_GET => experts::team_get(&params),
        names::CAPABILITY_TEAM_CREATE => experts::team_create(&params),
        names::CAPABILITY_TEAM_UPDATE => experts::team_update(&params),
        names::CAPABILITY_TEAM_DELETE => experts::team_delete(&params),
        other => Err(format!("unsupported capability method: {other}")),
    }
}

/// Trusted configuration source: register enabled MCP connectors into the
/// global runtime and refresh the skill catalogue. Called once at daemon
/// startup; individual CRUD operations keep the runtime in sync afterwards.
pub fn bootstrap() {
    match mcp::enabled_runtime_configs() {
        Ok(configs) => {
            let runtime = crate::mcp_runtime::global_mcp();
            for config in configs {
                let id = config.id.clone();
                if let Err(error) = runtime.register_server(config) {
                    eprintln!("[capability] bootstrap: register mcp '{id}' failed: {error}");
                }
            }
        }
        Err(error) => eprintln!("[capability] bootstrap: load mcp configs failed: {error}"),
    }
    if let Err(error) = skills::rescan(&Value::Null) {
        eprintln!("[capability] bootstrap: skill rescan failed: {error}");
    }
    // One-shot Host legacy subagent import (ADR-0016 retirement Phase A).
    match experts::migrate_host_subagents() {
        Ok(0) => {}
        Ok(n) => println!("[capability] migrated {n} host subagents into the capability library"),
        Err(error) => eprintln!("[capability] host subagent migration failed: {error}"),
    }
}
