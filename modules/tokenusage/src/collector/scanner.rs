//! scanner: 本地多工具增量扫描器与会话/日聚合归档。

use crate::collector::pricing::calculate_cost_micros;
use crate::collector::KNOWN_SOURCES;
use crate::storage::Store;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Clone)]
pub struct ScanSummary {
    pub scanned_sources: usize,
    pub new_records: usize,
    pub total_tokens: i64,
    pub total_cost_micros: i64,
}

pub fn seed_sources(store: &Store) -> Result<(), String> {
    let now = chrono_now();
    store.with_write(|conn| {
        let mut stmt = conn
            .prepare(
                "INSERT OR IGNORE INTO usage_sources (id, display_name, category, enabled, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )
            .map_err(|e| e.to_string())?;

        for src in KNOWN_SOURCES {
            stmt.execute([
                src.id,
                src.display_name,
                src.category,
                if src.default_enabled { "1" } else { "0" },
                &now,
            ])
            .map_err(|e| e.to_string())?;
        }
        Ok(())
    })
}

pub fn scan_all_sources(
    store: &Store,
    home_dir_override: Option<&Path>,
) -> Result<ScanSummary, String> {
    seed_sources(store)?;

    let home = home_dir_override
        .map(|p| p.to_path_buf())
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."));

    let mut summary = ScanSummary::default();

    for src in KNOWN_SOURCES {
        if !src.default_enabled {
            continue;
        }

        let mut source_paths = Vec::new();
        for rel in src.relative_paths {
            let p = home.join(rel);
            if p.exists() {
                source_paths.push(p);
            }
        }

        if source_paths.is_empty() {
            continue;
        }

        summary.scanned_sources += 1;
        for p in source_paths {
            if let Ok(count) = scan_directory(store, src.id, &p) {
                summary.new_records += count;
            }
        }

        // 更新 source 扫描时间
        let now = chrono_now();
        let _ = store.with_write(|conn| {
            conn.execute(
                "UPDATE usage_sources SET last_scanned_at = ?1, status = 'ok' WHERE id = ?2",
                [&now, src.id],
            )
            .map_err(|e| e.to_string())
        });
    }

    Ok(summary)
}

fn scan_directory(store: &Store, source_id: &str, dir: &Path) -> Result<usize, String> {
    scan_directory_bounded(store, source_id, dir, 0, 5)
}

fn scan_directory_bounded(
    store: &Store,
    source_id: &str,
    dir: &Path,
    current_depth: usize,
    max_depth: usize,
) -> Result<usize, String> {
    if current_depth > max_depth {
        return Ok(0);
    }
    let mut count = 0;
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(0),
    };

    for entry in entries.flatten() {
        // 防范符号链接越界与无限循环
        if entry.file_type().map(|ft| ft.is_symlink()).unwrap_or(false) {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            count += scan_directory_bounded(store, source_id, &path, current_depth + 1, max_depth)?;
        } else if is_parsable_file(&path) {
            count += parse_and_ingest_file(store, source_id, &path)?;
        }
    }
    Ok(count)
}

fn is_parsable_file(p: &Path) -> bool {
    let ext = p.extension().and_then(|s| s.to_str()).unwrap_or("");
    ext == "jsonl" || ext == "json"
}

#[derive(Debug, serde::Deserialize)]
struct RawUsageRecord {
    #[serde(default, alias = "sessionId")]
    session_id: Option<String>,
    #[serde(default)]
    turn_id: Option<String>,
    // 扁平字段（自定义/通用格式）
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    input_tokens: Option<i64>,
    #[serde(default)]
    output_tokens: Option<i64>,
    #[serde(default)]
    cache_read_tokens: Option<i64>,
    #[serde(default)]
    cache_write_tokens: Option<i64>,
    #[serde(default)]
    reasoning_tokens: Option<i64>,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    project_path: Option<String>,
    // Claude Code JSONL 嵌套结构：{ message: { model, usage: {...} }, ... }
    #[serde(default)]
    message: Option<NestedMessage>,
    // Codex JSONL 事件结构：{ payload: { type: "token_count", info: { total_token_usage: {...} } } }
    #[serde(default)]
    payload: Option<CodexPayload>,
}

#[derive(Debug, serde::Deserialize)]
struct CodexPayload {
    #[serde(default)]
    info: Option<CodexTokenInfo>,
}

#[derive(Debug, serde::Deserialize)]
struct CodexTokenInfo {
    #[serde(default)]
    total_token_usage: Option<CodexUsage>,
    #[serde(default)]
    last_token_usage: Option<CodexUsage>,
}

impl CodexTokenInfo {
    // last_token_usage 是本条事件的增量用量；total_token_usage 是会话累计口径，
    // 逐行累加会重复统计，因此优先取 last，回退 total。
    fn preferred(&self) -> Option<&CodexUsage> {
        self.last_token_usage
            .as_ref()
            .or(self.total_token_usage.as_ref())
    }
}

