//! Agent Profile discovery (Markdown + YAML frontmatter).

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Declarative agent profile.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AgentProfile {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub prompt_mode: Option<String>,
    pub system_prompt: Option<String>,
    pub tools: Option<Vec<String>>,
    pub disallowed_tools: Option<Vec<String>>,
    pub permission_mode: Option<String>,
    pub skills: Option<Vec<String>>,
    pub provider_id: Option<String>,
    pub key_id: Option<String>,
    pub model_id: Option<String>,
    pub base_url_override: Option<String>,
    pub context_mode: Option<String>,
    pub isolation_mode: Option<String>,
    pub max_steps: Option<u32>,
    pub max_duration: Option<u64>,
    pub token_budget: Option<u64>,
    pub completion_requirement: Option<String>,
    /// Body markdown after frontmatter.
    #[serde(skip)]
    pub body: String,
    #[serde(skip)]
    pub source_path: Option<PathBuf>,
}

/// Parse a Markdown agent profile with optional YAML frontmatter.
pub fn parse_agent_profile_markdown(raw: &str, path: Option<&Path>) -> Result<AgentProfile, String> {
    let trimmed = raw.trim_start_matches('\u{feff}');
    if !trimmed.starts_with("---") {
        let id = path
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "unnamed".into());
        return Ok(AgentProfile {
            id: id.clone(),
            name: id,
            system_prompt: Some(trimmed.to_string()),
            body: trimmed.to_string(),
            source_path: path.map(|p| p.to_path_buf()),
            ..Default::default()
        });
    }
    let rest = &trimmed[3..];
    let end = rest
        .find("\n---")
        .ok_or_else(|| "unclosed frontmatter".to_string())?;
    let yaml = rest[..end].trim();
    let body = rest[end + 4..].trim().to_string();

    // Minimal YAML subset: key: value lines (no nested structures yet).
    let mut profile = AgentProfile {
        body: body.clone(),
        system_prompt: if body.is_empty() { None } else { Some(body) },
        source_path: path.map(|p| p.to_path_buf()),
        ..Default::default()
    };
    for line in yaml.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim().trim_matches('"').trim_matches('\'').to_string();
        match key {
            "id" => profile.id = value,
            "name" => profile.name = value,
            "description" => profile.description = Some(value),
            "promptMode" | "prompt_mode" => profile.prompt_mode = Some(value),
            "permissionMode" | "permission_mode" => profile.permission_mode = Some(value),
            "providerId" | "provider_id" => profile.provider_id = Some(value),
            "keyId" | "key_id" => profile.key_id = Some(value),
            "modelId" | "model_id" => profile.model_id = Some(value),
            "baseUrlOverride" | "base_url_override" => profile.base_url_override = Some(value),
            "contextMode" | "context_mode" => profile.context_mode = Some(value),
            "isolationMode" | "isolation_mode" => profile.isolation_mode = Some(value),
            "maxSteps" | "max_steps" => profile.max_steps = value.parse().ok(),
            "maxDuration" | "max_duration" => profile.max_duration = value.parse().ok(),
            "tokenBudget" | "token_budget" => profile.token_budget = value.parse().ok(),
            "completionRequirement" | "completion_requirement" => {
                profile.completion_requirement = Some(value)
            }
            "tools" => profile.tools = Some(parse_inline_list(&value)),
            "skills" => profile.skills = Some(parse_inline_list(&value)),
            "disallowedTools" | "disallowed_tools" => {
                profile.disallowed_tools = Some(parse_inline_list(&value));
            }
            _ => {}
        }
    }
    if profile.id.is_empty() {
        profile.id = path
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "unnamed".into());
    }
    if profile.name.is_empty() {
        profile.name = profile.id.clone();
    }
    Ok(profile)
}

fn parse_inline_list(value: &str) -> Vec<String> {
    value
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .map(|item| {
            item.trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string()
        })
        .filter(|item| !item.is_empty())
        .collect()
}

/// Discover profiles from standard directories (highest priority first).
pub fn default_profile_search_roots(project_root: Option<&Path>) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(root) = project_root {
        roots.push(root.join(".agents/agents"));
        roots.push(root.join(".grok/agents"));
        roots.push(root.join(".claude/agents"));
        roots.push(root.join(".natives/agents"));
    }
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        roots.push(home.join(".agents/agents"));
        roots.push(home.join(".natives/agents"));
        roots.push(home.join(".grok/agents"));
        roots.push(home.join(".claude/agents"));
    }
    roots
}

/// True when `profile_id` is a safe single-segment id (no path traversal).
///
/// Shared by [`load_agent_profile`] and [`list_agent_profiles`] so a file that
/// cannot be loaded by id is never advertised as loadable.
fn is_safe_profile_id(profile_id: &str) -> bool {
    !profile_id.is_empty()
        && !profile_id.contains('/')
        && !profile_id.contains('\\')
        && !profile_id.contains("..")
}

