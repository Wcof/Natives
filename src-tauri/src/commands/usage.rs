use crate::{db, Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use tauri::State;

use crate::AppState;
use std::path::PathBuf;

// ── Frontend-facing usage structures ──
// These match the TypeScript types in src/types/agent.ts so the frontend
// can use the response directly without mapping.

/// Top-level usage response matching ClaudeUsage frontend expectations.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageResponse {
    pub claude: Option<ClaudeUsage>,
    /// Codex usage data (from ~/.codex/sessions)
    pub codex: Option<CodexUsage>,
    /// RTK command statistics (from ~/.natives/rtk-history.json)
    pub rtk: Option<RtkUsage>,
    pub history: Vec<UsageHistoryPoint>,
    pub model_stats: Vec<ModelStatUsage>,
    /// Whether a real data source (Claude/Codex local files) is configured and readable.
    pub source_configured: bool,
    /// Source breadcrumbs: list of file paths that were read to produce this data.
    /// Frontend can display these to satisfy the "no fake data" red line.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_breadcrumbs: Vec<String>,
    /// Optional error message when source is configured but unreadable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Claude usage matching src/types/agent.ts ClaudeUsage interface.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeUsage {
    /// Models breakdown (map of model name → usage)
    pub models: std::collections::HashMap<String, ModelTokenUsage>,
    /// Locally aggregated token stats
    pub local_tokens: LocalTokens,
    /// Activity stats
    pub activity: ActivityStats,
    /// Total requests
    pub total_requests: u64,
    /// Total cost in USD
    pub total_cost: Option<f64>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelTokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_input_tokens: u64,
    pub cache_creation_input_tokens: u64,
    pub cost_usd: f64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalTokens {
    pub today: u64,
    pub this_week: u64,
    pub total: u64,
    pub input: u64,
    pub output: u64,
    pub cache_creation: u64,
    pub cache_read: u64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityStats {
    pub total_sessions: u64,
    pub total_messages: u64,
    pub first_session_date: String,
}

/// Usage history point for trend charts.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageHistoryPoint {
    pub timestamp: i64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_tokens: u64,
    pub cache_read_tokens: u64,
    #[serde(default)]
    pub skills: u64,
}

/// Per-model usage stat (used in modelStats table).
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatUsage {
    pub model: String,
    pub request_count: u64,
    pub total_tokens: u64,
    pub total_cost: f64,
    pub avg_cost_per_request: f64,
}

/// Codex usage data parsed from ~/.codex/sessions/*/*.jsonl
#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CodexUsage {
    /// Total tokens used by Codex (aggregated from session files)
    pub total_tokens: u64,
    /// Total input tokens
    pub input_tokens: u64,
    /// Total output tokens
    pub output_tokens: u64,
    /// Total cache read tokens
    pub cache_read_tokens: u64,
    /// Total cache creation tokens
    pub cache_creation_tokens: u64,
    /// Today's token usage
    pub today_tokens: u64,
    /// This week's token usage
    pub week_tokens: u64,
    /// Total sessions count
    pub total_sessions: u64,
    /// Total cost in USD (estimated from token counts × model pricing)
    pub total_cost: f64,
    /// Per-model breakdown
    pub models: std::collections::HashMap<String, ModelTokenUsage>,
    /// Daily history points
    pub history: Vec<UsageHistoryPoint>,
}

