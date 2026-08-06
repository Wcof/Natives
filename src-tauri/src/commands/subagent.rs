use crate::AppState;
use crate::{Error, Result};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use tauri::State;

// ── Data types ──

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subagent {
    pub id: String,
    pub name: String,
    pub role: String,
    pub instructions: String,
    pub tools: String,
    pub provider_id: Option<String>,
    pub provider_key_id: Option<String>,
    pub model_id: String,
    pub fallback_enabled: bool,
    pub max_runs: i64,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentRun {
    pub id: String,
    pub subagent_id: String,
    pub status: String,
    pub input_text: String,
    pub output_text: String,
    pub error_text: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub provider_used: String,
    pub key_label: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSubagentInput {
    pub name: String,
    pub role: String,
    pub instructions: String,
    pub tools: String,
    pub provider_id: Option<String>,
    pub provider_key_id: Option<String>,
    pub model_id: Option<String>,
    pub fallback_enabled: Option<bool>,
    pub max_runs: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSubagentInput {
    pub id: String,
    pub name: String,
    pub role: String,
    pub instructions: String,
    pub tools: String,
    pub provider_id: Option<String>,
    pub provider_key_id: Option<String>,
    pub model_id: Option<String>,
    pub fallback_enabled: bool,
    pub max_runs: i64,
    pub enabled: bool,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSubagentInput {
    pub subagent_id: String,
    pub input_text: String,
}

// ── Helpers ──

pub fn ensure_tables(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS subagents (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            role TEXT NOT NULL DEFAULT '',
            instructions TEXT NOT NULL DEFAULT '',
            tools TEXT NOT NULL DEFAULT '',
            provider_id TEXT,
            provider_key_id TEXT,
            model_id TEXT NOT NULL DEFAULT '',
            fallback_enabled INTEGER NOT NULL DEFAULT 0,
            max_runs INTEGER NOT NULL DEFAULT 10,
            enabled INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS subagent_runs (
            id TEXT PRIMARY KEY,
            subagent_id TEXT NOT NULL REFERENCES subagents(id) ON DELETE CASCADE,
            status TEXT NOT NULL DEFAULT 'pending',
            input_text TEXT NOT NULL DEFAULT '',
            output_text TEXT NOT NULL DEFAULT '',
            error_text TEXT NOT NULL DEFAULT '',
            started_at TEXT NOT NULL,
            finished_at TEXT,
            provider_used TEXT NOT NULL DEFAULT '',
            key_label TEXT NOT NULL DEFAULT ''
        );
        CREATE INDEX IF NOT EXISTS idx_subagent_runs_agent
            ON subagent_runs(subagent_id, started_at);",
    )?;
    ensure_column(conn, "subagents", "model_id", "TEXT NOT NULL DEFAULT ''")?;
    Ok(())
}

fn ensure_column(
    conn: &rusqlite::Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({})", table))?;
    let cols = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if !cols.iter().any(|c| c == column) {
        conn.execute(
            &format!("ALTER TABLE {} ADD COLUMN {} {}", table, column, definition),
            [],
        )?;
    }
    Ok(())
}

// ── CRUD Commands ──

#[tauri::command]
pub fn subagent_list(state: State<'_, AppState>) -> Result<Vec<Subagent>> {
    let conn = state
        .db
        .get()
        .map_err(|e| Error::Internal(format!("DB error: {e}")))?;
    ensure_tables(&conn).map_err(|e| Error::Internal(e.to_string()))?;

    let mut stmt = conn
        .prepare(
            "SELECT id, name, role, instructions, tools, provider_id, provider_key_id, model_id,
                fallback_enabled, max_runs, enabled, created_at, updated_at
         FROM subagents ORDER BY name",
        )
        .map_err(|e| Error::Internal(e.to_string()))?;

    let agents = stmt
        .query_map([], |row| {
            Ok(Subagent {
                id: row.get(0)?,
                name: row.get(1)?,
                role: row.get(2)?,
                instructions: row.get(3)?,
                tools: row.get(4)?,
                provider_id: row.get(5)?,
                provider_key_id: row.get(6)?,
                model_id: row.get(7)?,
                fallback_enabled: row.get::<_, i64>(8)? != 0,
                max_runs: row.get(9)?,
                enabled: row.get::<_, i64>(10)? != 0,
                created_at: row.get(11)?,
                updated_at: row.get(12)?,
            })
        })
        .map_err(|e| Error::Internal(e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(agents)
}

#[tauri::command]
pub fn subagent_get(state: State<'_, AppState>, id: String) -> Result<Option<Subagent>> {
    let conn = state
        .db
        .get()
        .map_err(|e| Error::Internal(format!("DB error: {e}")))?;
    ensure_tables(&conn).map_err(|e| Error::Internal(e.to_string()))?;
    let mut stmt = conn
        .prepare(
            "SELECT id, name, role, instructions, tools, provider_id, provider_key_id, model_id,
                fallback_enabled, max_runs, enabled, created_at, updated_at
         FROM subagents WHERE id = ?1",
        )
        .map_err(|e| Error::Internal(e.to_string()))?;

    let mut rows = stmt
        .query_map(params![id], |row| {
            Ok(Subagent {
                id: row.get(0)?,
                name: row.get(1)?,
                role: row.get(2)?,
                instructions: row.get(3)?,
                tools: row.get(4)?,
                provider_id: row.get(5)?,
                provider_key_id: row.get(6)?,
                model_id: row.get(7)?,
                fallback_enabled: row.get::<_, i64>(8)? != 0,
                max_runs: row.get(9)?,
                enabled: row.get::<_, i64>(10)? != 0,
                created_at: row.get(11)?,
                updated_at: row.get(12)?,
            })
        })
        .map_err(|e| Error::Internal(e.to_string()))?;

    Ok(rows.next().and_then(|r| r.ok()))
}

/// ADR-0016 retirement Phase A: the capability library (daemon
/// `capability.expert.*`) is the sole expert authority. Host write commands
/// fail closed so no new rows appear after the one-shot migration; reads stay
/// available as an archive until Phase B removes this module.
const RETIRED: &str =
    "subagent commands are retired (ADR-0016): manage experts in the capability hub (能力库)";

#[tauri::command]
pub fn subagent_create(
    _state: State<'_, AppState>,
    _input: CreateSubagentInput,
) -> Result<Subagent> {
    Err(Error::Internal(RETIRED.into()))
}

#[tauri::command]
pub fn subagent_update(_state: State<'_, AppState>, _input: UpdateSubagentInput) -> Result<()> {
    Err(Error::Internal(RETIRED.into()))
}

#[tauri::command]
pub fn subagent_delete(_state: State<'_, AppState>, _id: String) -> Result<()> {
    Err(Error::Internal(RETIRED.into()))
}

// ── Run Commands ──

#[tauri::command]
pub fn subagent_run(_state: State<'_, AppState>, _input: RunSubagentInput) -> Result<SubagentRun> {
    Err(Error::Internal(RETIRED.into()))
}

#[tauri::command]
pub fn subagent_list_runs(
    state: State<'_, AppState>,
    subagent_id: String,
) -> Result<Vec<SubagentRun>> {
    let conn = state
        .db
        .get()
        .map_err(|e| Error::Internal(format!("DB error: {e}")))?;
    ensure_tables(&conn).map_err(|e| Error::Internal(e.to_string()))?;

    let mut stmt = conn
        .prepare(
            "SELECT id, subagent_id, status, input_text, output_text, error_text,
                started_at, finished_at, provider_used, key_label
         FROM subagent_runs WHERE subagent_id = ?1
         ORDER BY started_at DESC LIMIT 50",
        )
        .map_err(|e| Error::Internal(e.to_string()))?;

    let runs = stmt
        .query_map(params![subagent_id], |row| {
            Ok(SubagentRun {
                id: row.get(0)?,
                subagent_id: row.get(1)?,
                status: row.get(2)?,
                input_text: row.get(3)?,
                output_text: row.get(4)?,
                error_text: row.get(5)?,
                started_at: row.get(6)?,
                finished_at: row.get(7)?,
                provider_used: row.get(8)?,
                key_label: row.get(9)?,
            })
        })
        .map_err(|e| Error::Internal(e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(runs)
}

// ── Provider binding resolver (key isolation per-agent) ──

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentBinding {
    pub subagent_id: String,
    pub subagent_name: String,
    pub provider_id: Option<String>,
    pub provider_name: String,
    pub key_id: Option<String>,
    pub key_label: String,
    pub masked_key: String,
}

#[tauri::command]
pub fn subagent_resolve_binding(
    _state: State<'_, AppState>,
    _subagent_id: String,
) -> Result<Option<SubagentBinding>> {
    // Retired with the provider-direct execution line: no credential material
    // (even masked) is decrypted for a dead path.
    Err(Error::Internal(RETIRED.into()))
}
