use crate::{env_manager, log_sanitizer, provider_key_manager, Error, Result};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use tauri::State;
use crate::AppState;

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

fn uuid_v4() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: [u8; 16] = rng.gen();
    let mut buf = bytes;
    buf[6] = (buf[6] & 0x0f) | 0x40;
    buf[8] = (buf[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        buf[0], buf[1], buf[2], buf[3], buf[4], buf[5],
        buf[6], buf[7], buf[8], buf[9], buf[10], buf[11], buf[12], buf[13], buf[14], buf[15],
    )
}

fn chrono_now() -> String {
    use chrono::Utc;
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn mask_secret(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= 8 {
        return "***".to_string();
    }
    let prefix: String = chars.iter().take(4).collect();
    let suffix: String = chars.iter().skip(chars.len() - 4).collect();
    format!("{}…{}", prefix, suffix)
}

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
            ON subagent_runs(subagent_id, started_at);"
    )?;
    ensure_column(conn, "subagents", "model_id", "TEXT NOT NULL DEFAULT ''")?;
    Ok(())
}

fn ensure_column(conn: &rusqlite::Connection, table: &str, column: &str, definition: &str) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({})", table))?;
    let cols = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if !cols.iter().any(|c| c == column) {
        conn.execute(&format!("ALTER TABLE {} ADD COLUMN {} {}", table, column, definition), [])?;
    }
    Ok(())
}

// ── CRUD Commands ──

#[tauri::command]
pub fn subagent_list(state: State<'_, AppState>) -> Result<Vec<Subagent>> {
    let conn = state.db.get()
        .map_err(|e| Error::Internal(format!("DB error: {e}")))?;
    ensure_tables(&*conn).map_err(|e| Error::Internal(e.to_string()))?;

    let mut stmt = conn.prepare(
        "SELECT id, name, role, instructions, tools, provider_id, provider_key_id, model_id,
                fallback_enabled, max_runs, enabled, created_at, updated_at
         FROM subagents ORDER BY name"
    ).map_err(|e| Error::Internal(e.to_string()))?;

    let agents = stmt.query_map([], |row| {
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
    }).map_err(|e| Error::Internal(e.to_string()))?
    .filter_map(|r| r.ok())
    .collect();
    Ok(agents)
}

#[tauri::command]
pub fn subagent_get(state: State<'_, AppState>, id: String) -> Result<Option<Subagent>> {
    let conn = state.db.get()
        .map_err(|e| Error::Internal(format!("DB error: {e}")))?;
    ensure_tables(&*conn).map_err(|e| Error::Internal(e.to_string()))?;
    let mut stmt = conn.prepare(
        "SELECT id, name, role, instructions, tools, provider_id, provider_key_id, model_id,
                fallback_enabled, max_runs, enabled, created_at, updated_at
         FROM subagents WHERE id = ?1"
    ).map_err(|e| Error::Internal(e.to_string()))?;

    let mut rows = stmt.query_map(params![id], |row| {
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
    }).map_err(|e| Error::Internal(e.to_string()))?;

    Ok(rows.next().and_then(|r| r.ok()))
}

#[tauri::command]
pub fn subagent_create(state: State<'_, AppState>, input: CreateSubagentInput) -> Result<Subagent> {
    let conn = state.db.get()
        .map_err(|e| Error::Internal(format!("DB error: {e}")))?;
    ensure_tables(&*conn).map_err(|e| Error::Internal(e.to_string()))?;

    let id = uuid_v4();
    let now = chrono_now();
    let model_id = input.model_id.unwrap_or_default();
    conn.execute(
        "INSERT INTO subagents (id, name, role, instructions, tools, provider_id, provider_key_id, model_id, fallback_enabled, max_runs, enabled, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 1, ?11, ?12)",
        params![id, input.name, input.role, input.instructions, input.tools,
                input.provider_id, input.provider_key_id, model_id,
                input.fallback_enabled.unwrap_or(false) as i64,
                input.max_runs.unwrap_or(10), now, now],
    ).map_err(|e| Error::Internal(e.to_string()))?;

    Ok(Subagent {
        id,
        name: input.name,
        role: input.role,
        instructions: input.instructions,
        tools: input.tools,
        provider_id: input.provider_id,
        provider_key_id: input.provider_key_id,
        model_id,
        fallback_enabled: input.fallback_enabled.unwrap_or(false),
        max_runs: input.max_runs.unwrap_or(10),
        enabled: true,
        created_at: now.clone(),
        updated_at: now,
    })
}

