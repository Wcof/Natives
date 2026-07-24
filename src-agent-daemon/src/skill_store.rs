//! Skill discovery + trust + enable (Phase 6 minimum).
//!
//! Scans `.agents|claude|grok|natives/skills` at project and user scope.
//! Skills inject into system prompt only when
//! trusted and enabled; Subagent isolation is enforced by not auto-inheriting
//! parent skill selection (callers must pass explicit skill ids).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillScope {
    Project,
    User,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillRecord {
    pub id: String,
    pub name: String,
    pub description: String,
    pub scope: SkillScope,
    pub trusted: bool,
    pub enabled: bool,
    pub path: String,
    /// Prompt body (may be truncated in list views).
    pub body_preview: String,
}

#[derive(Default)]
pub struct SkillStore {
    items: Mutex<HashMap<String, SkillRecord>>,
}

impl SkillStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn discover_for_project(&self, project: Option<&Path>) {
        if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
            let home = PathBuf::from(home);
            for rel in [
                ".natives/skills",
                ".grok/skills",
                ".agents/skills",
                ".claude/skills",
            ] {
                self.scan_dir(&home.join(rel), SkillScope::User);
            }
        }
        if let Some(root) = project {
            for rel in [
                ".natives/skills",
                ".grok/skills",
                ".agents/skills",
                ".claude/skills",
            ] {
                self.scan_dir(&root.join(rel), SkillScope::Project);
            }
        }
    }

    fn scan_dir(&self, dir: &Path, scope: SkillScope) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            // SKILL.md or skill.md inside directory, or bare .md file
            let skill_md = if path.is_dir() {
                let a = path.join("SKILL.md");
                let b = path.join("skill.md");
                if a.exists() {
                    a
                } else if b.exists() {
                    b
                } else {
                    continue;
                }
            } else if path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("md"))
                .unwrap_or(false)
            {
                path.clone()
            } else {
                continue;
            };
            let Ok(body) = std::fs::read_to_string(&skill_md) else {
                continue;
            };
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("skill")
                .to_string();
            let id = format!(
                "{}:{}",
                match scope {
                    SkillScope::Project => "project",
                    SkillScope::User => "user",
                },
                name
            );
            let description = body
                .lines()
                .find(|l| !l.trim().is_empty() && !l.starts_with('#'))
                .unwrap_or("")
                .trim()
                .chars()
                .take(160)
                .collect::<String>();
            let preview: String = body.chars().take(400).collect();
            let rec = SkillRecord {
                id: id.clone(),
                name,
                description,
                scope: scope.clone(),
                // Project skills under project tree are trusted for injection; still
                // can be disabled. Untrusted external packs would set trusted=false later.
                trusted: true,
                enabled: true,
                path: skill_md.display().to_string(),
                body_preview: preview,
            };
            if let Ok(mut items) = self.items.lock() {
                items.insert(id, rec);
            }
        }
    }

    pub fn list(&self) -> Vec<SkillRecord> {
        self.items
            .lock()
            .map(|g| g.values().cloned().collect())
            .unwrap_or_default()
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<SkillRecord, String> {
        let mut items = self.items.lock().map_err(|e| e.to_string())?;
        let item = items
            .get_mut(id)
            .ok_or_else(|| format!("skill not found: {id}"))?;
        if enabled && !item.trusted {
            return Err("cannot enable untrusted skill".into());
        }
        item.enabled = enabled;
        Ok(item.clone())
    }

    /// Concatenate enabled trusted skill bodies for system prompt injection.
    pub fn inject_prompt(&self) -> String {
        let items = self.items.lock().ok();
        let Some(items) = items else {
            return String::new();
        };
        let mut skills = items
            .values()
            .filter(|skill| skill.enabled && skill.trusted)
            .collect::<Vec<_>>();
        skills.sort_by(|left, right| {
            let scope = |scope: &SkillScope| match scope {
                SkillScope::Project => 0,
                SkillScope::User => 1,
            };
            scope(&left.scope)
                .cmp(&scope(&right.scope))
                .then_with(|| left.name.cmp(&right.name))
        });
        let mut seen = std::collections::HashSet::new();
        let mut parts = Vec::new();
        for skill in skills {
            if seen.insert(skill.name.clone()) {
                if let Ok(body) = std::fs::read_to_string(&skill.path) {
                    parts.push(format!("### Skill: {}\n{}", skill.name, body));
                }
            }
        }
        parts.join("\n\n")
    }
}

/// Build the trusted skill prompt for one run without leaking project-scoped
/// records accumulated by the process-global catalog.
pub fn prompt_for_project(project: &Path) -> String {
    let skills = SkillStore::new();
    skills.discover_for_project(Some(project));
    skills.inject_prompt()
}

pub fn load_skill_for_project(project: &Path, name: &str) -> Result<Value, String> {
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err("invalid skill name".into());
    }
    let skills = SkillStore::new();
    skills.discover_for_project(Some(project));
    let mut matches = skills
        .list()
        .into_iter()
        .filter(|skill| skill.name == name && skill.enabled && skill.trusted)
        .collect::<Vec<_>>();
    matches.sort_by_key(|skill| match skill.scope {
        SkillScope::Project => 0,
        SkillScope::User => 1,
    });
    let skill = matches
        .into_iter()
        .next()
        .ok_or_else(|| format!("skill not found: {name}"))?;
    let body = std::fs::read_to_string(&skill.path).map_err(|error| error.to_string())?;
    Ok(serde_json::json!({
        "skill": skill.name,
        "loaded": true,
        "path": skill.path,
        "content": body,
    }))
}

static GLOBAL_SKILLS: std::sync::OnceLock<SkillStore> = std::sync::OnceLock::new();

pub fn global_skills() -> &'static SkillStore {
    GLOBAL_SKILLS.get_or_init(|| {
        let s = SkillStore::new();
        s.discover_for_project(std::env::current_dir().ok().as_deref());
        s
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_skill_md() {
        let dir = std::env::temp_dir().join(format!("natives-skill-{}", uuid::Uuid::new_v4()));
        let skill_dir = dir.join(".natives").join("skills").join("demo");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "# Demo\n\nDo the demo thing carefully.\n",
        )
        .unwrap();
        let s = SkillStore::new();
        s.discover_for_project(Some(&dir));
        let list = s.list();
        assert!(list.iter().any(|x| x.name == "demo"));
        assert!(!s.inject_prompt().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn discovers_agents_skill_md() {
        let dir = std::env::temp_dir().join(format!("natives-skill-{}", uuid::Uuid::new_v4()));
        let skill_dir = dir.join(".agents").join("skills").join("review");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "# Review\n\nCheck the actual diff before reporting completion.\n",
        )
        .unwrap();
        let s = SkillStore::new();
        s.discover_for_project(Some(&dir));
        assert!(s.list().iter().any(|skill| skill.name == "review"));
        assert!(s.inject_prompt().contains("actual diff"));
        let loaded = load_skill_for_project(&dir, "review").unwrap();
        assert_eq!(loaded["loaded"], true);
        assert!(loaded["content"].as_str().unwrap().contains("actual diff"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
