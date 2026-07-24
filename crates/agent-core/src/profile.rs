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

pub fn load_agent_profile(
    profile_id: &str,
    project_root: Option<&Path>,
) -> Option<AgentProfile> {
    if profile_id.is_empty()
        || profile_id.contains('/')
        || profile_id.contains('\\')
        || profile_id.contains("..")
    {
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
}
