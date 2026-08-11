//! Built-in prompt surfaces and agent-directive prompt compilation (extracted from `production.rs`, task-01 structure).
//!
//! Prompt-domain constants and helpers: the creative-draft surface (allowlist + working prompt),
//! the parent-authored child directive layering, and the sole ordering authority for Native prompt
//! layers (`compile_effective_prompt`).

use agent_core::assemble_context;

/// Profile id reported for a run whose only persona is the parent-authored
/// directive. Consumed by the subagent-persona tests; the production wiring
/// path is `production.rs` (which re-exports `compile_effective_prompt` and
/// `builtin_prompt_surface_default`). Kept `pub` as the documented contract.
#[cfg_attr(not(test), allow(dead_code))]
pub const TASK_DIRECTIVE_PROFILE_ID: &str = "task-directive";

/// Layer a parent-authored system prompt onto the child's agent profile.
///
/// Ordering is persona first, directive second: the on-disk profile establishes
/// the role, then the parent's task-specific instructions refine it. When no
/// profile was selected the directive becomes a synthetic profile so it still
/// flows through `assemble_context` (and shows up in its `sources`) instead of
/// being spliced in as an anonymous string.
///
/// Only `system_prompt` is touched. Tool surface, permission profile and token
/// budget are resolved before this point and are never derived from a directive.
/// Kept `pub` as the documented persona contract; directly exercised by the
/// subagent-persona tests.
#[cfg_attr(not(test), allow(dead_code))]
pub fn merge_agent_directive(
    profile: Option<agent_core::AgentProfile>,
    directive: Option<&str>,
) -> Option<agent_core::AgentProfile> {
    let directive = directive.map(str::trim).filter(|d| !d.is_empty());
    let Some(directive) = directive else {
        return profile;
    };
    match profile {
        Some(mut profile) => {
            let base = profile
                .system_prompt
                .as_deref()
                .map(str::trim)
                .filter(|p| !p.is_empty());
            profile.system_prompt = Some(match base {
                Some(base) => format!("{base}\n\n{directive}"),
                None => directive.to_string(),
            });
            Some(profile)
        }
        None => Some(agent_core::AgentProfile {
            id: TASK_DIRECTIVE_PROFILE_ID.to_string(),
            name: TASK_DIRECTIVE_PROFILE_ID.to_string(),
            system_prompt: Some(directive.to_string()),
            ..Default::default()
        }),
    }
}

