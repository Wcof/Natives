//! Context Assembler — system prompt, AGENTS.md, profile, skills stubs.

use crate::profile::AgentProfile;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct AssembledContext {
    pub system_prompt: String,
    pub estimated_tokens: u64,
    pub sources: Vec<String>,
}

/// Assemble a system prompt from profile + project files.
pub fn assemble_context(
    profile: Option<&AgentProfile>,
    project_root: Option<&Path>,
    extra_system: Option<&str>,
) -> AssembledContext {
    let mut parts = Vec::new();
    let mut sources = Vec::new();

    if let Some(extra) = extra_system {
        if !extra.trim().is_empty() {
            parts.push(extra.trim().to_string());
            sources.push("extra_system".into());
        }
    }

    if let Some(profile) = profile {
        if let Some(prompt) = &profile.system_prompt {
            if !prompt.trim().is_empty() {
                parts.push(prompt.trim().to_string());
                sources.push(format!("profile:{}", profile.id));
            }
        }
    }

    if let Some(root) = project_root {
        for name in ["AGENTS.md", "agents.md", "CLAUDE.md"] {
            let path = root.join(name);
            if let Ok(text) = std::fs::read_to_string(&path) {
                if !text.trim().is_empty() {
                    parts.push(format!("# {name}\n{}", text.trim()));
                    sources.push(path.display().to_string());
                    break;
                }
            }
        }
        // Skills stubs: list .claude/skills or .grok/skills directory names
        for skills_dir in [".claude/skills", ".grok/skills", ".natives/skills"] {
            let dir = root.join(skills_dir);
            if let Ok(rd) = std::fs::read_dir(&dir) {
                let names: Vec<_> = rd
                    .filter_map(|e| e.ok())
                    .filter(|e| e.path().is_dir() || e.path().extension().map(|x| x == "md").unwrap_or(false))
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .take(20)
                    .collect();
                if !names.is_empty() {
                    parts.push(format!(
                        "# Available skills\n{}",
                        names.iter().map(|n| format!("- {n}")).collect::<Vec<_>>().join("\n")
                    ));
                    sources.push(dir.display().to_string());
                }
            }
        }
    }

    let system_prompt = parts.join("\n\n");
    // Rough token estimate: ~4 chars per token
    let estimated_tokens = (system_prompt.len() as u64 / 4).max(1);
    AssembledContext {
        system_prompt,
        estimated_tokens,
        sources,
    }
}

/// Soft context budget: estimate + thresholds used by engine / RPC.
#[derive(Debug, Clone, Copy)]
pub struct ContextBudget {
    /// Approximate token budget for full history (chars/4 heuristic).
    pub token_budget: u64,
    /// Character soft limit before tool-output compaction kicks in.
    pub history_compact_chars: usize,
    /// Max characters retained per tool output after compaction.
    pub tool_output_max_chars: usize,
}

impl Default for ContextBudget {
    fn default() -> Self {
        Self {
            token_budget: 128_000,
            history_compact_chars: 48_000,
            tool_output_max_chars: 4_000,
        }
    }
}

impl ContextBudget {
    pub fn from_token_budget(token_budget: u64) -> Self {
        let chars = (token_budget.saturating_mul(4)) as usize;
        Self {
            token_budget: token_budget.max(1_024),
            history_compact_chars: chars.clamp(8_000, 200_000),
            tool_output_max_chars: (chars / 24).clamp(512, 8_000),
        }
    }

    pub fn estimate_tokens(text_chars: usize) -> u64 {
        (text_chars as u64 / 4).max(1)
    }
}

/// Compact history when over budget: keep system + last N user/assistant pairs.
pub fn compact_messages(
    messages: &[(String, String)],
    token_budget: u64,
) -> (Vec<(String, String)>, Option<String>) {
    let estimate: u64 = messages
        .iter()
        .map(|(_, c)| (c.len() as u64 / 4).max(1))
        .sum();
    if estimate <= token_budget {
        return (messages.to_vec(), None);
    }
    // Keep last 4 messages when over budget (deterministic minimum retention).
    let keep = messages.len().saturating_sub(4);
    let dropped = &messages[..keep];
    let summary = format!(
        "Previous conversation summary ({} messages omitted for context budget; budget={token_budget} tokens).",
        dropped.len()
    );
    let mut kept = vec![("system".into(), summary.clone())];
    kept.extend(messages[keep..].iter().cloned());
    (kept, Some(summary))
}

pub fn discover_agents_md(start: &Path) -> Option<PathBuf> {
    let mut current = Some(start);
    while let Some(dir) = current {
        for name in ["AGENTS.md", "agents.md"] {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        current = dir.parent();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assembles_profile_and_agents_md() {
        let dir = std::env::temp_dir().join(format!("natives-ctx-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let agents = dir.join("AGENTS.md");
        std::fs::write(&agents, "Use rustfmt.").unwrap();
        let profile = AgentProfile {
            id: "coder".into(),
            name: "Coder".into(),
            system_prompt: Some("You are a coder.".into()),
            ..Default::default()
        };
        let ctx = assemble_context(Some(&profile), Some(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(ctx.system_prompt.contains("You are a coder."));
        assert!(ctx.system_prompt.contains("rustfmt"));
        assert!(ctx.sources.iter().any(|s| s.contains("AGENTS.md")));
    }

    #[test]
    fn compact_drops_old_messages() {
        let messages: Vec<_> = (0..10)
            .map(|i| ("user".into(), format!("message number {i} with padding xxxxx")))
            .collect();
        let (kept, summary) = compact_messages(&messages, 20);
        assert!(summary.is_some());
        assert!(kept.len() < messages.len());
    }
}