/// RTK (CLI proxy) usage statistics parsed from ~/.natives/rtk-history.json
#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RtkUsage {
    /// Total tokens saved by RTK caching/proxying
    pub total_saved: u64,
    /// Total commands processed
    pub total_commands: u64,
    /// History of recent commands
    pub history: Vec<RtkCommandHistory>,
    /// Top commands by usage count
    pub top_commands: Vec<RtkCommandStat>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RtkCommandHistory {
    pub command: String,
    pub timestamp: i64,
    pub tokens_saved: u64,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RtkCommandStat {
    pub command: String,
    pub count: u64,
    pub total_saved: u64,
}

#[tauri::command]
pub fn usage_refresh(state: State<'_, AppState>) -> Result<JsonValue> {
    // Step 0: Check in-memory cache (30s TTL)
    {
        let cache_guard = state.usage_cache.lock().map_err(|e| Error::Internal(e.to_string()))?;
        if let Some((cached_value, cached_at)) = cache_guard.as_ref() {
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            if now_ms - *cached_at < 30_000 {
                return Ok(cached_value.clone());
            }
        }
    } // drop lock before heavy work

    let pool_conn = state.db.get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &*pool_conn;

    // Step 1: Read real usage from all available sources.
    // Each source pushes its own breadcrumb into the shared list.
    let mut source_breadcrumbs: Vec<String> = Vec::new();
    let claude_data = read_claude_usage_from_files(&mut source_breadcrumbs);
    let codex_data = read_codex_usage_from_files(&mut source_breadcrumbs);
    let rtk_data = read_rtk_usage_from_files(&mut source_breadcrumbs);

    // source_configured: true if at least one real source produced data.
    let source_configured = claude_data.is_some() || codex_data.is_some() || rtk_data.is_some();

    // Merge model_stats and history from Claude (primary) — Codex history is kept separately
    let (model_stats, history) = match &claude_data {
        Some(c) => (c.model_stats.clone(), c.history.clone()),
        None => (Vec::new(), Vec::new()),
    };

    let response = UsageResponse {
        claude: claude_data.as_ref().and_then(|resp| resp.claude.clone()),
        codex: codex_data,
        rtk: rtk_data,
        history,
        model_stats,
        source_configured,
        source_breadcrumbs,
        error: None,
    };

    // Step 2: Persist structured rows to usage_stats / skill_usage tables.
    // Also keep the legacy "usage:cached" setting for backwards compatibility.
    if let Err(e) = persist_usage_structured(conn, &response) {
        eprintln!("[usage] failed to persist structured rows: {}", e);
    }
    if let Err(e) = persist_usage_to_db(conn, &response) {
        eprintln!("[usage] failed to persist legacy cache: {}", e);
    }

    let result = serde_json::to_value(&response).map_err(|e| Error::Internal(e.to_string()))?;

    // Update in-memory cache
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    if let Ok(mut cache_guard) = state.usage_cache.lock() {
        *cache_guard = Some((result.clone(), now_ms));
    }

    // Drop response early so we don't hold it unnecessarily
    drop(response);
    Ok(result)
}

/// Try to read Claude usage from local stats-cache.json.
fn read_claude_usage_from_files(
    breadcrumbs: &mut Vec<String>,
) -> Option<UsageResponse> {
    let home = std::env::var("HOME").ok()?;
    let stats_path = PathBuf::from(&home).join(".claude").join("stats-cache.json");

    if !stats_path.exists() {
        return None;
    }

    // Record the source path so the frontend can show a breadcrumb.
    breadcrumbs.push(stats_path.to_string_lossy().to_string());

    let content = std::fs::read_to_string(&stats_path).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&content).ok()?;

    // Parse the stats-cache.json structure (Claude Code local format)
    // Expected: { modelUsage: { "model-id": { inputTokens, outputTokens, ... } }, dailyModelTokens: [...] }
    let models_map = parsed.get("modelUsage").and_then(|m| m.as_object())?;

    let mut model_stats = Vec::new();
    let mut total_input = 0u64;
    let mut total_output = 0u64;
    let mut total_cache_creation = 0u64;
    let mut total_cache_read = 0u64;
    let mut total_cost = 0.0f64;

    #[derive(Clone)]
    struct ModelProportion {
        input_ratio: f64,
        output_ratio: f64,
        cache_creation_ratio: f64,
        cache_read_ratio: f64,
    }

    let mut model_proportions = std::collections::HashMap::new();

    for (model_id, model_data) in models_map {
        let input: u64 = model_data.get("inputTokens").and_then(|v| v.as_u64()).unwrap_or(0);
        let output: u64 = model_data.get("outputTokens").and_then(|v| v.as_u64()).unwrap_or(0);
        let cache_creation: u64 = model_data.get("cacheCreationInputTokens").and_then(|v| v.as_u64()).unwrap_or(0);
        let cache_read: u64 = model_data.get("cacheReadInputTokens").and_then(|v| v.as_u64()).unwrap_or(0);
        let cost: f64 = model_data.get("costUSD").and_then(|v| v.as_f64()).unwrap_or(0.0);

        total_input += input;
        total_output += output;
        total_cache_creation += cache_creation;
        total_cache_read += cache_read;
        total_cost += cost;

        let total_model_tokens = input + output + cache_creation + cache_read;
        let prop = if total_model_tokens > 0 {
            ModelProportion {
                input_ratio: input as f64 / total_model_tokens as f64,
                output_ratio: output as f64 / total_model_tokens as f64,
                cache_creation_ratio: cache_creation as f64 / total_model_tokens as f64,
                cache_read_ratio: cache_read as f64 / total_model_tokens as f64,
            }
        } else {
            ModelProportion {
                input_ratio: 0.8,
                output_ratio: 0.2,
                cache_creation_ratio: 0.0,
                cache_read_ratio: 0.0,
            }
        };
        model_proportions.insert(model_id.clone(), prop);

        model_stats.push(ModelStatUsage {
            model: model_id.clone(),
            request_count: model_data.get("requestCount").and_then(|v| v.as_u64()).unwrap_or(0),
            total_tokens: total_model_tokens,
            total_cost: cost,
            avg_cost_per_request: if model_data.get("requestCount").and_then(|v| v.as_u64()).unwrap_or(0) > 0 {
                cost / (model_data.get("requestCount").and_then(|v| v.as_u64()).unwrap_or(0) as f64)
            } else {
                0.0
            },
        });
    }

    // Get today and week start string formatted as YYYY-MM-DD
    use chrono::Datelike;
    let now = chrono::Local::now();
    let today_str = now.format("%Y-%m-%d").to_string();
    let weekday = now.weekday();
    let days_from_monday = weekday.num_days_from_monday();
    let monday = now - chrono::Duration::days(days_from_monday as i64);
    let week_start_str = monday.format("%Y-%m-%d").to_string();

    let mut today_tokens = 0u64;
    let mut week_tokens = 0u64;
    let mut total_daily_tokens = 0u64;

    // Parse dailyModelTokens for history/trend
    let daily_tokens = parsed.get("dailyModelTokens").and_then(|a| a.as_array());

    // Build daily skill counts from Claude dailyActivity + Codex session files
    let mut daily_skill_counts = get_daily_skill_counts(&parsed);
    let codex_counts = get_codex_daily_skill_counts();
    for (date, count) in codex_counts {
        let entry = daily_skill_counts.entry(date).or_insert(0);
        *entry += count;
    }

    let mut history = Vec::new();
    if let Some(days) = daily_tokens {
        for day in days {
            if let Some(date_str) = day.get("date").and_then(|d| d.as_str()) {
                if let Ok(naive_date) = chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
                    let datetime = naive_date.and_hms_opt(0, 0, 0).unwrap();
                    let timestamp = datetime.and_utc().timestamp_millis();

                    let mut day_total = 0u64;
                    let mut day_input = 0u64;
                    let mut day_output = 0u64;
                    let mut day_cache_creation = 0u64;
                    let mut day_cache_read = 0u64;

                    if let Some(tokens_by_model) = day.get("tokensByModel").and_then(|t| t.as_object()) {
                        for (model_id, tokens_val) in tokens_by_model {
                            if let Some(tokens) = tokens_val.as_u64() {
                                day_total += tokens;

                                let prop = model_proportions.get(model_id).cloned().unwrap_or(ModelProportion {
                                    input_ratio: 0.8,
                                    output_ratio: 0.2,
                                    cache_creation_ratio: 0.0,
                                    cache_read_ratio: 0.0,
                                });

                                day_input += (tokens as f64 * prop.input_ratio).round() as u64;
                                day_output += (tokens as f64 * prop.output_ratio).round() as u64;
                                day_cache_creation += (tokens as f64 * prop.cache_creation_ratio).round() as u64;
                                day_cache_read += (tokens as f64 * prop.cache_read_ratio).round() as u64;
                            }
                        }
                    }

                    total_daily_tokens += day_total;
                    if date_str == today_str {
                        today_tokens = day_total;
                    }
                    if date_str >= week_start_str.as_str() {
                        week_tokens += day_total;
                    }

                    // Look up daily skill count from stats-cache dailyActivity
                    let day_skills = daily_skill_counts.get(date_str).copied().unwrap_or(0);

                    history.push(UsageHistoryPoint {
                        timestamp,
                        input_tokens: day_input,
                        output_tokens: day_output,
                        cache_creation_tokens: day_cache_creation,
                        cache_read_tokens: day_cache_read,
                        skills: day_skills,
                    });
                }
            }
        }
    }

    // Sort history by timestamp ascending
    history.sort_by_key(|h| h.timestamp);

    // Build the response — models_map_final from original parsed models data
    let mut models_map_final = std::collections::HashMap::new();
    for (model_id, model_data) in models_map {
        let input: u64 = model_data.get("inputTokens").and_then(|v| v.as_u64()).unwrap_or(0);
        let output: u64 = model_data.get("outputTokens").and_then(|v| v.as_u64()).unwrap_or(0);
        let cache_creation: u64 = model_data.get("cacheCreationInputTokens").and_then(|v| v.as_u64()).unwrap_or(0);
        let cache_read: u64 = model_data.get("cacheReadInputTokens").and_then(|v| v.as_u64()).unwrap_or(0);
        let cost: f64 = model_data.get("costUSD").and_then(|v| v.as_f64()).unwrap_or(0.0);

        models_map_final.insert(
            model_id.clone(),
            ModelTokenUsage {
                input_tokens: input,
                output_tokens: output,
                cache_creation_input_tokens: cache_creation,
                cache_read_input_tokens: cache_read,
                cost_usd: cost,
            },
        );
    }

    let total_messages = parsed.get("totalMessages").and_then(|v| v.as_u64()).unwrap_or(0);
    let total_requests_fallback = parsed.get("totalRequests").and_then(|v| v.as_u64()).unwrap_or(total_messages);

    let claude = ClaudeUsage {
        models: models_map_final,
        local_tokens: LocalTokens {
            today: today_tokens,
            this_week: week_tokens,
            total: if total_daily_tokens > 0 { total_daily_tokens } else { total_input + total_output + total_cache_creation + total_cache_read },
            input: total_input,
            output: total_output,
            cache_creation: total_cache_creation,
            cache_read: total_cache_read,
        },
        activity: ActivityStats {
            total_sessions: parsed.get("totalSessions").and_then(|v| v.as_u64()).unwrap_or(0),
            total_messages,
            first_session_date: parsed.get("firstSessionDate").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        },
        total_requests: total_requests_fallback,
        total_cost: Some(total_cost),
    };

    // Wrap model_stats and history inside the same UsageResponse shape for merging.
    Some(UsageResponse {
        claude: Some(claude),
        codex: None,
        rtk: None,
        history,
        model_stats,
        source_configured: true,
        source_breadcrumbs: vec![],
        error: None,
    })
}