// Codex total_token_usage / last_token_usage 字段名
#[derive(Debug, serde::Deserialize)]
struct CodexUsage {
    #[serde(default)]
    input_tokens: Option<i64>,
    #[serde(default)]
    output_tokens: Option<i64>,
    #[serde(default)]
    cached_input_tokens: Option<i64>,
    #[serde(default)]
    cache_write_input_tokens: Option<i64>,
    #[serde(default)]
    reasoning_output_tokens: Option<i64>,
}

#[derive(Debug, serde::Deserialize)]
struct NestedMessage {
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    usage: Option<NestedUsage>,
}

#[derive(Debug, serde::Deserialize)]
struct NestedUsage {
    #[serde(default, alias = "inputTokens")]
    input_tokens: Option<i64>,
    #[serde(default, alias = "outputTokens")]
    output_tokens: Option<i64>,
    // Claude Code 命名
    #[serde(default, alias = "cache_read_input_tokens")]
    cache_read_tokens: Option<i64>,
    #[serde(default, alias = "cache_creation_input_tokens")]
    cache_write_tokens: Option<i64>,
    // OpenAI Responses 命名
    #[serde(default, alias = "cached_input_tokens")]
    cached_input_tokens: Option<i64>,
    #[serde(default, alias = "reasoning_output_tokens")]
    reasoning_tokens: Option<i64>,
    #[serde(default, alias = "output_tokens_details")]
    output_tokens_details: Option<NestedOutputDetails>,
}

#[derive(Debug, serde::Deserialize)]
struct NestedOutputDetails {
    #[serde(default, alias = "thinkingTokens")]
    thinking_tokens: Option<i64>,
}

