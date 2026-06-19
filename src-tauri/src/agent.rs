use crate::Result;
use serde::Serialize;
use std::path::Path;

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

#[derive(Debug, Serialize)]
pub struct SkillInfo {
    pub name: String,
    pub path: String,
    pub enabled: bool,
}

/// Scan for Claude Code projects in common locations
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
            if !path.is_dir() || path.file_name().and_then(|n| n.to_str()).map(|s| s.starts_with('.')).unwrap_or(true) {
                continue;
            }
            let has_git = path.join(".git").exists();
            let has_claude = path.join(".claude").exists() || path.join("CLAUDE.md").exists();
            if has_git || has_claude {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
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

/// Scan for Claude Code sessions
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
        let id = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
        let meta = std::fs::metadata(&path).ok();
        let created_at = meta
            .as_ref()
            .and_then(|m| m.created().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| chrono::DateTime::from_timestamp(d.as_secs() as i64, 0)
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_default())
            .unwrap_or_default();
        let size = dir_size(&path);
        sessions.push(SessionInfo {
            id,
            path: path.to_string_lossy().to_string(),
            created_at,
            size,
        });
    }

    // Sort by created_at descending
    sessions.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(sessions)
}

/// Scan for skills in .claude/skills directories
pub fn scan_skills() -> Result<Vec<SkillInfo>> {
    let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    let skills_dirs = [
        home.join(".claude/skills"),
        home.join(".claude/plugins"),
    ];

    let mut skills = Vec::new();

    for skills_dir in &skills_dirs {
        if !skills_dir.exists() {
            continue;
        }
        let entries = match std::fs::read_dir(skills_dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
            let enabled = !path.join(".disabled").exists();
            skills.push(SkillInfo {
                name,
                path: path.to_string_lossy().to_string(),
                enabled,
            });
        }
    }

    Ok(skills)
}

/// Detect agent status from output
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