#[tauri::command]
pub fn subagent_update(state: State<'_, AppState>, input: UpdateSubagentInput) -> Result<()> {
    let conn = state.db.get()
        .map_err(|e| Error::Internal(format!("DB error: {e}")))?;
    ensure_tables(&*conn).map_err(|e| Error::Internal(e.to_string()))?;
    let now = chrono_now();
    conn.execute(
        "UPDATE subagents SET name=?1, role=?2, instructions=?3, tools=?4,
         provider_id=?5, provider_key_id=?6, model_id=?7, fallback_enabled=?8, max_runs=?9,
         enabled=?10, updated_at=?11 WHERE id=?12",
        params![input.name, input.role, input.instructions, input.tools,
                input.provider_id, input.provider_key_id, input.model_id.unwrap_or_default(),
                input.fallback_enabled as i64, input.max_runs,
                input.enabled as i64, now, input.id],
    ).map_err(|e| Error::Internal(e.to_string()))?;
    Ok(())
}

#[tauri::command]
pub fn subagent_delete(state: State<'_, AppState>, id: String) -> Result<()> {
    let conn = state.db.get()
        .map_err(|e| Error::Internal(format!("DB error: {e}")))?;
    ensure_tables(&*conn).map_err(|e| Error::Internal(e.to_string()))?;
    conn.execute("DELETE FROM subagents WHERE id = ?1", params![id])
        .map_err(|e| Error::Internal(e.to_string()))?;
    Ok(())
}

// ── Run Commands ──

#[tauri::command]
pub fn subagent_run(state: State<'_, AppState>, input: RunSubagentInput) -> Result<SubagentRun> {
    let conn = state.db.get()
        .map_err(|e| Error::Internal(format!("DB error: {e}")))?;
    ensure_tables(&*conn).map_err(|e| Error::Internal(e.to_string()))?;

    // Look up the subagent
    let agent = conn.query_row(
        "SELECT name, role, instructions, tools, provider_id, provider_key_id, model_id, fallback_enabled, enabled
         FROM subagents WHERE id = ?1",
        params![input.subagent_id],
        |row| Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, Option<String>>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, i64>(7)? != 0,
            row.get::<_, i64>(8)? != 0,
        ))
    ).map_err(|_| Error::Internal("Subagent not found".to_string()))?;

    if !agent.8 {
        return Err(Error::Internal("Subagent is disabled".to_string()));
    }
    if input.input_text.trim().is_empty() {
        return Err(Error::Internal("Run input cannot be empty".to_string()));
    }
    let provider_id = agent.4.clone().ok_or_else(|| Error::Internal("Select a provider before running this subagent".to_string()))?;
    let model_id = agent.6.trim().to_string();
    if model_id.is_empty() {
        return Err(Error::Internal("Select a model before running this subagent".to_string()));
    }

    let run_id = uuid_v4();
    let now = chrono_now();
    let provider_used = provider_id.clone();
    let key_label = agent.5.clone().unwrap_or_default();

    conn.execute(
        "INSERT INTO subagent_runs (id, subagent_id, status, input_text, started_at, provider_used, key_label)
         VALUES (?1, ?2, 'running', ?3, ?4, ?5, ?6)",
        params![run_id, input.subagent_id, input.input_text, now, provider_used, key_label],
    ).map_err(|e| Error::Internal(e.to_string()))?;

    let outcome = run_subagent_request(&conn, &run_id, &provider_id, agent.5.as_deref(), agent.7, &model_id, &agent.0, &agent.1, &agent.2, &agent.3, &input.input_text);
    let finished_at = chrono_now();

    // Release key lease if one was acquired
    let _ = crate::key_lease::release_key_lease(&conn, &run_id);

    let (status, output_text, error_text, final_provider, final_key_label) = match outcome {
        Ok(result) => ("completed".to_string(), result.output, String::new(), result.provider_name, result.key_label),
        Err(err) => ("failed".to_string(), String::new(), log_sanitizer::sanitize(&err.to_string()), provider_used, key_label),
    };

    conn.execute(
        "UPDATE subagent_runs SET status=?1, output_text=?2, error_text=?3, finished_at=?4, provider_used=?5, key_label=?6 WHERE id=?7",
        params![status, output_text, error_text, finished_at, final_provider, final_key_label, run_id],
    ).map_err(|e| Error::Internal(e.to_string()))?;

    Ok(SubagentRun {
        id: run_id,
        subagent_id: input.subagent_id,
        status,
        input_text: input.input_text,
        output_text,
        error_text,
        started_at: now,
        finished_at: Some(finished_at),
        provider_used: final_provider,
        key_label: final_key_label,
    })
}

struct SubagentExecutionResult {
    output: String,
    provider_name: String,
    key_label: String,
}

