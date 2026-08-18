//! The on-demand `skill` tool surface: loading one skill body and capping its
//! declared `allowed-tools` by the caller's own surface.
//!
//! A skill body is unfiltered text, so loading always re-checks the content
//! hash against the trust grant; a body swapped between listing and load is
//! never served under the old approval.

use agent_core::resolve_child_tool_allowlist;
use serde_json::Value;
use std::path::Path;

use super::skill_parse::parse_skill_markdown;
use super::skill_trust::content_hash;
use super::{SkillScope, SkillStore, SkillTrust, MAX_LOADED_BODY_CHARS};

/// Resolve the tool surface a skill may narrow the run to.
///
/// `None` means the skill declared nothing, so nothing changes. A declaration is
/// intersected with `parent_surface` through
/// [`agent_core::resolve_child_tool_allowlist`], the sole definition of allowlist
/// matching — a skill can only ever remove tools. `parent_surface = None` is an
/// unrestricted (root) run, where the declaration passes through unchanged.
pub fn resolve_skill_tool_surface(
    declared: Option<&[String]>,
    parent_surface: Option<&[String]>,
) -> Option<Vec<String>> {
    let declared = declared?;
    Some(resolve_child_tool_allowlist(
        parent_surface,
        Some(declared),
        None,
        None,
    ))
}

/// Build the skill advertisement for one run without leaking project-scoped
/// records accumulated by the process-global catalog.
///
/// Returns name + description lines only; see [`super::SkillStore::advertisement`].
pub fn prompt_for_project(project: &Path) -> String {
    let skills = SkillStore::new();
    skills.discover_for_project(Some(project));
    skills.advertisement()
}

/// Load one skill body on demand (the `skill` tool).
pub fn load_skill_for_project(project: &Path, name: &str) -> Result<Value, String> {
    load_skill_for_project_with_surface(project, name, None)
}

/// Load one skill body, capping its declared `allowed-tools` by the caller's
/// own surface.
///
/// `parent_surface` is the tool allowlist of the run making the call
/// (`None` = unrestricted root run). The returned `allowed_tools` is always a
/// subset of it, so a skill file can never widen the surface of the run that
/// loads it.
pub fn load_skill_for_project_with_surface(
    project: &Path,
    name: &str,
    parent_surface: Option<&[String]>,
) -> Result<Value, String> {
    load_skill_for_project_with_selection(project, name, parent_surface, None)
}

/// Load one skill only when it belongs to the run's persisted selection.
///
/// A missing or malformed selection is denied rather than falling back to the
/// process-wide trusted catalog. This keeps the on-demand tool aligned with the
/// snapshot that the run was approved to use.
pub fn load_selected_skill_for_project_with_surface(
    project: &Path,
    name: &str,
    snapshot: Option<&Value>,
    parent_surface: Option<&[String]>,
) -> Result<Value, String> {
    let snapshot =
        snapshot.ok_or_else(|| "skill selection missing from run snapshot".to_string())?;
    if snapshot.get("selectionActive").and_then(Value::as_bool) != Some(true) {
        return Err("skill selection missing from run snapshot".into());
    }
    let selected = snapshot
        .get("skillIds")
        .and_then(Value::as_array)
        .ok_or_else(|| "skill selection is invalid in run snapshot".to_string())?;
    let selected = selected
        .iter()
        .map(Value::as_str)
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| "skill selection is invalid in run snapshot".to_string())?;
    load_skill_for_project_with_selection(project, name, parent_surface, Some(&selected))
}