/// Compile the exact system prompt that will be sent to the Provider.
///
/// This is the sole ordering authority for Native prompt layers. Callers may
/// project the returned summaries into a Run snapshot, but only
/// `effective_full_text` is passed to the engine and it is never persisted.
///
/// Natives-owned prompt blocks are consumed exactly once per placement
/// (NE-P0-03), anchored to the layer they describe:
///
/// ```text
/// BuiltinSurface (agent kind / replacement)
/// SkillCatalog
///   -> NativesPromptBlock placement BeforeProfile
/// CapabilityExpert (profile / expert prompt)
///   -> NativesPromptBlock placement AfterProfile
/// ChildDirective
/// InstructionFiles (project instructions)
///   -> NativesPromptBlock placement AfterProjectInstructions (the default)
/// TeamRoster
///   -> NativesPromptBlock placement Final
/// ```
///
/// Block order inside each group is stable: `add_prompt_blocks` sorts the
/// matching blocks by their `order` field. The four groups above are the only
/// places Natives prompt blocks may appear and each is emitted at most once.
#[allow(clippy::too_many_arguments)] // pre-existing: parameter list is fixed
pub(crate) fn compile_effective_prompt(
    agent_kind: Option<&str>,
    profile: Option<&agent_core::AgentProfile>,
    child_directive: Option<&str>,
    project_root: Option<&std::path::Path>,
    skill_prompt: Option<&str>,
    prompt_blocks: &[harness_core::blueprint::PromptBlock],
    builtin_prompt_replacements: &[harness_core::blueprint::BuiltinPromptReplacementSpecV4],
    team_roster: Option<&str>,
) -> harness_core::CompiledPromptPlan {
    let mut builder = harness_core::PromptPlanBuilder::new();

    if let Some(agent_kind) = agent_kind {
        if let Some(default) = builtin_surface_system_prompt(agent_kind) {
            let surface_id = match agent_kind {
                CREATIVE_DRAFT_AGENT_KIND | "creative_draft" => CREATIVE_DRAFT_PROMPT_SURFACE_ID,
                _ => agent_kind,
            };
            builder.add_builtin_surface_with_replacements(
                surface_id,
                default,
                builtin_prompt_replacements,
            );
        }
    }

    if let Some(skill_prompt) = skill_prompt.filter(|prompt| !prompt.trim().is_empty()) {
        builder.add_skill_catalog("selected_skill_catalog", skill_prompt);
    }

    // BeforeProfile: anchored immediately before the Capability Expert layer.
    builder.add_prompt_blocks(
        prompt_blocks,
        harness_core::blueprint::PromptBlockPlacement::BeforeProfile,
    );

    if let Some(profile) = profile {
        if let Some(prompt) = profile
            .system_prompt
            .as_deref()
            .filter(|prompt| !prompt.trim().is_empty())
        {
            builder.add_capability_expert(profile.id.clone(), prompt);
        }
    }

    // AfterProfile: anchored immediately after the Capability Expert layer.
    builder.add_prompt_blocks(
        prompt_blocks,
        harness_core::blueprint::PromptBlockPlacement::AfterProfile,
    );

    if let Some(child_directive) = child_directive.filter(|prompt| !prompt.trim().is_empty()) {
        builder.add_child_directive(child_directive);
    }

    if let Some(project_root) = project_root {
        let instructions = assemble_context(None, Some(project_root), None);
        if !instructions.system_prompt.trim().is_empty() {
            builder.add_instruction_file("project_instructions", instructions.system_prompt);
        }
    }

    // AfterProjectInstructions (the PromptBlockPlacement default): anchored
    // immediately after the project instruction files.
    builder.add_prompt_blocks(
        prompt_blocks,
        harness_core::blueprint::PromptBlockPlacement::AfterProjectInstructions,
    );

    if let Some(team_roster) = team_roster.filter(|prompt| !prompt.trim().is_empty()) {
        builder.add_team_roster(team_roster);
    }

    // Final: the very last layer, after the team roster.
    builder.add_prompt_blocks(
        prompt_blocks,
        harness_core::blueprint::PromptBlockPlacement::Final,
    );

    builder.build()
}

/// Agent kind of the creative session (ADR-0014 section 8). Carried on a run as
/// `agent_profile_id`, which is the only per-run surface selector that reaches
/// `start_run`.
pub(crate) const CREATIVE_DRAFT_AGENT_KIND: &str = "creative-draft";

/// Resolve a built-in surface name to its allowlist.
///
/// Returns `None` for anything that is not a built-in surface, so the caller
/// falls back to the run-scoped or agent-profile allowlist and behaviour for
/// every existing agent kind is unchanged.
///
/// The creative surface is defined by omission as much as by inclusion: no
/// `write_file`, `edit_file`, `apply_patch` or `run_terminal`. That is what keeps
/// ADR-0014 invariant #3 ("the model cannot reach the real module directory")
/// true without a second gate — publishing stays a host command.
pub(crate) fn builtin_surface_allowlist(agent_kind: &str) -> Option<Vec<String>> {
    match agent_kind {
        CREATIVE_DRAFT_AGENT_KIND | "creative_draft" => Some(
            capability_gateway::tools::CREATIVE_DRAFT_TOOL_NAMES
                .iter()
                .map(|name| (*name).to_string())
                .collect(),
        ),
        _ => None,
    }
}

/// Working instructions for the creative surface.
///
/// Tool schemas alone tell the model what it *can* call, not what the session is
/// for. Without this it treats "make me a pomodoro timer" as a chat request and
/// answers with prose instead of writing a revision — the tools are registered
/// but never used. The creative surface has no agent profile on disk, so this is
/// where its behaviour is defined.
const CREATIVE_DRAFT_SYSTEM_PROMPT: &str = r#"You are building a small, self-contained web app for the user inside the Natives creative workshop.

The user's message begins with `[draft:<draftId>]`. That id identifies the draft you are editing — pass it to every draft tool. It is not part of the user's request; do not mention it back to them.

How to work:
- Write the whole app as a single HTML document with inline CSS and JS, then save it with `write_draft_module`. The user sees a live preview of whatever you save.
- For a change request, call `read_draft_module` first and edit what is already there. Do not regenerate from scratch and do not drop features the user did not ask you to remove.
- Save your work with `write_draft_module` before you finish. A reply without a saved revision leaves the user with nothing to look at.

