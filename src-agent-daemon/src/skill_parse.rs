//! Parsing for `SKILL.md` files: minimal YAML frontmatter plus a Markdown body.
//!
//! Frontmatter semantics (quoting, inline lists, comments) have exactly one
//! definition — [`agent_core::parse_agent_profile_markdown`], the same reader
//! agent profiles use. This module only rewrites the Claude Code
//! `allowed-tools`/`allowedTools`/`allowed_tools` key alias onto the `tools`
//! spelling the shared reader understands, then delegates.

use agent_core::parse_agent_profile_markdown;
use std::path::Path;

use super::MAX_DESCRIPTION_CHARS;

/// One parsed `SKILL.md`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSkill {
    pub name: String,
    pub description: String,
    pub body: String,
    pub allowed_tools: Option<Vec<String>>,
}

/// Rewrite the Claude Code `allowed-tools` frontmatter key to the `tools` spelling
/// [`parse_agent_profile_markdown`] already understands.
///
/// A key alias is not a reason to fork the frontmatter reader: the minimal YAML
/// semantics (quoting, inline lists, comments) must have exactly one definition,
/// and it lives in `agent-core`. Only lines inside the frontmatter block are
/// touched, so body text is never rewritten.
fn normalize_frontmatter_aliases(raw: &str) -> std::borrow::Cow<'_, str> {
    let trimmed = raw.trim_start_matches('\u{feff}');
    if !trimmed.starts_with("---") {
        return std::borrow::Cow::Borrowed(raw);
    }
    let mut out = String::with_capacity(raw.len());
    let mut in_frontmatter = false;
    let mut rewrote = false;
    for (index, line) in trimmed.split_inclusive('\n').enumerate() {
        let bare = line.trim_end_matches(['\n', '\r']).trim();
        if index == 0 {
            in_frontmatter = true;
            out.push_str(line);
            continue;
        }
        if in_frontmatter && bare == "---" {
            in_frontmatter = false;
            out.push_str(line);
            continue;
        }
        if in_frontmatter {
            if let Some((key, value)) = bare.split_once(':') {
                if matches!(
                    key.trim(),
                    "allowed-tools" | "allowedTools" | "allowed_tools"
                ) {
                    out.push_str("tools:");
                    out.push_str(value);
                    out.push('\n');
                    rewrote = true;
                    continue;
                }
            }
        }
        out.push_str(line);
    }
    if rewrote {
        std::borrow::Cow::Owned(out)
    } else {
        std::borrow::Cow::Borrowed(raw)
    }
}

/// Collapse a description to one bounded single-line summary.
fn summarize(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    flat.chars().take(MAX_DESCRIPTION_CHARS).collect()
}

/// First non-empty, non-heading line of the body — the pre-frontmatter heuristic,
/// kept as the fallback so a skill without a `description:` still says something
/// true rather than nothing.
fn description_from_body(body: &str) -> String {
    summarize(
        body.lines()
            .find(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
            .unwrap_or(""),
    )
}

/// Parse a skill file. `naming_path` supplies the fallback name (the skill
/// *directory* for `<dir>/SKILL.md`, the file itself for a bare `foo.md`).
///
/// Degrades rather than fails: a file with no frontmatter, or with frontmatter
/// that is never closed, is treated as a body-only skill named after its path.
/// A malformed skill must stay visible and inert, not disappear.
pub fn parse_skill_markdown(raw: &str, naming_path: Option<&Path>) -> ParsedSkill {
    let fallback_name = naming_path
        .and_then(|path| path.file_stem())
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| "skill".to_string());
    let normalized = normalize_frontmatter_aliases(raw);
    match parse_agent_profile_markdown(&normalized, naming_path) {
        Ok(profile) => {
            let body = profile.body.trim().to_string();
            let name = {
                let name = profile.name.trim();
                if name.is_empty() {
                    fallback_name
                } else {
                    name.to_string()
                }
            };
            let description = profile
                .description
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(summarize)
                .unwrap_or_else(|| description_from_body(&body));
            ParsedSkill {
                name,
                description,
                body,
                allowed_tools: profile.tools,
            }
        }
        // Unclosed frontmatter: keep every byte as body so nothing is lost, and
        // fall back to path-derived identity.
        Err(_) => {
            let body = raw.trim_start_matches('\u{feff}').trim().to_string();
            let description = description_from_body(&body);
            ParsedSkill {
                name: fallback_name,
                description,
                body,
                allowed_tools: None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_frontmatter_name_description_and_allowed_tools() {
        let parsed = parse_skill_markdown(
            "---\nname: Diff Reviewer\ndescription: Review a diff before claiming completion.\nallowed-tools: read_file, grep\n---\nStep 1. Read the diff.\n",
            Some(Path::new("/tmp/skills/review")),
        );
        assert_eq!(parsed.name, "Diff Reviewer");
        assert_eq!(
            parsed.description,
            "Review a diff before claiming completion."
        );
        assert_eq!(
            parsed.allowed_tools.as_deref(),
            Some(["read_file".to_string(), "grep".to_string()].as_slice())
        );
        // Frontmatter never leaks into the body.
        assert_eq!(parsed.body, "Step 1. Read the diff.");
        assert!(!parsed.body.contains("allowed-tools"));
    }

    #[test]
    fn accepts_every_allowed_tools_spelling_and_bracket_lists() {
        for key in ["allowed-tools", "allowedTools", "allowed_tools", "tools"] {
            let parsed = parse_skill_markdown(
                &format!("---\nname: t\n{key}: [read_file, \"grep\"]\n---\nbody\n"),
                None,
            );
            assert_eq!(
                parsed.allowed_tools.as_deref(),
                Some(["read_file".to_string(), "grep".to_string()].as_slice()),
                "key {key}"
            );
        }
    }

    #[test]
    fn missing_frontmatter_degrades_to_body_and_path_name() {
        let parsed = parse_skill_markdown(
            "# Heading\n\nFirst real line.\nSecond line.\n",
            Some(Path::new("/tmp/skills/legacy")),
        );
        assert_eq!(parsed.name, "legacy");
        assert_eq!(parsed.description, "First real line.");
        assert!(parsed.allowed_tools.is_none());
        assert!(parsed.body.contains("Second line."));
    }

    #[test]
    fn malformed_frontmatter_degrades_without_losing_content() {
        // Opened but never closed: must stay visible and keep every byte.
        let parsed = parse_skill_markdown(
            "---\nname: broken\ndescription: never closed\nDo the thing.\n",
            Some(Path::new("/tmp/skills/broken")),
        );
        assert_eq!(parsed.name, "broken");
        assert!(parsed.body.contains("Do the thing."));
        assert!(parsed.body.contains("name: broken"));
        assert!(parsed.allowed_tools.is_none());
    }

    #[test]
    fn body_text_that_looks_like_an_alias_is_not_rewritten() {
        let parsed = parse_skill_markdown(
            "---\nname: t\n---\nallowed-tools: this is prose, not frontmatter\n",
            None,
        );
        assert!(parsed.body.contains("allowed-tools: this is prose"));
        assert!(parsed.allowed_tools.is_none());
    }
}