fn run_subagent_request(
    conn: &rusqlite::Connection,
    run_id: &str,
    provider_id: &str,
    preferred_key_id: Option<&str>,
    fallback_enabled: bool,
    model_id: &str,
    name: &str,
    role: &str,
    instructions: &str,
    tools: &str,
    input_text: &str,
) -> Result<SubagentExecutionResult> {
    let (provider_name, base_url): (String, String) = conn.query_row(
        "SELECT name, base_url FROM user_providers WHERE id = ?1",
        params![provider_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ).map_err(|e| Error::Internal(format!("Provider not found: {e}")))?;

    // Determine the key to use: lease a secondary key or use preferred key
    let (actual_key_id, candidate) = if let Some(key_id) = preferred_key_id {
        // Use the explicitly assigned key directly
        match load_key_candidate(conn, provider_id, key_id)? {
            Some(c) => (key_id.to_string(), c),
            None => return Err(Error::Internal("Assigned provider key not found".to_string())),
        }
    } else {
        // No preferred key — lease a non-primary secondary key
        let (leased_key_id, _) = crate::key_lease::acquire_secondary_key(conn, provider_id, run_id)?;
        let _label = conn.query_row(
            "SELECT label FROM provider_api_keys WHERE id = ?1",
            params![leased_key_id],
            |row| row.get::<_, String>(0),
        ).unwrap_or_else(|_| "leased".to_string());
        let candidate = load_key_candidate(conn, provider_id, &leased_key_id)?
            .ok_or_else(|| Error::Internal("Leased key not found".to_string()))?;
        (leased_key_id, candidate)
    };

    // First attempt with the selected key
    match execute_chat_completion(&base_url, &candidate.api_key, model_id, name, role, instructions, tools, input_text) {
        Ok(output) => Ok(SubagentExecutionResult {
            output,
            provider_name: provider_name.clone(),
            key_label: candidate.label,
        }),
        Err(err) => {
            let err_str = err.to_string();
            // Check if fallback is allowed and this is the first failure
            if fallback_enabled && !crate::key_lease::has_fallback_used(conn, run_id).unwrap_or(false) {
                // Mark the key as failed
                let _ = crate::key_lease::mark_key_failed(conn, &actual_key_id, "subagent_error", &log_sanitizer::sanitize(&err_str));
                // Mark fallback as used
                let _ = crate::key_lease::mark_fallback_used(conn, run_id);
                // Try with primary key
                if let Ok((primary_key_id, _)) = crate::key_lease::get_primary_key(conn, provider_id) {
                    if let Ok(Some(fallback_candidate)) = load_key_candidate(conn, provider_id, &primary_key_id) {
                        match execute_chat_completion(&base_url, &fallback_candidate.api_key, model_id, name, role, instructions, tools, input_text) {
                            Ok(output) => return Ok(SubagentExecutionResult {
                                output,
                                provider_name: provider_name.clone(),
                                key_label: format!("{} (fallback)", fallback_candidate.label),
                            }),
                            Err(fb_err) => {
                                return Err(Error::Internal(format!(
                                    "Primary key fallback also failed: {}",
                                    log_sanitizer::sanitize(&fb_err.to_string())
                                )));
                            }
                        }
                    }
                }
            }
            Err(Error::Internal(log_sanitizer::sanitize(&err_str)))
        }
    }
}

struct KeyCandidate {
    label: String,
    api_key: String,
}

fn load_key_candidate(conn: &rusqlite::Connection, provider_id: &str, key_id: &str) -> Result<Option<KeyCandidate>> {
    let row = conn.query_row(
        "SELECT label, api_key_encrypted, dek_encrypted FROM provider_api_keys WHERE id = ?1 AND provider_id = ?2",
        params![key_id, provider_id],
        |row| Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
        )),
    );
    let (label, encrypted, dek) = match row {
        Ok(row) => row,
        Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
        Err(e) => return Err(Error::Internal(e.to_string())),
    };
    let api_key = if let Some(dek) = dek {
        if !dek.is_empty() {
            provider_key_manager::envelope_decrypt(&encrypted, &dek, conn)?
        } else {
            let encryption_key = env_manager::get_encryption_key(conn)?;
            env_manager::decrypt(&encrypted, &encryption_key)?
        }
    } else {
        let encryption_key = env_manager::get_encryption_key(conn)?;
        env_manager::decrypt(&encrypted, &encryption_key)?
    };
    Ok(Some(KeyCandidate { label, api_key }))
}