/// Enumerate every agent profile reachable from the standard search roots.
///
/// Ordering follows [`default_profile_search_roots`] (project before `$HOME`),
/// and within a root the file name order is stabilized by sorting. The first
/// profile seen for an id wins, so a project profile shadows a `$HOME` profile
/// with the same id — the same precedence [`load_agent_profile`] applies.
///
/// The returned `id` is always the file stem, because that is the key
/// [`load_agent_profile`] resolves by. A frontmatter `id:` that disagrees with
/// the file name is not addressable and is therefore not reported as the id.
/// This keeps the round trip exact: `load_agent_profile(p.id, root)` returns
/// the same file for every `p` in this list.
pub fn list_agent_profiles(project_root: Option<&Path>) -> Vec<AgentProfile> {
    let mut out: Vec<AgentProfile> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for root in default_profile_search_roots(project_root) {
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        let mut paths: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.is_file()
                    && path
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .is_some_and(|ext| {
                            ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown")
                        })
            })
            .collect();
        paths.sort();
        for path in paths {
            let Ok(raw) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(mut profile) = parse_agent_profile_markdown(&raw, Some(&path)) else {
                continue;
            };
            let stem = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            if !is_safe_profile_id(&stem) {
                continue;
            }
            if profile.name.trim().is_empty() {
                profile.name = stem.clone();
            }
            profile.id = stem;
            if seen.insert(profile.id.clone()) {
                out.push(profile);
            }
        }
    }
    out
}

pub fn load_agent_profile(
    profile_id: &str,
    project_root: Option<&Path>,
) -> Option<AgentProfile> {
    if !is_safe_profile_id(profile_id) {
        return None;
    }
    for root in default_profile_search_roots(project_root) {
        for extension in ["md", "markdown"] {
            let path = root.join(format!("{profile_id}.{extension}"));
            let Ok(raw) = std::fs::read_to_string(&path) else {
                continue;
            };
            if let Ok(profile) = parse_agent_profile_markdown(&raw, Some(&path)) {
                return Some(profile);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_frontmatter_profile() {
        let raw = r#"---
id: coder
name: Coder
providerId: openai
modelId: gpt-4o
tools: [read_file, write_file]
maxSteps: 20
---
You are a careful coding agent.
"#;
        let profile = parse_agent_profile_markdown(raw, None).unwrap();
        assert_eq!(profile.id, "coder");
        assert_eq!(profile.provider_id.as_deref(), Some("openai"));
        assert_eq!(profile.max_steps, Some(20));
        assert_eq!(profile.tools.as_ref().unwrap().len(), 2);
        assert!(profile
            .system_prompt
            .as_deref()
            .unwrap()
            .contains("careful coding"));
    }

    #[test]
    fn loads_agents_project_profile() {
        let root = std::env::temp_dir().join(format!("natives-profile-{}", uuid::Uuid::new_v4()));
        let agents = root.join(".agents").join("agents");
        std::fs::create_dir_all(&agents).unwrap();
        std::fs::write(
            agents.join("reviewer.md"),
            "---\nname: Reviewer\ntokenBudget: 4096\n---\nReview carefully.",
        )
        .unwrap();
        let profile = load_agent_profile("reviewer", Some(&root)).unwrap();
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(profile.name, "Reviewer");
        assert_eq!(profile.token_budget, Some(4096));
        assert!(profile.system_prompt.unwrap().contains("Review carefully"));
    }

    #[test]
    fn rejects_path_traversal_profile_id() {
        let root = std::env::temp_dir();
        assert!(load_agent_profile("../etc/passwd", Some(&root)).is_none());
        assert!(load_agent_profile("a/b", Some(&root)).is_none());
        assert!(load_agent_profile("a\\b", Some(&root)).is_none());
        assert!(load_agent_profile("", Some(&root)).is_none());
    }

    #[test]
    fn lists_profiles_with_project_precedence_and_loadable_ids() {
        let root = std::env::temp_dir().join(format!("natives-list-{}", uuid::Uuid::new_v4()));
        let high = root.join(".agents").join("agents");
        let low = root.join(".claude").join("agents");
        std::fs::create_dir_all(&high).unwrap();
        std::fs::create_dir_all(&low).unwrap();
        // Same id in two roots — highest-priority root wins.
        std::fs::write(
            high.join("reviewer.md"),
            "---\nname: Reviewer HIGH\ntools: [read_file]\nmaxSteps: 7\n---\nHigh prompt.",
        )
        .unwrap();
        std::fs::write(low.join("reviewer.md"), "---\nname: Reviewer LOW\n---\nLow prompt.").unwrap();
        std::fs::write(low.join("scribe.md"), "---\nname: Scribe\n---\nWrite docs.").unwrap();
        // Non-markdown files are ignored.
        std::fs::write(low.join("notes.txt"), "not a profile").unwrap();
        // Frontmatter id that disagrees with the file name is reported by file stem,
        // because the stem is what `load_agent_profile` resolves.
        std::fs::write(
            low.join("auditor.md"),
            "---\nid: totally-different\nname: Auditor\n---\nAudit.",
        )
        .unwrap();

        let listed = list_agent_profiles(Some(&root));
        let ids: Vec<&str> = listed.iter().map(|p| p.id.as_str()).collect();
        assert!(ids.contains(&"reviewer"), "{ids:?}");
        assert!(ids.contains(&"scribe"), "{ids:?}");
        assert!(ids.contains(&"auditor"), "{ids:?}");
        assert!(!ids.contains(&"totally-different"), "{ids:?}");
        assert!(!ids.contains(&"notes"), "{ids:?}");
        // One entry per id, project root wins.
        assert_eq!(ids.iter().filter(|id| **id == "reviewer").count(), 1);
        let reviewer = listed.iter().find(|p| p.id == "reviewer").unwrap();
        assert_eq!(reviewer.name, "Reviewer HIGH");
        assert_eq!(reviewer.max_steps, Some(7));

        // Round trip: every listed id must load back to the same file.
        for p in &listed {
            let loaded = load_agent_profile(&p.id, Some(&root))
                .unwrap_or_else(|| panic!("listed profile `{}` is not loadable", p.id));
            assert_eq!(loaded.source_path, p.source_path);
        }
        let _ = std::fs::remove_dir_all(&root);
    }
}
