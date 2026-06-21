use crate::Result;
use serde::Serialize;
use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

// ── 常量 ──

/// Claude Code 单条 description 的截断线（官方文档）
const SKILL_DESC_CUT: usize = 1536;
/// 描述总预算的社区实测估算值（窗口的 1%），仅作预警参考
const SKILL_BUDGET_CHARS: usize = 15_000;
/// 结果缓存有效期
const CACHE_TTL_SECS: u64 = 30;
/// 触发统计保留天数
const STATS_RETENTION_DAYS: i64 = 45;
/// 最多扫描的日志文件数
const MAX_LOG_FILES: usize = 500;

// ── 数据结构 ──

#[derive(Debug, Serialize)]
pub struct ProjectInfo {
    pub path: String,
    pub name: String,
    pub has_git: bool,
    pub languages: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct SessionInfo {
    pub id: String,
    pub path: String,
    pub created_at: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillHealth {
    pub ok: bool,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    pub desc_len: usize,
    pub source: String,
    pub label: String,
    pub path: String,
    pub dir: String,
    pub enabled: bool,
    pub residue: bool,
    pub health: SkillHealth,
    pub trigger_count: u64,
    pub last_triggered: Option<i64>,
    /// 跨来源副本：同名 skill 出现在多处时，列出各来源路径
    #[serde(skip_serializing_if = "Option::is_none")]
    pub copies: Option<Vec<String>>,
    pub mtime: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillsOverview {
    pub total: usize,
    pub unique: usize,
    pub active: usize,
    pub dust: usize,
    pub issues: usize,
    pub budget_chars: usize,
    pub budget_limit: usize,
    pub desc_cut: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillsData {
    pub ok: bool,
    pub at: i64,
    pub items: Vec<SkillInfo>,
    pub overview: SkillsOverview,
}

struct SkillLogStat {
    count: u64,
    last_triggered: Option<i64>,
}

// ── 全局缓存 ──

static SKILLS_CACHE: Mutex<Option<(Instant, SkillsData)>> = Mutex::new(None);

// ── 扫描：项目 ──

pub fn scan_projects() -> Result<Vec<ProjectInfo>> {
    let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    let search_dirs = [
        home.join("projects"),
        home.join("code"),
        home.join("dev"),
        home.join("src"),
        home.join("Downloads/project"),
    ];

    let mut projects = Vec::new();

    for search_dir in &search_dirs {
        if !search_dir.exists() || !search_dir.is_dir() {
            continue;
        }
        let entries = match std::fs::read_dir(search_dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir()
                || path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|s| s.starts_with('.'))
                    .unwrap_or(true)
            {
                continue;
            }
            let has_git = path.join(".git").exists();
            let has_claude = path.join(".claude").exists() || path.join("CLAUDE.md").exists();
            if has_git || has_claude {
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();
                let languages = detect_languages(&path);
                projects.push(ProjectInfo {
                    path: path.to_string_lossy().to_string(),
                    name,
                    has_git,
                    languages,
                });
            }
        }
    }

    Ok(projects)
}

// ── 扫描：会话 ──

pub fn scan_sessions(project_path: &str) -> Result<Vec<SessionInfo>> {
    let claude_dir = Path::new(project_path).join(".claude");
    let sessions_dir = claude_dir.join("sessions");
    if !sessions_dir.exists() {
        return Ok(Vec::new());
    }

    let mut sessions = Vec::new();
    let entries = match std::fs::read_dir(&sessions_dir) {
        Ok(e) => e,
        Err(_) => return Ok(Vec::new()),
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let id = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        let meta = std::fs::metadata(&path).ok();
        let created_at = meta
            .as_ref()
            .and_then(|m| m.created().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| {
                chrono::DateTime::from_timestamp(d.as_secs() as i64, 0)
                    .map(|dt| dt.to_rfc3339())
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        let size = dir_size(&path);
        sessions.push(SessionInfo {
            id,
            path: path.to_string_lossy().to_string(),
            created_at,
            size,
        });
    }

    sessions.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(sessions)
}

// ── Skills: Frontmatter 解析 ──

/// 解析 SKILL.md 的 YAML frontmatter，提取 description
fn skill_frontmatter(txt: &str) -> Option<String> {
    let clean = txt.trim_start_matches('\u{FEFF}');
    // Try LF first, then CRLF — note: CRLF is 5 bytes, LF is 4 bytes
    let fm_start = clean.find("---\n").or_else(|| clean.find("---\r\n"))?;
    let is_crlf = fm_start + 4 < clean.len()
        && clean.as_bytes()[fm_start + 3] == b'\r'
        && clean.as_bytes()[fm_start + 4] == b'\n';
    let after_first = &clean[fm_start + if is_crlf { 5 } else { 4 }..];
    let fm_end = after_first.find("\n---").or_else(|| after_first.find("\r\n---"))?;
    let fm = &after_first[..fm_end];
    // Normalize line endings: strip trailing \r so .lines() doesn't carry it into values
    let fm_normalized = fm.replace("\r\n", "\n");

    for line in fm_normalized.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("description:") {
            let val = trimmed["description:".len()..].trim();
            // 处理块标量前缀（|, >, |+, >- 等）
            let val = if val.starts_with('|') || val.starts_with('>') {
                // 块标量：取后续行的缩进内容（简化处理，取第一行非空内容）
                let prefix_len = val.chars().take_while(|c| *c == '|' || *c == '>' || *c == '+' || *c == '-').count();
                val[prefix_len..].trim()
            } else {
                val
            };
            // 去引号
            let val = val.trim_matches(|c| c == '\'' || c == '"');
            return Some(val.to_string());
        }
    }
    None
}

// ── Skills: 健康检查 ──

fn check_skill_health(content: &str) -> (bool, Vec<String>) {
    let mut issues = Vec::new();
    let clean = content.trim_start_matches('\u{FEFF}');

    if clean.trim().is_empty() {
        issues.push("missing-skill-md".to_string());
        return (false, issues);
    }

    let lines: Vec<&str> = clean.lines().collect();
    if lines.is_empty() || lines[0].trim() != "---" {
        issues.push("missing-frontmatter".to_string());
        return (false, issues);
    }

    let mut end_fm_idx = None;
    for i in 1..lines.len() {
        if lines[i].trim() == "---" {
            end_fm_idx = Some(i);
            break;
        }
    }

    if end_fm_idx.is_none() {
        issues.push("missing-frontmatter".to_string());
        return (false, issues);
    }

    // 检查 description 长度
    if let Some(desc) = skill_frontmatter(content) {
        if desc.len() > SKILL_DESC_CUT {
            issues.push(format!(
                "description {} 字符，超过 {} 截断线",
                desc.len(),
                SKILL_DESC_CUT
            ));
        }
    } else {
        issues.push("missing-frontmatter".to_string());
    }

    (issues.is_empty(), issues)
}

// ── Skills: 递归扫描单个 skill 根目录 ──

fn scan_skill_root(
    root: &Path,
    source: &str,
    label: &str,
    out: &mut Vec<SkillInfo>,
    disabled: bool,
) {
    let entries = match std::fs::read_dir(root) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // 跳过隐藏文件/目录、归档、备份
        if name_str.starts_with('.') || name_str == "_archive" || name_str == "_backups" {
            continue;
        }

        let fp = root.join(&name);

        // 递归进入 _disabled/ 子目录
        if name_str == "_disabled" {
            if !disabled {
                // 用 metadata（跟随符号链接）判断是否为目录
                if std::fs::metadata(&fp)
                    .map(|m| m.is_dir())
                    .unwrap_or(false)
                {
                    scan_skill_root(&fp, source, label, out, true);
                }
            }
            continue;
        }

        // 跟随符号链接判断是否为目录
        let is_dir = if entry.path().is_symlink() {
            std::fs::metadata(&fp)
                .map(|m| m.is_dir())
                .unwrap_or(false)
        } else {
            entry
                .file_type()
                .map(|ft| ft.is_dir())
                .unwrap_or(false)
        };

        if !is_dir {
            // 非目录非 .md 文件 → 残留
            if name_str.to_lowercase().ends_with(".md") {
                continue;
            }
            out.push(SkillInfo {
                name: name_str.to_string(),
                description: String::new(),
                desc_len: 0,
                source: source.to_string(),
                label: label.to_string(),
                path: fp.to_string_lossy().to_string(),
                dir: fp.to_string_lossy().to_string(),
                enabled: !disabled,
                residue: true,
                health: SkillHealth {
                    ok: false,
                    issues: vec!["残留文件——不是有效 skill，只占目录".to_string()],
                },
                trigger_count: 0,
                last_triggered: None,
                copies: None,
                mtime: 0,
            });
            continue;
        }

        // 是目录 → 检查 SKILL.md
        let skill_md = fp.join("SKILL.md");
        let (mtime, content) = match std::fs::metadata(&skill_md) {
            Ok(meta) => {
                let mtime = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0);
                // 只读前 32KB
                let bytes = std::fs::read(&skill_md).unwrap_or_default();
                let head = if bytes.len() > 32768 {
                    String::from_utf8_lossy(&bytes[..32768]).to_string()
                } else {
                    String::from_utf8_lossy(&bytes).to_string()
                };
                (mtime, head)
            }
            Err(_) => {
                // 缺 SKILL.md → 残留
                out.push(SkillInfo {
                    name: name_str.to_string(),
                    description: String::new(),
                    desc_len: 0,
                    source: source.to_string(),
                    label: label.to_string(),
                    path: fp.join("SKILL.md").to_string_lossy().to_string(),
                    dir: fp.to_string_lossy().to_string(),
                    enabled: !disabled,
                    residue: true,
                    health: SkillHealth {
                        ok: false,
                        issues: vec!["缺 SKILL.md——不是有效 skill".to_string()],
                    },
                    trigger_count: 0,
                    last_triggered: None,
                    copies: None,
                    mtime: 0,
                });
                continue;
            }
        };

        let (ok, issues) = check_skill_health(&content);
        let description = skill_frontmatter(&content).unwrap_or_default();
        let desc_len = description.len();

        out.push(SkillInfo {
            name: name_str.to_string(),
            description: if description.len() > 240 {
                description[..240].to_string()
            } else {
                description
            },
            desc_len,
            source: source.to_string(),
            label: label.to_string(),
            path: skill_md.to_string_lossy().to_string(),
            dir: fp.to_string_lossy().to_string(),
            enabled: !disabled,
            residue: false,
            health: SkillHealth { ok, issues },
            trigger_count: 0,
            last_triggered: None,
            copies: None,
            mtime,
        });
    }
}

// ── Skills: Claude Code 触发统计（增量解析）──

fn parse_log_file_for_skills_incremental(
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
                                if let Some(skill_name) =
                                    b.get("input").and_then(|i| i.get("skill")).and_then(|s| s.as_str())
                                {
                                    let entry = stats
                                        .entry(skill_name.to_string())
                                        .or_insert(SkillLogStat {
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
        } else {
            // <command-name> 手动调用
            if let Some(start) = line.find("<command-name>") {
                let rest = &line[start + 14..];
                if let Some(end) = rest.find("</command-name>") {
                    let raw = rest[..end].trim();
                    let skill_name = raw.trim_start_matches('/');
                    if !skill_name.is_empty() {
                        let entry = stats
                            .entry(skill_name.to_string())
                            .or_insert(SkillLogStat {
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

fn get_skill_stats() -> std::collections::HashMap<String, SkillLogStat> {
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

// ── Skills: Codex 触发统计 ──

fn parse_codex_skill_events(
    cutoff: i64,
) -> std::collections::HashMap<String, SkillLogStat> {
    let mut stats = std::collections::HashMap::new();
    let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    let codex_sessions = home.join(".codex").join("sessions");
    if !codex_sessions.exists() {
        return stats;
    }

    let mut files: Vec<(std::path::PathBuf, i64)> = Vec::new();
    collect_jsonl_files(&codex_sessions, 0, &mut files);
    files.sort_by(|a, b| b.1.cmp(&a.1)); // 按 mtime 降序
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
                        && skill
                            .chars()
                            .all(|c| c.is_alphanumeric() || c == '.' || c == '-' || c == '_' || c == ':');
                    if valid && seen.insert(skill.to_string()) {
                        let entry = stats
                            .entry(skill.to_string())
                            .or_insert(SkillLogStat {
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

// ── Skills: 插件 skills 扫描 ──

fn scan_plugin_skills(out: &mut Vec<SkillInfo>) {
    let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    let plugins_json = home.join(".claude").join("plugins").join("installed_plugins.json");
    if !plugins_json.exists() {
        return;
    }
    let content = match std::fs::read_to_string(&plugins_json) {
        Ok(c) => c,
        Err(_) => return,
    };
    let val: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return,
    };
    if let Some(plugins) = val.get("plugins").and_then(|p| p.as_object()) {
        for (key, arr) in plugins {
            if let Some(arr) = arr.as_array() {
                for p in arr {
                    if let Some(install_path) = p.get("installPath").and_then(|v| v.as_str()) {
                        let skills_dir = Path::new(install_path).join("skills");
                        let label = key.split('@').next().unwrap_or(key);
                        if skills_dir.exists() {
                            scan_skill_root(&skills_dir, "plugin", label, out, false);
                        }
                    }
                }
            }
        }
    }
}

// ── Skills: 主扫描函数 ──

pub fn scan_skills() -> Result<SkillsData> {
    // 检查缓存
    {
        let cache = SKILLS_CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((instant, data)) = cache.as_ref() {
            if instant.elapsed().as_secs() < CACHE_TTL_SECS {
                return Ok(SkillsData {
                    ok: true,
                    at: data.at,
                    items: data.items.clone(),
                    overview: SkillsOverview {
                        total: data.overview.total,
                        unique: data.overview.unique,
                        active: data.overview.active,
                        dust: data.overview.dust,
                        issues: data.overview.issues,
                        budget_chars: data.overview.budget_chars,
                        budget_limit: data.overview.budget_limit,
                        desc_cut: data.overview.desc_cut,
                    },
                });
            }
        }
    }

    let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    let now = chrono::Utc::now().timestamp_millis();
    let cutoff = now - STATS_RETENTION_DAYS * 86_400_000;

    let mut items: Vec<SkillInfo> = Vec::new();

    // 1. 全局 skills（3 来源）
    scan_skill_root(
        &home.join(".claude").join("skills"),
        "claude",
        "~/.claude",
        &mut items,
        false,
    );
    scan_skill_root(
        &home.join(".codex").join("skills"),
        "codex",
        "~/.codex",
        &mut items,
        false,
    );
    scan_skill_root(
        &home.join(".agents").join("skills"),
        "agents",
        "~/.agents",
        &mut items,
        false,
    );

    // 2. 插件 skills
    scan_plugin_skills(&mut items);

    // 3. 项目级 skills（从 agent_projects 获取）
    let projects = scan_projects().unwrap_or_default();
    for p in &projects {
        let project_skills = Path::new(&p.path).join(".claude").join("skills");
        if project_skills.exists() {
            scan_skill_root(&project_skills, "project", &p.name, &mut items, false);
        }
    }

    // 4. 触发统计合并（Claude + Codex）
    let claude_stats = get_skill_stats();
    let codex_stats = parse_codex_skill_events(cutoff);

    for item in &mut items {
        if item.residue {
            continue;
        }
        let claude = claude_stats.get(&item.name);
        let codex = codex_stats.get(&item.name);
        let mut count = 0u64;
        let mut last: Option<i64> = None;
        if let Some(s) = claude {
            count += s.count;
            if let Some(t) = s.last_triggered {
                last = Some(last.map_or(t, |l| l.max(t)));
            }
        }
        if let Some(s) = codex {
            count += s.count;
            if let Some(t) = s.last_triggered {
                last = Some(last.map_or(t, |l| l.max(t)));
            }
        }
        item.trigger_count = count;
        item.last_triggered = last;
    }

    // 5. 跨来源副本检测
    let mut copies_map: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for item in &items {
        if item.residue {
            continue;
        }
        copies_map
            .entry(item.name.clone())
            .or_default()
            .push(format!(
                "{}/skills{}",
                item.label,
                if !item.enabled { "/_disabled" } else { "" }
            ));
    }
    for item in &mut items {
        if let Some(copies) = copies_map.get(&item.name) {
            if copies.len() > 1 {
                item.copies = Some(copies.clone());
            }
        }
    }

    // 6. 概览统计
    let non_residue: Vec<_> = items.iter().filter(|i| !i.residue).collect();
    let enabled: Vec<_> = non_residue.iter().filter(|i| i.enabled).collect();
    let unique_names: std::collections::HashSet<_> = non_residue.iter().map(|i| &i.name).collect();

    let mut budget_chars = 0usize;
    for item in &items {
        if item.enabled
            && !item.residue
            && (item.source == "claude" || item.source == "plugin")
        {
            budget_chars += item.desc_len;
        }
    }

    let overview = SkillsOverview {
        total: non_residue.len(),
        unique: unique_names.len(),
        active: enabled.iter().filter(|i| i.trigger_count > 0).count(),
        dust: enabled.iter().filter(|i| i.trigger_count == 0).count(),
        issues: items.iter().filter(|i| !i.health.ok).count(),
        budget_chars,
        budget_limit: SKILL_BUDGET_CHARS,
        desc_cut: SKILL_DESC_CUT,
    };

    let data = SkillsData {
        ok: true,
        at: now,
        items,
        overview,
    };

    // 更新缓存
    {
        let mut cache = SKILLS_CACHE.lock().unwrap_or_else(|e| e.into_inner());
        *cache = Some((Instant::now(), data.clone()));
    }

    Ok(data)
}

// ── Skills: 路径校验 ──

/// 校验路径在最近扫描结果中（防止任意路径移动/删除）
pub fn validate_skill_dir(dir: &str) -> Result<bool> {
    let cache = SKILLS_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, data)) = cache.as_ref() {
        let target = std::path::Path::new(dir)
            .canonicalize()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| dir.to_string());
        return Ok(data.items.iter().any(|i| {
            let item_dir = std::path::Path::new(&i.dir)
                .canonicalize()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| i.dir.clone());
            item_dir == target
        }));
    }
    // 没有缓存时放行（降级行为）
    Ok(true)
}

/// 清除 skills 缓存（toggle/trash 后调用）
pub fn invalidate_skills_cache() {
    let mut cache = SKILLS_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    *cache = None;
}

// ── Agent 状态检测 ──

pub fn detect_status(output: &str, exit_code: Option<i32>) -> Result<serde_json::Value> {
    let status = if exit_code == Some(0) {
        "success"
    } else if output.contains("error") || output.contains("Error") || output.contains("ERROR") {
        "error"
    } else if output.contains("warning") || output.contains("Warning") {
        "warning"
    } else {
        "unknown"
    };

    Ok(serde_json::json!({
        "status": status,
        "exitCode": exit_code,
        "hasOutput": !output.is_empty(),
    }))
}

// ── 工具函数 ──

fn detect_languages(dir: &Path) -> Vec<String> {
    let mut languages = Vec::new();
    let indicators = [
        ("package.json", "JavaScript"),
        ("tsconfig.json", "TypeScript"),
        ("Cargo.toml", "Rust"),
        ("go.mod", "Go"),
        ("requirements.txt", "Python"),
        ("pyproject.toml", "Python"),
        ("Gemfile", "Ruby"),
        ("pom.xml", "Java"),
        ("build.gradle", "Java"),
        ("Cargo.lock", "Rust"),
        ("yarn.lock", "JavaScript"),
        ("pnpm-lock.yaml", "JavaScript"),
    ];
    for (file, lang) in &indicators {
        if dir.join(file).exists() && !languages.contains(&lang.to_string()) {
            languages.push(lang.to_string());
        }
    }
    languages
}

fn dir_size(dir: &Path) -> u64 {
    let mut size = 0u64;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                size += dir_size(&path);
            } else if let Ok(meta) = std::fs::metadata(&path) {
                size += meta.len();
            }
        }
    }
    size
}