Hard constraints (the save is rejected if you break them):
- No remote scripts or stylesheets. No CDN links. Everything inline.
- No `eval` or `new Function`.
- Persist data with `localStorage` if the app needs to remember anything.

If a save is rejected, the error text says exactly what failed — fix it and save again. Keep replies short: the app itself is the deliverable, not a description of it."#;

pub(crate) const CREATIVE_DRAFT_PROMPT_SURFACE_ID: &str = "builtin:surface:creative_draft";

pub(crate) fn builtin_prompt_surface_default(surface_id: &str) -> Option<&'static str> {
    match surface_id {
        CREATIVE_DRAFT_PROMPT_SURFACE_ID => Some(CREATIVE_DRAFT_SYSTEM_PROMPT),
        _ => None,
    }
}

/// Working instructions for a built-in surface, or `None` for agent kinds that
/// carry a profile on disk (whose prompt comes from that profile instead).
pub(crate) fn builtin_surface_system_prompt(agent_kind: &str) -> Option<&'static str> {
    match agent_kind {
        CREATIVE_DRAFT_AGENT_KIND | "creative_draft" => Some(CREATIVE_DRAFT_SYSTEM_PROMPT),
        _ => None,
    }
}

#[cfg(test)]
fn effective_builtin_surface_prompt(
    agent_kind: &str,
    replacements: &[harness_core::blueprint::BuiltinPromptReplacementSpecV4],
) -> Option<String> {
    let default = builtin_surface_system_prompt(agent_kind)?;
    let surface_id = match agent_kind {
        CREATIVE_DRAFT_AGENT_KIND | "creative_draft" => CREATIVE_DRAFT_PROMPT_SURFACE_ID,
        _ => return Some(default.to_string()),
    };
    Some(
        replacements
            .iter()
            .find(|replacement| replacement.surface_id == surface_id)
            .map(|replacement| replacement.markdown.clone())
            .unwrap_or_else(|| default.to_string()),
    )
}

#[cfg(test)]
mod builtin_prompt_replacement_tests {
    use super::*;

    #[test]
    fn harness_replacement_changes_the_native_surface_prompt() {
        let replacements = vec![harness_core::blueprint::BuiltinPromptReplacementSpecV4 {
            surface_id: CREATIVE_DRAFT_PROMPT_SURFACE_ID.into(),
            markdown: "replacement prompt".into(),
            base_default_digest: harness_core::sha256_hex(CREATIVE_DRAFT_SYSTEM_PROMPT),
        }];
        assert_eq!(
            effective_builtin_surface_prompt("creative_draft", &replacements).as_deref(),
            Some("replacement prompt")
        );
        assert_eq!(
            effective_builtin_surface_prompt("creative_draft", &[]).as_deref(),
            Some(CREATIVE_DRAFT_SYSTEM_PROMPT)
        );
    }

    #[test]
    fn effective_prompt_compiles_once_in_the_required_layer_order() {
        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir(project.path().join(".git")).unwrap();
        std::fs::write(project.path().join("AGENTS.md"), "project instruction").unwrap();
        let profile = agent_core::AgentProfile {
            id: "expert".into(),
            name: "Expert".into(),
            system_prompt: Some("expert prompt".into()),
            ..Default::default()
        };
        let blocks = vec![harness_core::blueprint::PromptBlockSpecV3 {
            id: "harness".into(),
            name: "Harness".into(),
            markdown: "harness prompt".into(),
            enabled: true,
            order: 0,
            placement: harness_core::blueprint::PromptBlockPlacement::Final,
        }];

        let compiled = compile_effective_prompt(
            Some("creative_draft"),
            Some(&profile),
            Some("child directive"),
            Some(project.path()),
            Some("skill catalog"),
            &blocks,
            &[],
            Some("team roster"),
        );
        let kinds = compiled
            .layers
            .iter()
            .map(|layer| layer.kind)
            .collect::<Vec<_>>();
        assert_eq!(
            kinds,
            vec![
                harness_core::PromptLayerKind::BuiltinSurface,
                harness_core::PromptLayerKind::SkillCatalog,
                harness_core::PromptLayerKind::CapabilityExpert,
                harness_core::PromptLayerKind::ChildDirective,
                harness_core::PromptLayerKind::InstructionFiles,
                harness_core::PromptLayerKind::TeamRoster,
                harness_core::PromptLayerKind::NativesPromptBlock,
            ]
        );
        assert_eq!(
            harness_core::sha256_hex(&compiled.effective_full_text),
            compiled.effective_prompt_hash
        );
    }

