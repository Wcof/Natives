use crate::Result;
use serde::Serialize;
use std::path::Path;

mod skills;

pub use skills::{
    invalidate_skills_cache, scan_skills, validate_skill_dir, SkillHealth, SkillInfo, SkillsData,
    SkillsOverview,
};

// ── 数据结构 ──

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInfo {
    pub path: String,
    pub name: String,
    pub has_git: bool,
    pub languages: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    /// 会话 UUID（jsonl 文件名去扩展名，可直接用于 `claude --resume <id>`）
    pub id: String,
    /// jsonl 转写文件的绝对路径
    pub path: String,
    /// 最后活动时间（epoch ms，文件 mtime）
    pub mtime_ms: i64,
    /// 转写文件大小（字节）
    pub size: u64,
    /// 会话摘要（jsonl 内 type=summary 行；可能缺失）
    pub title: Option<String>,
}

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

/// 最多返回的会话数（按 mtime 倒序取最近）
const MAX_SESSIONS: usize = 50;
/// 提取标题时最多读取的行数 / 字节数
const TITLE_SCAN_LINES: usize = 25;
const TITLE_SCAN_BYTES: u64 = 64 * 1024;

/// Claude Code 把项目路径映射为 `~/.claude/projects/` 下的目录名：
/// 非字母数字字符全部替换为 '-'（如 `/Users/a/b.c` → `-Users-a-b-c`）。
fn project_slug(project_path: &str) -> String {
    project_path
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// 从 jsonl 转写头部提取会话摘要（type=summary 行的 summary 字段）。
fn session_title(path: &Path) -> Option<String> {
    use std::io::{BufRead, BufReader, Read};
    let file = std::fs::File::open(path).ok()?;
    let reader = BufReader::new(file.take(TITLE_SCAN_BYTES));
    for line in reader.lines().take(TITLE_SCAN_LINES).flatten() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if value.get("type").and_then(|t| t.as_str()) == Some("summary") {
            if let Some(summary) = value.get("summary").and_then(|s| s.as_str()) {
                let trimmed = summary.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.chars().take(120).collect());
                }
            }
        }
    }
    None
}

/// 扫描 Claude Code 的真实会话存储：`~/.claude/projects/<slug>/*.jsonl`。
/// 旧实现读 `<project>/.claude/sessions/`——该目录从不存在，列表恒为空。
pub fn scan_sessions(project_path: &str) -> Result<Vec<SessionInfo>> {
    let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    let sessions_dir = home
        .join(".claude")
        .join("projects")
        .join(project_slug(project_path));
    if !sessions_dir.is_dir() {
        return Ok(Vec::new());
    }

    let entries = match std::fs::read_dir(&sessions_dir) {
        Ok(e) => e,
        Err(_) => return Ok(Vec::new()),
    };

    let mut sessions = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let id = path
            .file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        if id.is_empty() {
            continue;
        }
        let meta = std::fs::metadata(&path).ok();
        let mtime_ms = meta
            .as_ref()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let size = meta.map(|m| m.len()).unwrap_or(0);
        sessions.push(SessionInfo {
            id,
            path: path.to_string_lossy().to_string(),
            mtime_ms,
            size,
            // 标题延后到排序截断之后再读，避免为不返回的会话做 IO
            title: None,
        });
    }

    sessions.sort_by_key(|s| std::cmp::Reverse(s.mtime_ms));
    sessions.truncate(MAX_SESSIONS);
    for session in &mut sessions {
        session.title = session_title(Path::new(&session.path));
    }
    Ok(sessions)
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

#[cfg(test)]
mod tests {
    use super::project_slug;

    #[test]
    fn project_slug_matches_claude_code_layout() {
        assert_eq!(
            project_slug("/Users/ldh/Downloads/project/AiNative/Natives"),
            "-Users-ldh-Downloads-project-AiNative-Natives"
        );
        assert_eq!(project_slug("/home/a/my.app_v2"), "-home-a-my-app-v2");
    }
}
