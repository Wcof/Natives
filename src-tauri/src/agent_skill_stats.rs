//! Skill trigger-statistics parsing (W3 split from agent.rs).
//!
//! Owns incremental Claude JSONL log parsing, Codex session event parsing,
//! and JSONL file collection. The scan/aggregate code in `agent.rs` composes
//! these; no scanning or caching state lives here.

use std::path::Path;

/// 触发统计保留天数
const STATS_RETENTION_DAYS: i64 = 45;
/// 最多扫描的日志文件数
const MAX_LOG_FILES: usize = 500;

#[derive(Debug, Clone)]
pub struct SkillLogStat {
    pub count: u64,
    pub last_triggered: Option<i64>,
}

/// Incrementally parse one Claude Code JSONL transcript for `Skill` tool_use
/// entries and `<command-name>` invocations. Deduplicates repeated lines within
/// one message id.
pub fn parse_log_file_for_skills_incremental(
    content: &str,
    stats: &mut std::collections::HashMap<String, SkillLogStat>,
    last_msg_id: &mut String,
) {
    for line in content.lines() {
        let is_tool = line.contains("\"name\":\"Skill\"") && line.contains("\"tool_use\"");
        let is_cmd = line.contains("<command-name>");
        if !is_tool && !is_cmd {
            continue;
        }

        let ts = line
            .find("\"timestamp\":\"")
            .and_then(|i| {
                let rest = &line[i + 13..];
                rest.find('"').map(|end| &rest[..end])
            })
            .and_then(|s| {
                chrono::DateTime::parse_from_rfc3339(s)
                    .ok()
                    .or_else(|| chrono::DateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.fZ").ok())
            })
            .map(|dt| dt.timestamp_millis())
            .unwrap_or(0);

        if is_tool {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(line) {
                // 去重同一条消息
                let msg_id = val
                    .get("message")
                    .and_then(|m| m.get("id"))
                    .and_then(|id| id.as_str())
                    .unwrap_or("");
                if !msg_id.is_empty() && msg_id == *last_msg_id {
                    continue;
                }
                if !msg_id.is_empty() {
                    *last_msg_id = msg_id.to_string();
                }

                if val.get("type").and_then(|t| t.as_str()) == Some("assistant") {
                    if let Some(content_arr) = val
                        .get("message")
                        .and_then(|m| m.get("content"))
                        .and_then(|c| c.as_array())
                    {
                        for b in content_arr {
                            if b.get("type").and_then(|t| t.as_str()) == Some("tool_use")
                                && b.get("name").and_then(|n| n.as_str()) == Some("Skill")
                            {
                                if let Some(skill_name) = b
                                    .get("input")
                                    .and_then(|i| i.get("skill"))
                                    .and_then(|s| s.as_str())
                                {
                                    let entry = stats.entry(skill_name.to_string()).or_insert(
                                        SkillLogStat {
                                            count: 0,
                                            last_triggered: None,
                                        },
                                    );
                                    entry.count += 1;
                                    if ts > 0 {
                                        entry.last_triggered =
                                            Some(entry.last_triggered.map_or(ts, |lt| lt.max(ts)));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        } else {
            // <command-name> 手动调用
            if let Some(start) = line.find("<command-name>") {
                let rest = &line[start + 14..];
                if let Some(end) = rest.find("</command-name>") {
                    let raw = rest[..end].trim();
                    let skill_name = raw.trim_start_matches('/');
                    if !skill_name.is_empty() {
                        let entry = stats.entry(skill_name.to_string()).or_insert(SkillLogStat {
                            count: 0,
                            last_triggered: None,
                        });
                        entry.count += 1;
                        if ts > 0 {
                            entry.last_triggered =
                                Some(entry.last_triggered.map_or(ts, |lt| lt.max(ts)));
                        }
                    }
                }
            }
        }
    }
}

/// Aggregate Claude Code skill trigger stats from recent `~/.claude/projects`
/// JSONL transcripts (bounded by `MAX_LOG_FILES`, filtered by retention).
pub fn get_skill_stats() -> std::collections::HashMap<String, SkillLogStat> {
    let mut stats = std::collections::HashMap::new();
    let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    let projects_dir = home.join(".claude").join("projects");
    if !projects_dir.exists() {
        return stats;
    }

    let now = chrono::Utc::now().timestamp_millis();
    let cutoff = now - STATS_RETENTION_DAYS * 86_400_000;
    let mut file_count = 0;

    for entry in walkdir::WalkDir::new(&projects_dir)
        .min_depth(1)
        .max_depth(3)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if file_count >= MAX_LOG_FILES {
            break;
        }

        let path = entry.path();
        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("jsonl") {
            if let Ok(meta) = entry.metadata() {
                if let Ok(modified) = meta.modified() {
                    if let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH) {
                        let mtime_ms = duration.as_millis() as i64;
                        if mtime_ms >= cutoff {
                            file_count += 1;
                            if let Ok(content) = std::fs::read_to_string(path) {
                                let mut last_msg_id = String::new();
                                parse_log_file_for_skills_incremental(
                                    &content,
                                    &mut stats,
                                    &mut last_msg_id,
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    stats
}

/// Aggregate Codex skill trigger stats from recent `~/.codex/sessions` events.
pub fn parse_codex_skill_events(cutoff: i64) -> std::collections::HashMap<String, SkillLogStat> {
    let mut stats = std::collections::HashMap::new();
    let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    let codex_sessions = home.join(".codex").join("sessions");
    if !codex_sessions.exists() {
        return stats;
    }

    let mut files: Vec<(std::path::PathBuf, i64)> = Vec::new();
    collect_jsonl_files(&codex_sessions, 0, &mut files);
    files.sort_by_key(|f| std::cmp::Reverse(f.1)); // 按 mtime 降序
    files.truncate(60); // 封顶控 IO

    // 匹配模式：<skill>\n<name>X</name>（Codex rollout 中 skill 激活标记）
    // 不用 regex，纯字符串匹配
    let tag_open = "<skill>\\n<name>";
    let tag_close = "</name>";

    for (fp, mtime) in files {
        if mtime < cutoff {
            continue;
        }
        if let Ok(content) = std::fs::read_to_string(&fp) {
            let mut seen = std::collections::HashSet::new();
            let mut search_start = 0;
            while let Some(open_pos) = content[search_start..].find(tag_open) {
                let abs_open = search_start + open_pos + tag_open.len();
                if let Some(close_pos) = content[abs_open..].find(tag_close) {
                    let skill = &content[abs_open..abs_open + close_pos];
                    // 校验 skill name 格式（字母数字点横杠下划线冒号）
                    let valid = !skill.is_empty()
                        && skill.chars().all(|c| {
                            c.is_alphanumeric() || c == '.' || c == '-' || c == '_' || c == ':'
                        });
                    if valid && seen.insert(skill.to_string()) {
                        let entry = stats.entry(skill.to_string()).or_insert(SkillLogStat {
                            count: 0,
                            last_triggered: None,
                        });
                        entry.count += 1;
                        entry.last_triggered =
                            Some(entry.last_triggered.map_or(mtime, |lt| lt.max(mtime)));
                    }
                    search_start = abs_open + close_pos + tag_close.len();
                } else {
                    break;
                }
            }
        }
    }

    stats
}

/// Recursively collect JSONL files (max depth 3) with their mtimes.
fn collect_jsonl_files(dir: &Path, depth: usize, out: &mut Vec<(std::path::PathBuf, i64)>) {
    if depth > 3 {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl_files(&path, depth + 1, out);
        } else if path.extension().and_then(|s| s.to_str()) == Some("jsonl") {
            if let Ok(meta) = std::fs::metadata(&path) {
                if let Ok(modified) = meta.modified() {
                    if let Ok(d) = modified.duration_since(std::time::UNIX_EPOCH) {
                        out.push((path, d.as_millis() as i64));
                    }
                }
            }
        }
    }
}
