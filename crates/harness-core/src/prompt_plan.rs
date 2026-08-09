//! Pure Prompt Plan domain model and deterministic builder.
//!
//! Assembles system prompts across the 6 authoritative layers, computes layer
//! digests and redacted previews, and generates an effective SHA-256 hash.

use crate::blueprint::{sha256_hex, PromptBlockPlacement, PromptBlockSpecV3};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptLayerKind {
    BuiltinSurface,
    CapabilityExpert,
    InstructionFiles,
    SkillCatalog,
    NativesPromptBlock,
    TeamRoster,
    ChildDirective,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptLayerSummary {
    pub layer_id: String,
    pub kind: PromptLayerKind,
    pub source_owner: String,
    pub digest: String,
    pub char_estimate: usize,
    pub redacted_preview: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CompiledPromptPlan {
    pub layers: Vec<PromptLayerSummary>,
    pub effective_prompt_hash: String,
    #[serde(skip)]
    pub effective_full_text: String,
}

#[derive(Default)]
pub struct PromptPlanBuilder {
    layers: Vec<(String, PromptLayerKind, String, String)>,
}

impl PromptPlanBuilder {
    pub fn new() -> Self {
        Self { layers: Vec::new() }
    }

    pub fn add_builtin_surface(
        &mut self,
        id: impl Into<String>,
        text: impl Into<String>,
    ) -> &mut Self {
        self.add_builtin_surface_with_replacements(id, text, &[])
    }

    pub fn add_builtin_surface_with_replacements(
        &mut self,
        id: impl Into<String>,
        default_text: impl Into<String>,
        replacements: &[crate::blueprint::BuiltinPromptReplacementSpecV4],
    ) -> &mut Self {
        let surface_id = id.into();
        let default_str = default_text.into();

        let (effective_text, owner) =
            if let Some(rep) = replacements.iter().find(|r| r.surface_id == surface_id) {
                (
                    rep.markdown.clone(),
                    format!("Harness Blueprint (Replaced {})", surface_id),
                )
            } else {
                (default_str, "Native Engine".into())
            };

        if !effective_text.trim().is_empty() {
            self.layers.push((
                surface_id,
                PromptLayerKind::BuiltinSurface,
                owner,
                effective_text,
            ));
        }
        self
    }

    pub fn add_capability_expert(
        &mut self,
        expert_id: impl Into<String>,
        text: impl Into<String>,
    ) -> &mut Self {
        let text_str = text.into();
        if !text_str.trim().is_empty() {
            self.layers.push((
                expert_id.into(),
                PromptLayerKind::CapabilityExpert,
                "Capability Hub".into(),
                text_str,
            ));
        }
        self
    }

    pub fn add_instruction_file(
        &mut self,
        path: impl Into<String>,
        text: impl Into<String>,
    ) -> &mut Self {
        let text_str = text.into();
        let path_str = path.into();
        if !text_str.trim().is_empty() {
            self.layers.push((
                path_str.clone(),
                PromptLayerKind::InstructionFiles,
                path_str,
                text_str,
            ));
        }
        self
    }

    pub fn add_skill_catalog(
        &mut self,
        skill_id: impl Into<String>,
        text: impl Into<String>,
    ) -> &mut Self {
        let text_str = text.into();
        if !text_str.trim().is_empty() {
            self.layers.push((
                skill_id.into(),
                PromptLayerKind::SkillCatalog,
                "Skill Catalog".into(),
                text_str,
            ));
        }
        self
    }

    pub fn add_prompt_blocks(
        &mut self,
        blocks: &[PromptBlockSpecV3],
        target_placement: PromptBlockPlacement,
    ) -> &mut Self {
        let mut matching: Vec<&PromptBlockSpecV3> = blocks
            .iter()
            .filter(|b| b.enabled && b.placement == target_placement)
            .collect();
        matching.sort_by_key(|b| b.order);

        for block in matching {
            if !block.markdown.trim().is_empty() {
                self.layers.push((
                    block.id.clone(),
                    PromptLayerKind::NativesPromptBlock,
                    format!("Harness Blueprint: {}", block.name),
                    block.markdown.clone(),
                ));
            }
        }
        self
    }

    pub fn add_team_roster(&mut self, roster_text: impl Into<String>) -> &mut Self {
        let text_str = roster_text.into();
        if !text_str.trim().is_empty() {
            self.layers.push((
                "team_roster".into(),
                PromptLayerKind::TeamRoster,
                "Capability Resolution".into(),
                text_str,
            ));
        }
        self
    }

    pub fn add_child_directive(&mut self, directive_text: impl Into<String>) -> &mut Self {
        let text_str = directive_text.into();
        if !text_str.trim().is_empty() {
            self.layers.push((
                "child_directive".into(),
                PromptLayerKind::ChildDirective,
                "Parent Run".into(),
                text_str,
            ));
        }
        self
    }

    pub fn build(self) -> CompiledPromptPlan {
        let mut summaries = Vec::new();
        let mut full_text_parts = Vec::new();

        for (layer_id, kind, source_owner, text) in self.layers {
            let digest = sha256_hex(&text);
            let char_estimate = text.chars().count();
            let redacted_preview = create_redacted_preview(&text, 120);

            summaries.push(PromptLayerSummary {
                layer_id,
                kind,
                source_owner,
                digest,
                char_estimate,
                redacted_preview,
            });

            full_text_parts.push(text);
        }

        let effective_full_text = full_text_parts.join("\n\n");
        let effective_prompt_hash = sha256_hex(&effective_full_text);

        CompiledPromptPlan {
            layers: summaries,
            effective_prompt_hash,
            effective_full_text,
        }
    }
}

fn create_redacted_preview(text: &str, max_len: usize) -> String {
    let trimmed = text.trim();
    let sample = if trimmed.chars().count() > max_len {
        let prefix: String = trimmed.chars().take(max_len).collect();
        format!("{prefix}...")
    } else {
        trimmed.to_string()
    };
    // Redact secret-looking strings like token=xyz
    sample
        .lines()
        .map(|line| {
            if line.contains("secret") || line.contains("key") || line.contains("token") {
                "[redacted prompt instruction line]"
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_plan_builder_assembles_deterministically() {
        let mut builder = PromptPlanBuilder::new();
        builder.add_builtin_surface("base", "System instruction base.");
        builder.add_instruction_file("AGENTS.md", "Project instruction.");

        let plan = builder.build();
        assert_eq!(plan.layers.len(), 2);
        assert!(!plan.effective_prompt_hash.is_empty());
        assert!(plan
            .effective_full_text
            .contains("System instruction base."));
        assert!(plan.effective_full_text.contains("Project instruction."));
    }

    #[test]
    fn prompt_plan_builder_replaces_builtin_surface() {
        let replacements = vec![crate::blueprint::BuiltinPromptReplacementSpecV4 {
            surface_id: "base".into(),
            markdown: "Replaced system instruction base.".into(),
            base_default_digest: "sha256_placeholder".into(),
        }];

        let mut builder = PromptPlanBuilder::new();
        builder.add_builtin_surface_with_replacements("base", "Original base.", &replacements);

        let plan = builder.build();
        assert_eq!(plan.layers.len(), 1);
        assert!(plan
            .effective_full_text
            .contains("Replaced system instruction base."));
        assert!(!plan.effective_full_text.contains("Original base."));
        assert_eq!(
            plan.layers[0].source_owner,
            "Harness Blueprint (Replaced base)"
        );
    }

    /// NE-P0-03: every `PromptBlockPlacement` is consumed exactly once and the
    /// builder emits the blocks in the anchor order the production compiler
    /// uses (BeforeProfile -> AfterProfile -> AfterProjectInstructions -> Final).
    #[test]
    fn all_four_placements_are_consumed_in_anchor_order() {
        fn block(id: &str, markdown: &str, placement: PromptBlockPlacement) -> PromptBlockSpecV3 {
            PromptBlockSpecV3 {
                id: id.into(),
                name: id.into(),
                markdown: markdown.into(),
                enabled: true,
                order: 0,
                placement,
            }
        }
        let blocks = vec![
            block("a1", "A_BEFORE", PromptBlockPlacement::BeforeProfile),
            block("a2", "A_AFTER_PROFILE", PromptBlockPlacement::AfterProfile),
            block(
                "a3",
                "A_AFTER_PROJECT",
                PromptBlockPlacement::AfterProjectInstructions,
            ),
            block("a4", "A_FINAL", PromptBlockPlacement::Final),
        ];

        let mut builder = PromptPlanBuilder::new();
        builder
            .add_builtin_surface("base", "base")
            .add_skill_catalog("skills", "skills");
        builder.add_prompt_blocks(&blocks, PromptBlockPlacement::BeforeProfile);
        builder.add_capability_expert("expert", "expert");
        builder.add_prompt_blocks(&blocks, PromptBlockPlacement::AfterProfile);
        builder.add_child_directive("directive");
        builder.add_instruction_file("AGENTS.md", "project");
        builder.add_prompt_blocks(&blocks, PromptBlockPlacement::AfterProjectInstructions);
        builder.add_team_roster("roster");
        builder.add_prompt_blocks(&blocks, PromptBlockPlacement::Final);

        let plan = builder.build();
        let kinds = plan
            .layers
            .iter()
            .map(|layer| layer.kind)
            .collect::<Vec<_>>();
        assert_eq!(
            kinds,
            vec![
                PromptLayerKind::BuiltinSurface,
                PromptLayerKind::SkillCatalog,
                PromptLayerKind::NativesPromptBlock,
                PromptLayerKind::CapabilityExpert,
                PromptLayerKind::NativesPromptBlock,
                PromptLayerKind::ChildDirective,
                PromptLayerKind::InstructionFiles,
                PromptLayerKind::NativesPromptBlock,
                PromptLayerKind::TeamRoster,
                PromptLayerKind::NativesPromptBlock,
            ]
        );

        // Each placement contributed its block exactly once.
        for marker in ["A_BEFORE", "A_AFTER_PROFILE", "A_AFTER_PROJECT", "A_FINAL"] {
            assert_eq!(
                plan.effective_full_text.matches(marker).count(),
                1,
                "{marker} must appear exactly once"
            );
        }

        // Raw-text order proves the semantic anchor of every placement.
        let expected = [
            "A_BEFORE",
            "expert",
            "A_AFTER_PROFILE",
            "directive",
            "project",
            "A_AFTER_PROJECT",
            "roster",
            "A_FINAL",
        ];
        let mut cursor = 0usize;
        for needle in expected {
            let at = plan.effective_full_text[cursor..]
                .find(needle)
                .unwrap_or_else(|| panic!("missing {needle} in assembled prompt"));
            cursor += at + needle.len();
        }
    }
}