    /// NE-P0-03: each of the four `PromptBlockPlacement` groups is consumed
    /// exactly once and lands in the correct relative order inside the
    /// Provider capture. Table-driven: every row carries one block per
    /// placement with a distinctive marker, then the test asserts the full
    /// layer-kind sequence, the raw-text order, and that every marker appears
    /// exactly once.
    #[test]
    fn prompt_placements_are_consumed_exactly_once_in_design_order() {
        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir(project.path().join(".git")).unwrap();
        std::fs::write(project.path().join("AGENTS.md"), "project instruction").unwrap();
        let profile = agent_core::AgentProfile {
            id: "expert".into(),
            name: "Expert".into(),
            system_prompt: Some("expert prompt".into()),
            ..Default::default()
        };
        let placements = [
            harness_core::blueprint::PromptBlockPlacement::BeforeProfile,
            harness_core::blueprint::PromptBlockPlacement::AfterProfile,
            harness_core::blueprint::PromptBlockPlacement::AfterProjectInstructions,
            harness_core::blueprint::PromptBlockPlacement::Final,
        ];
        let markers = [
            "BLOCK_BEFORE_PROFILE",
            "BLOCK_AFTER_PROFILE",
            "BLOCK_AFTER_PROJECT",
            "BLOCK_FINAL",
        ];
        let blocks: Vec<harness_core::blueprint::PromptBlockSpecV3> = placements
            .iter()
            .copied()
            .zip(markers.iter().copied())
            .enumerate()
            .map(
                |(index, (placement, marker))| harness_core::blueprint::PromptBlockSpecV3 {
                    id: format!("block-{index}"),
                    name: format!("block-{index}"),
                    markdown: marker.to_string(),
                    enabled: true,
                    order: 0,
                    placement,
                },
            )
            .collect();

        let compiled = compile_effective_prompt(
            Some("creative_draft"),
            Some(&profile),
            Some("child directive"),
            Some(project.path()),
            Some("skill catalog"),
            &blocks,
            &[],
            Some("team roster"),
        );

        // Every placement appears exactly once in the layer-kind sequence.
        let kinds = compiled
            .layers
            .iter()
            .map(|layer| layer.kind)
            .collect::<Vec<_>>();
        assert_eq!(
            kinds
                .iter()
                .filter(|kind| **kind == harness_core::PromptLayerKind::NativesPromptBlock)
                .count(),
            4,
            "each of the four placements must be captured exactly once"
        );
        assert_eq!(
            kinds,
            vec![
                harness_core::PromptLayerKind::BuiltinSurface,
                harness_core::PromptLayerKind::SkillCatalog,
                harness_core::PromptLayerKind::NativesPromptBlock,
                harness_core::PromptLayerKind::CapabilityExpert,
                harness_core::PromptLayerKind::NativesPromptBlock,
                harness_core::PromptLayerKind::ChildDirective,
                harness_core::PromptLayerKind::InstructionFiles,
                harness_core::PromptLayerKind::NativesPromptBlock,
                harness_core::PromptLayerKind::TeamRoster,
                harness_core::PromptLayerKind::NativesPromptBlock,
            ]
        );

        // Raw-text order proves the semantic anchor of each placement.
        let text = &compiled.effective_full_text;
        let expected = [
            "skill catalog",
            "BLOCK_BEFORE_PROFILE",
            "expert prompt",
            "BLOCK_AFTER_PROFILE",
            "child directive",
            "project instruction",
            "BLOCK_AFTER_PROJECT",
            "team roster",
            "BLOCK_FINAL",
        ];
        let mut cursor = 0usize;
        for needle in expected {
            let at = text[cursor..]
                .find(needle)
                .unwrap_or_else(|| panic!("missing layer text {needle:?} in Provider capture"));
            cursor += at + needle.len();
        }

        for marker in markers {
            assert_eq!(
                text.matches(marker).count(),
                1,
                "{marker} must appear exactly once in the Provider capture"
            );
        }
        assert_eq!(
            harness_core::sha256_hex(text),
            compiled.effective_prompt_hash,
            "Snapshot/Provider hash must equal SHA-256 of the captured raw text"
        );
    }
}