/// Try to read Codex usage from ~/.codex/account.json and session files.
fn read_codex_usage_from_files(
    breadcrumbs: &mut Vec<String>,
) -> Option<CodexUsage> {
    let home = std::env::var("HOME").ok()?;

    // account.json is optional — parse it if it exists but don't require it
    let account_path = PathBuf::from(&home).join(".codex").join("account.json");
    if account_path.exists() {
        breadcrumbs.push(account_path.to_string_lossy().to_string());
        // Parse for possible future use (plan info, quotas)
        if let Ok(content) = std::fs::read_to_string(&account_path) {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&content) {
                let _five_hour = parsed.get("fiveHourWindow");
                let _plan_type = parsed.get("planType");
            }
        }
    }

    // Walk Codex sessions for token usage patterns.
    let sessions_dir = PathBuf::from(&home).join(".codex").join("sessions");
    let mut total_tokens: u64 = 0;
    let mut total_sessions: u64 = 0;
    let mut tot_input: u64 = 0;
    let mut tot_output: u64 = 0;
    let tot_cache_read: u64 = 0;
    let tot_cache_creation: u64 = 0;
    let mut today_tok: u64 = 0;
    let mut week_tok: u64 = 0;
    let models: std::collections::HashMap<String, ModelTokenUsage> = std::collections::HashMap::new();
    let mut history: Vec<UsageHistoryPoint> = Vec::new();

    use chrono::Datelike;
    let now = chrono::Local::now();
    let today_str = now.format("%Y-%m-%d").to_string();
    let weekday = now.weekday();
    let days_from_monday = weekday.num_days_from_monday();
    let monday = now - chrono::Duration::days(days_from_monday as i64);
    let week_start_str = monday.format("%Y-%m-%d").to_string();

    // If sessions dir doesn't exist AND account.json doesn't exist, return None.
    // If only sessions dir is missing but account.json exists, still return some data.
    if !sessions_dir.exists() && !account_path.exists() {
        return None;
    }

    if sessions_dir.exists() {
        breadcrumbs.push(sessions_dir.to_string_lossy().to_string());
        if let Ok(year_entries) = std::fs::read_dir(&sessions_dir) {
            for year_entry in year_entries.flatten() {
                let year_path = year_entry.path();
                if !year_path.is_dir() { continue; }
                let year_name = year_path.file_name().and_then(|n| n.to_str()).map(|s| s.to_string());
                let year_name = match year_name { Some(y) => y, None => continue };

                if let Ok(month_entries) = std::fs::read_dir(&year_path) {
                    for month_entry in month_entries.flatten() {
                        let month_path = month_entry.path();
                        if !month_path.is_dir() { continue; }
                        let month_name = month_path.file_name().and_then(|n| n.to_str()).map(|s| format!("{:0>2}", s));
                        let month_name = match month_name { Some(m) => m, None => continue };

                        if let Ok(day_entries) = std::fs::read_dir(&month_path) {
                            for day_entry in day_entries.flatten() {
                                let day_path = day_entry.path();
                                if !day_path.is_dir() { continue; }
                                let day_name = day_path.file_name().and_then(|n| n.to_str()).map(|s| format!("{:0>2}", s));
                                let day_name = match day_name { Some(d) => d, None => continue };

                                let date_str = format!("{}-{}-{}", year_name, month_name, day_name);
                                let mut day_tokens: u64 = 0;

                                if let Ok(file_entries) = std::fs::read_dir(&day_path) {
                                    for file_entry in file_entries.flatten() {
                                        let file_path = file_entry.path();
                                        if file_path.extension().and_then(|e| e.to_str()) != Some("jsonl") { continue; }

                                        // Try to get token usage from session metadata (first line of JSONL)
                                        if let Ok(content) = std::fs::read_to_string(&file_path) {
                                            if let Some(first_line) = content.lines().next() {
                                                if let Ok(session_meta) = serde_json::from_str::<serde_json::Value>(first_line) {
                                                    if let Some(usage) = session_meta.get("tokenUsage") {
                                                        let inp = usage.get("input").and_then(|v| v.as_u64()).unwrap_or(0);
                                                        let out = usage.get("output").and_then(|v| v.as_u64()).unwrap_or(0);
                                                        day_tokens += inp + out;
                                                        tot_input += inp;
                                                        tot_output += out;
                                                        // Only count valid sessions (have tokenUsage metadata)
                                                        total_sessions += 1;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }

                                total_tokens += day_tokens;
                                if date_str == today_str { today_tok = day_tokens; }
                                if date_str >= week_start_str { week_tok += day_tokens; }

                                if let Ok(naive_date) = chrono::NaiveDate::parse_from_str(&date_str, "%Y-%m-%d") {
                                    let datetime = naive_date.and_hms_opt(0, 0, 0).unwrap();
                                    history.push(UsageHistoryPoint {
                                        timestamp: datetime.and_utc().timestamp_millis(),
                                        input_tokens: day_tokens / 2, // approximate 50/50 split for Codex
                                        output_tokens: day_tokens / 2,
                                        cache_creation_tokens: 0,
                                        cache_read_tokens: 0,
                                        skills: 0,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    history.sort_by_key(|h| h.timestamp);

    // Estimate cost at ~$0.015 per 1K tokens (Claude Sonnet rate as proxy for Codex)
    let estimated_cost = total_tokens as f64 * 0.015 / 1000.0;

    Some(CodexUsage {
        total_tokens,
        input_tokens: tot_input,
        output_tokens: tot_output,
        cache_read_tokens: tot_cache_read,
        cache_creation_tokens: tot_cache_creation,
        today_tokens: today_tok,
        week_tokens: week_tok,
        total_sessions,
        total_cost: estimated_cost,
        models,
        history,
    })
}

/// Try to read RTK (CLI proxy) command history from ~/.natives/rtk-history.json.
fn read_rtk_usage_from_files(
    breadcrumbs: &mut Vec<String>,
) -> Option<RtkUsage> {
    let home = std::env::var("HOME").ok()?;
    let rtk_path = PathBuf::from(&home).join(".natives").join("rtk-history.json");

    if !rtk_path.exists() {
        return None;
    }
    breadcrumbs.push(rtk_path.to_string_lossy().to_string());

    let content = std::fs::read_to_string(&rtk_path).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&content).ok()?;

    // Expected format: { "totalSaved": 12345, "commands": [ { "command": "npm run build", "timestamp": ..., "tokensSaved": 123 }, ... ] }
    let total_saved = parsed.get("totalSaved").and_then(|v| v.as_u64()).unwrap_or(0);
    let commands = parsed.get("commands").and_then(|a| a.as_array());

    let mut history = Vec::new();
    let mut top_commands: std::collections::HashMap<String, (u64, u64)> = std::collections::HashMap::new();

    if let Some(cmd_list) = commands {
        for cmd_val in cmd_list {
            let cmd = cmd_val.get("command").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let ts = cmd_val.get("timestamp").and_then(|v| v.as_i64()).unwrap_or(0);
            let saved = cmd_val.get("tokensSaved").and_then(|v| v.as_u64()).unwrap_or(0);

            history.push(RtkCommandHistory {
                command: cmd.clone(),
                timestamp: ts,
                tokens_saved: saved,
            });

            let entry = top_commands.entry(cmd).or_insert((0, 0));
            entry.0 += 1;
            entry.1 += saved;
        }
    }

    let top_list: Vec<RtkCommandStat> = {
        let mut v: Vec<RtkCommandStat> = top_commands
            .into_iter()
            .map(|(cmd, (count, total))| RtkCommandStat {
                command: cmd,
                count,
                total_saved: total,
            })
            .collect();
        v.sort_by(|a, b| b.count.cmp(&a.count));
        v.truncate(20);
        v
    };

    let total_commands = history.len() as u64;

    Some(RtkUsage {
        total_saved,
        total_commands,
        history,
        top_commands: top_list,
    })
}

/// Persist usage data to structured DB rows (usage_stats table).
fn persist_usage_structured(conn: &rusqlite::Connection, usage: &UsageResponse) -> Result<()> {
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();

    // Persist Claude model stats per model
    if let Some(claude) = &usage.claude {
        let claude_source = usage.source_breadcrumbs.iter().find(|p| p.contains(".claude")).map(|s| s.as_str());
        for (model, model_usage) in &claude.models {
            db::upsert_usage_stat(
                conn,
                &today,
                "claude",
                claude_source,
                model,
                model_usage.input_tokens,
                model_usage.output_tokens,
                model_usage.cache_creation_input_tokens,
                model_usage.cache_read_input_tokens,
                0, // request_count not split per model daily in this schema yet
                model_usage.cost_usd,
            )?;
        }
        // Persist total cost as a separate summary row with model="_total"
        let total_cost = claude.total_cost.unwrap_or(0.0);
        if total_cost > 0.0 {
            db::upsert_usage_stat(
                conn,
                &today,
                "claude",
                claude_source,
                "_total",
                0, 0, 0, 0, 0, total_cost,
            )?;
        }
    }

    // Persist Codex stats
    if let Some(codex) = &usage.codex {
        let codex_source = usage.source_breadcrumbs.iter().find(|p| p.contains(".codex")).map(|s| s.as_str());
        db::upsert_usage_stat(
            conn,
            &today,
            "codex",
            codex_source,
            "_aggregate",
            codex.input_tokens,
            codex.output_tokens,
            codex.cache_creation_tokens,
            codex.cache_read_tokens,
            codex.total_sessions,
            codex.total_cost,
        )?;
    }

    // Persist skill usage from history into skill_usage table
    // Use UTC date directly — the timestamp was created as UTC midnight
    // (naive_date.and_hms_opt(0,0,0).and_utc()), so no timezone conversion needed.
    for hist in &usage.history {
        if hist.skills > 0 {
            let secs = hist.timestamp / 1000;
            if let Some(dt) = chrono::DateTime::from_timestamp(secs, 0) {
                let date_str = dt.format("%Y-%m-%d").to_string();
                let claude_source = usage.source_breadcrumbs.iter().find(|p| p.contains(".claude")).map(|s| s.as_str());
                db::upsert_skill_usage(
                    conn,
                    &date_str,
                    "claude-log",
                    claude_source,
                    "_aggregate",
                    hist.skills,
                )?;
            }
        }
    }

    Ok(())
}

/// Extract daily skill (tool call) counts from stats-cache.json dailyActivity.
///
/// stats-cache.json format:
/// ```json
/// { "dailyActivity": [{ "date": "2026-03-23", "toolCallCount": 13, ... }, ...] }
/// ```
/// Returns a HashMap mapping "YYYY-MM-DD" → toolCallCount.
fn get_daily_skill_counts(parsed: &serde_json::Value) -> std::collections::HashMap<String, u64> {
    let mut counts = std::collections::HashMap::new();
    if let Some(activity) = parsed.get("dailyActivity").and_then(|a| a.as_array()) {
        for entry in activity {
            let date = match entry.get("date").and_then(|d| d.as_str()) {
                Some(d) => d.to_string(),
                None => continue,
            };
            let count = entry.get("toolCallCount").and_then(|c| c.as_u64()).unwrap_or(0);
            counts.insert(date, count);
        }
    }
    counts
}

/// Extract daily tool call counts from Codex session JSONL files.
///
/// Codex stores sessions at ~/.codex/sessions/YYYY/MM/DD/*.jsonl.
/// Each session file contains event_msg entries; tool invocations
/// are logged as `exec_command_end` (shell commands) and `patch_apply_end`
/// (file edits). These are counted as Codex "skill" invocations per day.
///
/// Returns a HashMap mapping "YYYY-MM-DD" → tool call count.
fn get_codex_daily_skill_counts() -> std::collections::HashMap<String, u64> {
    let mut counts = std::collections::HashMap::new();
    let home = match std::env::var("HOME") {
        Ok(h) => h,
        Err(_) => return counts,
    };
    let sessions_dir = PathBuf::from(&home).join(".codex").join("sessions");
    if !sessions_dir.exists() {
        return counts;
    }

    // Walk the sessions/YYYY/MM/DD directory structure
    if let Ok(year_entries) = std::fs::read_dir(&sessions_dir) {
        for year_entry in year_entries.flatten() {
            let year_path = year_entry.path();
            if !year_path.is_dir() {
                continue;
            }
            let year_name = match year_path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };

            if let Ok(month_entries) = std::fs::read_dir(&year_path) {
                for month_entry in month_entries.flatten() {
                    let month_path = month_entry.path();
                    if !month_path.is_dir() {
                        continue;
                    }
                    let month_name = match month_path.file_name().and_then(|n| n.to_str()) {
                        Some(n) => format!("{:0>2}", n), // pad single-digit months
                        None => continue,
                    };

                    if let Ok(day_entries) = std::fs::read_dir(&month_path) {
                        for day_entry in day_entries.flatten() {
                            let day_path = day_entry.path();
                            if !day_path.is_dir() {
                                continue;
                            }
                            let day_name = match day_path.file_name().and_then(|n| n.to_str()) {
                                Some(n) => format!("{:0>2}", n),
                                None => continue,
                            };

                            let date_key = format!("{}-{}-{}", year_name, month_name, day_name);

                            // Scan all JSONL files in this day directory
                            if let Ok(file_entries) = std::fs::read_dir(&day_path) {
                                let mut day_count: u64 = 0;
                                for file_entry in file_entries.flatten() {
                                    let file_path = file_entry.path();
                                    if file_path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                                        continue;
                                    }

                                    // Parse JSONL and count tool events
                                    if let Ok(content) = std::fs::read_to_string(&file_path) {
                                        for line in content.lines() {
                                            if line.is_empty() {
                                                continue;
                                            }
                                            // Quick skip: non-tool-event lines usually don't contain "exec_command_end" or "patch_apply_end"
                                            if !line.contains("exec_command_end") && !line.contains("patch_apply_end") {
                                                continue;
                                            }
                                            // Parse the JSON line to confirm the type
                                            if let Ok(val) = serde_json::from_str::<serde_json::Value>(line) {
                                                if val.get("type").and_then(|t| t.as_str()) == Some("event_msg") {
                                                    let evt_type = val
                                                        .get("payload")
                                                        .and_then(|p| p.get("type"))
                                                        .and_then(|t| t.as_str())
                                                        .unwrap_or("");
                                                    if evt_type == "exec_command_end" || evt_type == "patch_apply_end" {
                                                        day_count += 1;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                                if day_count > 0 {
                                    counts.insert(date_key, day_count);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    counts
}

/// Persist usage response to DB (key: "usage:cached") for offline fallback.
fn persist_usage_to_db(conn: &rusqlite::Connection, usage: &UsageResponse) -> Result<()> {
    let json = serde_json::to_string(usage)
        .map_err(|e| Error::Internal(format!("failed to serialize usage: {e}")))?;
    db::set_setting(conn, "usage:cached", &json)
}
