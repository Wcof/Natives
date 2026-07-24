//! Context Assembler — system prompt, AGENTS.md, profile, skills stubs.

use crate::profile::AgentProfile;
use std::collections::HashSet;
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
        let instruction_dirs = project_instruction_dirs(root);
        let mut seen_paths = HashSet::new();
        if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
            let home = PathBuf::from(home);
            for config_root in [
                home.join(".natives"),
                home.join(".agents"),
                home.join(".claude"),
            ] {
                for name in ["AGENTS.md", "agents.md", "CLAUDE.md", "Claude.md"] {
                    append_instruction_file(
                        &config_root.join(name),
                        &mut seen_paths,
                        &mut parts,
                        &mut sources,
                    );
                }
                let mut rules = std::fs::read_dir(config_root.join("rules"))
                    .ok()
                    .into_iter()
                    .flatten()
                    .filter_map(Result::ok)
                    .map(|entry| entry.path())
                    .filter(|path| {
                        path.extension()
                            .and_then(|ext| ext.to_str())
                            .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
                    })
                    .collect::<Vec<_>>();
                rules.sort();
                for path in rules {
                    append_instruction_file(
                        &path,
                        &mut seen_paths,
                        &mut parts,
                        &mut sources,
                    );
                }
            }
        }
        for dir in &instruction_dirs {
            for name in ["AGENTS.md", "agents.md", "CLAUDE.md", "Claude.md"] {
                append_instruction_file(
                    &dir.join(name),
                    &mut seen_paths,
                    &mut parts,
                    &mut sources,
                );
            }
            for rules_dir in [".agents/rules", ".claude/rules", ".natives/rules"] {
                let mut rules = std::fs::read_dir(dir.join(rules_dir))
                    .ok()
                    .into_iter()
                    .flatten()
                    .filter_map(Result::ok)
                    .map(|entry| entry.path())
                    .filter(|path| {
                        path.extension()
                            .and_then(|ext| ext.to_str())
                            .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
                    })
                    .collect::<Vec<_>>();
                rules.sort();
                for path in rules {
                    append_instruction_file(
                        &path,
                        &mut seen_paths,
                        &mut parts,
                        &mut sources,
                    );
                }
            }
        }
        parts.push(
            "Project instructions above are ordered from repository root to the current project directory; deeper files take precedence. Before working in a deeper subdirectory, read any additional AGENTS.md, CLAUDE.md, .agents/rules/*.md, or .claude/rules/*.md found there."
                .to_string(),
        );

        // Advertise project skills from every applicable directory. Full trusted
        // skill bodies are injected by the daemon SkillStore.
        let mut seen_skills = HashSet::new();
        for dir in instruction_dirs.iter().rev() {
            for skills_dir in [
                ".natives/skills",
                ".grok/skills",
                ".agents/skills",
                ".claude/skills",
            ] {
                let dir = dir.join(skills_dir);
            if let Ok(rd) = std::fs::read_dir(&dir) {
                let names: Vec<_> = rd
                    .filter_map(|e| e.ok())
                    .filter(|e| e.path().is_dir() || e.path().extension().map(|x| x == "md").unwrap_or(false))
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .filter(|name| seen_skills.insert(name.clone()))
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

fn project_instruction_dirs(start: &Path) -> Vec<PathBuf> {
    let root = start
        .ancestors()
        .find(|dir| dir.join(".git").exists())
        .unwrap_or(start);
    let mut dirs = Vec::new();
    let mut current = Some(start);
    while let Some(dir) = current {
        dirs.push(dir.to_path_buf());
        if dir == root {
            break;
        }
        current = dir.parent();
    }
    dirs.reverse();
    dirs
}

fn append_instruction_file(
    path: &Path,
    seen_paths: &mut HashSet<PathBuf>,
    parts: &mut Vec<String>,
    sources: &mut Vec<String>,
) {
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if !path.is_file() || !seen_paths.insert(canonical) {
        return;
    }
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    if text.trim().is_empty() {
        return;
    }
    parts.push(format!("# From: {}\n{}", path.display(), text.trim()));
    sources.push(path.display().to_string());
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

    /// Resolve budget from Profile tokenBudget and model context_window.
    /// Uses `min(profile, model)` when both present; missing → 128K default.
    ///
    /// `chars/4` is an explicit fallback only when Provider usage is unavailable.
    pub fn resolve(profile_token_budget: Option<u64>, model_context_window: Option<u64>) -> Self {
        let resolved = match (profile_token_budget, model_context_window) {
            (Some(p), Some(m)) => p.min(m),
            (Some(p), None) => p,
            (None, Some(m)) => m,
            (None, None) => 128_000,
        };
        Self::from_token_budget(resolved.max(1_024))
    }

    /// chars/4 heuristic — use only when Provider does not report usage tokens.
    pub fn estimate_tokens(text_chars: usize) -> u64 {
        (text_chars as u64 / 4).max(1)
    }
}

/// Compact history when over budget.
///
/// Retention policy:
/// 1. Always keep the last complete user/assistant dialogue turn (at least 2 msgs).
/// 2. Prefer keeping tool-call / tool-result adjacent pairs when dropping.
/// 3. Prepend a system summary for omitted messages.
pub fn compact_messages(
    messages: &[(String, String)],
    token_budget: u64,
) -> (Vec<(String, String)>, Option<String>) {
    let estimate: u64 = messages
        .iter()
        .map(|(_, c)| ContextBudget::estimate_tokens(c.len()))
        .sum();
    if estimate <= token_budget {
        return (messages.to_vec(), None);
    }

    // Find a cut that keeps the last dialogue turn and tool pairings.
    let mut keep_from = messages.len().saturating_sub(4);
    // Extend cut leftward if we would split a tool_call/tool_result pair:
    // treat roles containing "tool" as pair-sensitive; never start mid-pair.
    while keep_from > 0 {
        let role = messages[keep_from].0.to_ascii_lowercase();
        if role.contains("tool") && !role.contains("result") && !role.contains("output") {
            // Starting at a tool_call is ok; if previous is tool_call and this is
            // tool_result we already skipped. Break when cut is clean.
            break;
        }
        let prev_role = messages[keep_from.saturating_sub(1)]
            .0
            .to_ascii_lowercase();
        if prev_role.contains("tool")
            && !prev_role.contains("result")
            && (role.contains("result") || role.contains("output") || role.contains("tool"))
        {
            // Would split call/result — include the call.
            keep_from = keep_from.saturating_sub(1);
            continue;
        }
        break;
    }
    // Always keep at least the last user/assistant turn (2 msgs) when available.
    if messages.len() >= 2 {
        keep_from = keep_from.min(messages.len().saturating_sub(2));
    }

    let dropped = &messages[..keep_from];
    if dropped.is_empty() {
        return (messages.to_vec(), None);
    }
    let summary = format!(
        "Previous conversation summary ({} messages omitted for context budget; budget={token_budget} tokens; estimate via chars/4 fallback when provider usage unavailable).",
        dropped.len()
    );
    let mut kept = vec![("system".into(), summary.clone())];
    kept.extend(messages[keep_from..].iter().cloned());
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
    fn assembles_layered_agents_claude_rules_and_agents_skills() {
        let root = std::env::temp_dir().join(format!("natives-ctx-{}", uuid::Uuid::new_v4()));
        let nested = root.join("packages").join("app");
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::create_dir_all(nested.join(".claude").join("rules")).unwrap();
        std::fs::create_dir_all(nested.join(".agents").join("rules")).unwrap();
        std::fs::create_dir_all(nested.join(".agents").join("skills").join("review")).unwrap();
        std::fs::write(root.join("AGENTS.md"), "repo instruction").unwrap();
        std::fs::write(nested.join("CLAUDE.md"), "nested instruction").unwrap();
        std::fs::write(
            nested.join(".claude").join("rules").join("rust.md"),
            "claude rule",
        )
        .unwrap();
        std::fs::write(
            nested.join(".agents").join("rules").join("review.md"),
            "agents rule",
        )
        .unwrap();
        std::fs::write(
            nested
                .join(".agents")
                .join("skills")
                .join("review")
                .join("SKILL.md"),
            "# Review",
        )
        .unwrap();

        let ctx = assemble_context(None, Some(&nested), None);
        let _ = std::fs::remove_dir_all(&root);
        assert!(ctx.system_prompt.contains("repo instruction"));
        assert!(ctx.system_prompt.contains("nested instruction"));
        assert!(ctx.system_prompt.contains("claude rule"));
        assert!(ctx.system_prompt.contains("agents rule"));
        assert!(ctx.system_prompt.contains("- review"));
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