fn parse_and_ingest_file(store: &Store, source_id: &str, path: &Path) -> Result<usize, String> {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return Ok(0),
    };

    let reader = BufReader::new(file);
    let mut records = Vec::new();
    let file_stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown");

    const MAX_LINES_PER_FILE: usize = 5000;
    let mut lines_read = 0;

    for line_res in reader.lines() {
        if lines_read >= MAX_LINES_PER_FILE {
            break;
        }
        lines_read += 1;

        let line = match line_res {
            Ok(l) => l,
            Err(_) => continue,
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if let Ok(raw) = serde_json::from_str::<RawUsageRecord>(trimmed) {
            // 嵌套（Claude Code JSONL）优先，扁平字段兜底
            let (nested_model, nested_usage) = raw
                .message
                .map(|m| (m.model, m.usage))
                .unwrap_or((None, None));
            let usage = nested_usage.unwrap_or(NestedUsage {
                input_tokens: None,
                output_tokens: None,
                cache_read_tokens: None,
                cache_write_tokens: None,
                cached_input_tokens: None,
                reasoning_tokens: None,
                output_tokens_details: None,
            });
            let session_id = raw
                .session_id
                .unwrap_or_else(|| format!("{}_{}", source_id, file_stem));
            let model = raw
                .model
                .or(nested_model)
                .unwrap_or_else(|| "unknown".into());
            let input = raw.input_tokens.or(usage.input_tokens).unwrap_or(0);
            let output = raw.output_tokens.or(usage.output_tokens).unwrap_or(0);
            let cache_read = raw
                .cache_read_tokens
                .or(usage.cache_read_tokens)
                .or(usage.cached_input_tokens)
                .unwrap_or(0);
            let cache_write = raw
                .cache_write_tokens
                .or(usage.cache_write_tokens)
                .unwrap_or(0);
            let reasoning = raw
                .reasoning_tokens
                .or(usage.reasoning_tokens)
                .or_else(|| usage.output_tokens_details.and_then(|d| d.thinking_tokens))
                .unwrap_or(0);
            // Codex 事件结构兜底：payload.info.{total,last}_token_usage
            let codex_usage = raw
                .payload
                .as_ref()
                .and_then(|p| p.info.as_ref())
                .and_then(|i| i.preferred())
                .map(|u| {
                    (
                        u.input_tokens,
                        u.output_tokens,
                        u.cached_input_tokens,
                        u.cache_write_input_tokens,
                        u.reasoning_output_tokens,
                    )
                });
            let (c_in, c_out, c_cache_read, c_cache_write, c_reasoning) =
                codex_usage.unwrap_or((None, None, None, None, None));
            let input = if input == 0 { c_in.unwrap_or(0) } else { input };
            let output = if output == 0 {
                c_out.unwrap_or(0)
            } else {
                output
            };
            let cache_read = if cache_read == 0 {
                c_cache_read.unwrap_or(0)
            } else {
                cache_read
            };
            let cache_write = if cache_write == 0 {
                c_cache_write.unwrap_or(0)
            } else {
                cache_write
            };
            let reasoning = if reasoning == 0 {
                c_reasoning.unwrap_or(0)
            } else {
                reasoning
            };
            let recorded_at = raw.timestamp.unwrap_or_else(chrono_now);
            let turn_id = raw.turn_id;
            let title = raw.title.unwrap_or_default();
            let project = raw.project_path.unwrap_or_default();

            // 计算费用
            let cost_micros =
                calculate_cost_micros(&model, input, output, cache_read, cache_write, reasoning);

            records.push((
                session_id,
                turn_id,
                model,
                input,
                output,
                cache_read,
                cache_write,
                reasoning,
                cost_micros,
                recorded_at,
                title,
                project,
            ));
        }
    }

    if records.is_empty() {
        return Ok(0);
    }

    let mut inserted_count = 0;
    store.with_write(|conn| {
        let mut rec_stmt = conn.prepare(
            "INSERT OR IGNORE INTO usage_records (
                record_hash, source_id, session_id, turn_id, model,
                input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
                reasoning_tokens, cost_micros, recorded_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)"
        ).map_err(|e| e.to_string())?;

        let mut sess_stmt = conn.prepare(
            "INSERT INTO sessions (
                session_id, source_id, title, project_path, started_at, last_used_at,
                total_input_tokens, total_output_tokens, total_cache_read_tokens, total_cache_write_tokens,
                total_reasoning_tokens, total_cost_micros, message_count, created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 1, ?5, ?6)
            ON CONFLICT(session_id) DO UPDATE SET
                last_used_at = MAX(sessions.last_used_at, excluded.last_used_at),
                total_input_tokens = sessions.total_input_tokens + excluded.total_input_tokens,
                total_output_tokens = sessions.total_output_tokens + excluded.total_output_tokens,
                total_cache_read_tokens = sessions.total_cache_read_tokens + excluded.total_cache_read_tokens,
                total_cache_write_tokens = sessions.total_cache_write_tokens + excluded.total_cache_write_tokens,
                total_reasoning_tokens = sessions.total_reasoning_tokens + excluded.total_reasoning_tokens,
                total_cost_micros = sessions.total_cost_micros + excluded.total_cost_micros,
                message_count = sessions.message_count + 1,
                updated_at = excluded.updated_at"
        ).map_err(|e| e.to_string())?;

        let mut daily_stmt = conn.prepare(
            "INSERT INTO daily_aggregates (
                date, source_id, model, total_tokens, input_tokens, output_tokens,
                cache_read_tokens, cost_micros, session_count
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1)
            ON CONFLICT(date, source_id, model) DO UPDATE SET
                total_tokens = daily_aggregates.total_tokens + excluded.total_tokens,
                input_tokens = daily_aggregates.input_tokens + excluded.input_tokens,
                output_tokens = daily_aggregates.output_tokens + excluded.output_tokens,
                cache_read_tokens = daily_aggregates.cache_read_tokens + excluded.cache_read_tokens,
                cost_micros = daily_aggregates.cost_micros + excluded.cost_micros,
                session_count = daily_aggregates.session_count + 1"
        ).map_err(|e| e.to_string())?;

        let mut check_stmt = conn.prepare(
            "SELECT 1 FROM usage_records WHERE record_hash = ?1"
        ).map_err(|e| e.to_string())?;

        for (session_id, turn_id, model, input, output, cache_read, cache_write, reasoning, cost_micros, recorded_at, title, project) in records {
            let total_tokens = input + output + reasoning;
            let record_hash = compute_record_hash(source_id, &session_id, turn_id.as_deref(), &recorded_at, total_tokens);

            let already_exists: bool = check_stmt
                .query_row([&record_hash], |_| Ok(true))
                .unwrap_or(false);

            if !already_exists {
                // 先插入/更新 session，满足 usage_records 对 sessions(session_id) 的外键约束
                sess_stmt.execute(rusqlite::params![
                    session_id, source_id, title, project, recorded_at, recorded_at,
                    input, output, cache_read, cache_write, reasoning, cost_micros
                ]).map_err(|e| e.to_string())?;

                // 插入 usage_records
                rec_stmt.execute(rusqlite::params![
                    record_hash, source_id, session_id, turn_id, model,
                    input, output, cache_read, cache_write, reasoning, cost_micros, recorded_at
                ]).map_err(|e| e.to_string())?;

                inserted_count += 1;

                // 提取 YYYY-MM-DD
                let date_str = if recorded_at.len() >= 10 { &recorded_at[..10] } else { "1970-01-01" };

                // 更新 daily_aggregates
                daily_stmt.execute(rusqlite::params![
                    date_str, source_id, model, total_tokens, input, output, cache_read, cost_micros
                ]).map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    })?;

    Ok(inserted_count)
}

fn compute_record_hash(
    source_id: &str,
    session_id: &str,
    turn_id: Option<&str>,
    recorded_at: &str,
    total_tokens: i64,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(source_id.as_bytes());
    hasher.update(session_id.as_bytes());
    if let Some(t) = turn_id {
        hasher.update(t.as_bytes());
    }
    hasher.update(recorded_at.as_bytes());
    hasher.update(total_tokens.to_string().as_bytes());
    format!("{:x}", hasher.finalize())
}

fn chrono_now() -> String {
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = dur / 86400;
    let secs_of_day = dur % 86400;
    let (y, m, d) = days_to_ymd(days);
    let h = secs_of_day / 3600;
    let min = (secs_of_day % 3600) / 60;
    let s = secs_of_day % 60;
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, m, d, h, min, s)
}

fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let yr = if m <= 2 { y + 1 } else { y };
    (yr, m, d)
}