fn load_skill_for_project_with_selection(
    project: &Path,
    name: &str,
    parent_surface: Option<&[String]>,
    selected_ids: Option<&[&str]>,
) -> Result<Value, String> {
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err("invalid skill name".into());
    }
    let skills = SkillStore::new();
    skills.discover_for_project(Some(project));
    let mut matches = skills
        .list()
        .into_iter()
        .filter(|skill| {
            skill.name == name
                && skill.enabled
                && skill.trust == SkillTrust::Trusted
                && selected_ids.is_none_or(|ids| {
                    ids.iter()
                        .any(|id| *id == skill.id || (!id.contains(':') && *id == skill.name))
                })
        })
        .collect::<Vec<_>>();
    matches.sort_by_key(|skill| match skill.scope {
        SkillScope::Project => 0,
        SkillScope::User => 1,
    });
    let skill = matches.into_iter().next().ok_or_else(|| {
        if selected_ids.is_some() {
            return format!("skill `{name}` was not selected for this run");
        }
        // Distinguish "no such skill" from "present but not trusted" so the model
        // does not retry forever and the user learns an approval is pending.
        match skills
            .list()
            .into_iter()
            .find(|candidate| candidate.name == name)
        {
            Some(found) if found.trust != SkillTrust::Trusted => format!(
                "skill `{name}` is not trusted ({:?}); approve it before it can be loaded",
                found.trust_basis
            ),
            Some(_) => format!("skill `{name}` is disabled"),
            None => format!("skill not found: {name}"),
        }
    })?;
    let raw = std::fs::read_to_string(&skill.path).map_err(|error| error.to_string())?;
    // Trust is pinned to content: a body swapped between listing and load must
    // not be served under the old approval.
    if content_hash(&raw) != skill.content_hash {
        return Err(format!("skill `{name}` changed on disk; re-approve it"));
    }
    let parsed = parse_skill_markdown(&raw, Some(Path::new(&skill.path)));
    let truncated = parsed.body.chars().count() > MAX_LOADED_BODY_CHARS;
    let body: String = parsed.body.chars().take(MAX_LOADED_BODY_CHARS).collect();
    let effective_tools =
        resolve_skill_tool_surface(parsed.allowed_tools.as_deref(), parent_surface);
    let mut payload = serde_json::json!({
        "skill": skill.name,
        "loaded": true,
        "path": skill.path,
        "scope": skill.scope,
        "content": body,
        "truncated": truncated,
    });
    if let Some(tools) = effective_tools {
        let note = if tools.is_empty() {
            "This skill declares a tool restriction, but none of the tools it names are available to this run. Follow the skill without them.".to_string()
        } else {
            format!(
                "While following this skill, restrict yourself to these tools: {}.",
                tools.join(", ")
            )
        };
        payload["allowed_tools"] = serde_json::json!(tools);
        payload["declared_allowed_tools"] = serde_json::json!(parsed.allowed_tools);
        payload["tool_policy"] = serde_json::json!(note);
    }
    Ok(payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skill_store::test_support::Fixture;
    use serde_json::json;

    // ─── allowed-tools capping ───

    #[test]
    fn skill_tools_cannot_widen_the_run_surface() {
        let parent = vec!["read_file".to_string(), "grep".to_string()];
        let declared = vec![
            "read_file".to_string(),
            "run_terminal".to_string(),
            "write_file".to_string(),
        ];
        let resolved = resolve_skill_tool_surface(Some(&declared), Some(&parent)).unwrap();
        assert_eq!(resolved, vec!["read_file".to_string()]);
        assert!(!resolved.contains(&"run_terminal".to_string()));
    }

    #[test]
    fn skill_without_declaration_leaves_the_surface_alone() {
        let parent = vec!["read_file".to_string()];
        assert!(resolve_skill_tool_surface(None, Some(&parent)).is_none());
        assert!(resolve_skill_tool_surface(None, None).is_none());
    }

    #[test]
    fn root_run_surface_passes_declaration_through() {
        let declared = vec!["read_file".to_string(), "mcp__github__issue".to_string()];
        assert_eq!(
            resolve_skill_tool_surface(Some(&declared), None).unwrap(),
            declared
        );
        // Empty declaration means "no tools", not "all tools".
        assert!(resolve_skill_tool_surface(Some(&[]), None)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn mcp_surface_matching_matches_the_runtime_definition() {
        // `mcp_call` stands for the whole MCP surface — the same rule
        // `tool_list_allows` enforces at call time.
        let parent = vec!["mcp_call".to_string()];
        let declared = vec!["mcp__github__create_issue".to_string()];
        assert_eq!(
            resolve_skill_tool_surface(Some(&declared), Some(&parent)).unwrap(),
            declared
        );
        let narrow = vec!["read_file".to_string()];
        assert!(resolve_skill_tool_surface(Some(&declared), Some(&narrow))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn loaded_skill_reports_capped_tools() {
        let fx = Fixture::new();
        fx.write_skill(
            ".natives/skills",
            "scoped",
            "---\nname: scoped\ndescription: d\nallowed-tools: read_file, run_terminal\n---\nBody.\n",
        );
        let store = fx.store();
        let id = store.list()[0].id.clone();
        store.set_trust(&id, SkillTrust::Trusted).unwrap();

        let parent = vec!["read_file".to_string(), "grep".to_string()];
        let loaded =
            load_skill_for_project_with_surface(&fx.root, "scoped", Some(&parent)).unwrap();
        assert_eq!(loaded["allowed_tools"], json!(["read_file"]));
        assert_eq!(
            loaded["declared_allowed_tools"],
            json!(["read_file", "run_terminal"])
        );
        assert!(loaded["tool_policy"]
            .as_str()
            .unwrap()
            .contains("read_file"));
        assert!(!loaded["tool_policy"]
            .as_str()
            .unwrap()
            .contains("run_terminal"));
    }

    #[test]
    fn selected_skill_load_is_scoped_and_capped_by_the_parent_surface() {
        let fx = Fixture::new();
        fx.write_skill(
            ".natives/skills",
            "selected-a",
            "---\nname: selected-a\ndescription: a\nallowed-tools: read_file, run_terminal\n---\nA.\n",
        );
        fx.write_skill(
            ".natives/skills",
            "selected-b",
            "---\nname: selected-b\ndescription: b\n---\nB.\n",
        );
        let store = fx.store();
        let skills = store.list();
        let selected_id = skills
            .iter()
            .find(|skill| skill.name == "selected-a")
            .expect("selected a")
            .id
            .clone();
        for skill in skills {
            store.set_trust(&skill.id, SkillTrust::Trusted).unwrap();
        }
        let snapshot = json!({"selectionActive": true, "skillIds": [selected_id]});
        let parent = vec!["read_file".to_string(), "grep".to_string()];

        let loaded = load_selected_skill_for_project_with_surface(
            &fx.root,
            "selected-a",
            Some(&snapshot),
            Some(&parent),
        )
        .unwrap();
        assert_eq!(loaded["allowed_tools"], json!(["read_file"]));
        assert!(load_selected_skill_for_project_with_surface(
            &fx.root,
            "selected-b",
            Some(&snapshot),
            Some(&parent),
        )
        .unwrap_err()
        .contains("not selected"));
    }

    #[test]
    fn selected_skill_load_fails_closed_without_a_valid_snapshot() {
        let fx = Fixture::new();
        assert!(
            load_selected_skill_for_project_with_surface(&fx.root, "anything", None, None,)
                .unwrap_err()
                .contains("selection missing")
        );
    }

    // ─── Load-path guards ───

    #[test]
    fn rejects_path_traversal_skill_names() {
        let fx = Fixture::new();
        for name in ["", "../etc/passwd", "a/b", "a\\b"] {
            assert!(load_skill_for_project(&fx.root, name).is_err(), "{name}");
        }
    }
}