fn execute_chat_completion(
    base_url: &str,
    api_key: &str,
    model_id: &str,
    name: &str,
    role: &str,
    instructions: &str,
    tools: &str,
    input_text: &str,
) -> Result<String> {
    let normalized = normalize_base_url(base_url)?;
    let url = format!("{}/chat/completions", normalized.trim_end_matches('/'));
    let system_prompt = format!(
        "You are subagent '{}'. Role: {}\nInstructions:\n{}\nAllowed tools/configuration:\n{}",
        name, role, instructions, tools
    );
    let body = serde_json::json!({
        "model": model_id,
        "messages": [
            { "role": "system", "content": system_prompt },
            { "role": "user", "content": input_text }
        ],
        "stream": false
    });
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| Error::Internal(format!("Failed to build HTTP client: {e}")))?;
    let resp = client
        .post(url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .map_err(|e| Error::Internal(format!("Subagent request failed: {e}")))?;
    let status = resp.status();
    let text = resp.text().unwrap_or_default();
    if !status.is_success() {
        return Err(Error::Internal(format!("Provider returned HTTP {}: {}", status, text.chars().take(300).collect::<String>())));
    }
    let json: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| Error::Internal(format!("Invalid provider JSON: {e}")))?;
    let output = json["choices"]
        .as_array()
        .and_then(|choices| choices.first())
        .and_then(|choice| choice["message"]["content"].as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if output.is_empty() {
        return Err(Error::Internal("Provider response did not include message content".to_string()));
    }
    Ok(output)
}

fn normalize_base_url(raw: &str) -> Result<String> {
    let trimmed = raw.trim().trim_end_matches('/').to_string();
    if trimmed.is_empty() {
        return Err(Error::Internal("Provider base URL is empty".to_string()));
    }
    if let Some(base) = trimmed.strip_suffix("/chat/completions") {
        let clean = base.trim_end_matches('/');
        if clean.ends_with("/v1") {
            Ok(clean.to_string())
        } else {
            Ok(format!("{}/v1", clean))
        }
    } else if let Some(base) = trimmed.strip_suffix("/v1/v1") {
        Ok(format!("{}/v1", base.trim_end_matches('/')))
    } else {
        Ok(trimmed)
    }
}

#[tauri::command]
pub fn subagent_list_runs(state: State<'_, AppState>, subagent_id: String) -> Result<Vec<SubagentRun>> {
    let conn = state.db.get()
        .map_err(|e| Error::Internal(format!("DB error: {e}")))?;
    ensure_tables(&*conn).map_err(|e| Error::Internal(e.to_string()))?;

    let mut stmt = conn.prepare(
        "SELECT id, subagent_id, status, input_text, output_text, error_text,
                started_at, finished_at, provider_used, key_label
         FROM subagent_runs WHERE subagent_id = ?1
         ORDER BY started_at DESC LIMIT 50"
    ).map_err(|e| Error::Internal(e.to_string()))?;

    let runs = stmt.query_map(params![subagent_id], |row| {
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
    }).map_err(|e| Error::Internal(e.to_string()))?
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
pub fn subagent_resolve_binding(state: State<'_, AppState>, subagent_id: String) -> Result<Option<SubagentBinding>> {
    let conn = state.db.get()
        .map_err(|e| Error::Internal(format!("DB error: {e}")))?;
    ensure_tables(&*conn).map_err(|e| Error::Internal(e.to_string()))?;

    let result = conn.query_row(
        "SELECT s.id, s.name, s.provider_id, COALESCE(p.name, ''), s.provider_key_id, COALESCE(k.label, ''), k.api_key_encrypted, k.dek_encrypted
         FROM subagents s
         LEFT JOIN user_providers p ON s.provider_id = p.id
         LEFT JOIN provider_api_keys k ON s.provider_key_id = k.id
         WHERE s.id = ?1",
        params![subagent_id],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, Option<String>>(7)?,
            ))
        }
    );

    match result {
        Ok((subagent_id, subagent_name, provider_id, provider_name, key_id, key_label, encrypted, dek)) => {
            let masked_key = match (encrypted, dek) {
                (Some(encrypted), Some(dek)) if !encrypted.is_empty() && !dek.is_empty() => {
                    mask_secret(&provider_key_manager::envelope_decrypt(&encrypted, &dek, &conn)?)
                }
                (Some(encrypted), _) if !encrypted.is_empty() => {
                    let encryption_key = env_manager::get_encryption_key(&conn)?;
                    mask_secret(&env_manager::decrypt(&encrypted, &encryption_key)?)
                }
                _ => "***".to_string(),
            };
            Ok(Some(SubagentBinding {
                subagent_id,
                subagent_name,
                provider_id,
                provider_name,
                key_id,
                key_label,
                masked_key,
            }))
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(Error::Internal(e.to_string())),
    }
}
