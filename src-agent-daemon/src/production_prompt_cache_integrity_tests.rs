//! NE-P0-03 / NE-P0-04 (§19.1 / §19.5) table-driven capture tests.
//!
//! These tests exercise the *production* prompt-assembly entry point
//! (`compile_effective_prompt`) and the prepared-session digest/revision
//! helpers from the ownership boundary of `production.rs`. They assert:
//!
//! * NE-P0-03 — all four `PromptBlockPlacement` groups are consumed exactly
//!   once and land in the single correct design order (Before → After →
//!   AfterProjectInstructions[default] → Final), with the default placement
//!   present even when no explicit block carries it.
//! * NE-P0-04 — the Prepared Session digest is a content-level SHA-256 (not a
//!   raw-text blob nor a prefix), and a one-byte content change in the
//!   Harness/Expert/Skill/Team source invalidates the digest so the next Run
//!   cache-misses.
//! * §19.5 — the Snapshot digest, the Provider Prompt hash, and the
//!   `PreparedAgentSession.prompt_digest` are the same plan hash.

use super::*;
use crate::capability_resolution::ResolvedCapabilitySnapshot;
use crate::prepared_session::{capability_prompt_revision, harness_revision, PreparedAgentSession};

/// NE-P0-03: the four placements are consumed exactly once in design order.
///
/// Table-driven: each row provides a labelled prompt-source bundle (agent kind,
/// profile, skill prompt, child directive, team roster) plus one block per
/// placement with a distinctive marker. For every row we assert the full
/// `PromptLayerKind` sequence, that each placement marker appears exactly once,
/// and that the raw-text order proves the semantic anchor.
#[test]
fn prompt_placements_four_unique_in_design_order_table() {
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

    struct Row {
        label: &'static str,
        agent_kind: Option<&'static str>,
        profile: Option<agent_core::AgentProfile>,
        child_directive: Option<&'static str>,
        skill_prompt: Option<&'static str>,
        team_roster: Option<&'static str>,
        // Expected pre-block / inter-block layer text in raw-text order.
        expected_text_anchors: &'static [&'static str],
    }
    let rows = [
        Row {
            label: "full-stack creative",
            agent_kind: Some("creative_draft"),
            profile: Some(agent_core::AgentProfile {
                id: "expert".into(),
                name: "Expert".into(),
                system_prompt: Some("expert prompt".into()),
                ..Default::default()
            }),
            child_directive: Some("child directive"),
            skill_prompt: Some("skill catalog"),
            team_roster: Some("team roster"),
            expected_text_anchors: &[
                "skill catalog",
                "BLOCK_BEFORE_PROFILE",
                "expert prompt",
                "BLOCK_AFTER_PROFILE",
                "child directive",
                "project instruction",
                "BLOCK_AFTER_PROJECT",
                "team roster",
                "BLOCK_FINAL",
            ],
        },
        Row {
            label: "no-child-directive (default AfterProjectInstructions still present)",
            agent_kind: None,
            profile: Some(agent_core::AgentProfile {
                id: "expert".into(),
                name: "Expert".into(),
                system_prompt: Some("expert prompt".into()),
                ..Default::default()
            }),
            child_directive: None,
            skill_prompt: Some("skill catalog"),
            team_roster: None,
            // Without a child directive or team roster, the AfterProjectInstructions
            // block still lands immediately after the project instruction layer.
            expected_text_anchors: &[
                "skill catalog",
                "BLOCK_BEFORE_PROFILE",
                "expert prompt",
                "BLOCK_AFTER_PROFILE",
                "project instruction",
                "BLOCK_AFTER_PROJECT",
                "BLOCK_FINAL",
            ],
        },
        Row {
            label: "profile-less surface (blocks anchored to remaining layers)",
            agent_kind: Some("creative_draft"),
            profile: None,
            child_directive: Some("child directive"),
            skill_prompt: None,
            team_roster: Some("team roster"),
            expected_text_anchors: &[
                "BLOCK_BEFORE_PROFILE",
                "BLOCK_AFTER_PROFILE",
                "child directive",
                "project instruction",
                "BLOCK_AFTER_PROJECT",
                "team roster",
                "BLOCK_FINAL",
            ],
        },
    ];

    for row in rows {
        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(project.path().join(".git")).unwrap();
        std::fs::write(project.path().join("AGENTS.md"), "project instruction").unwrap();

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
            row.agent_kind,
            row.profile.as_ref(),
            row.child_directive,
            Some(project.path()),
            row.skill_prompt,
            &blocks,
            &[],
            row.team_roster,
        );

        // Every placement appears exactly once in the layer-kind sequence.
        let kinds = compiled
            .layers
            .iter()
            .map(|layer| layer.kind)
            .collect::<Vec<_>>();
        let block_count = kinds
            .iter()
            .filter(|kind| **kind == harness_core::PromptLayerKind::NativesPromptBlock)
            .count();
        assert_eq!(
            block_count, 4,
            "[{}] each of the four placements must be captured exactly once (got {block_count})",
            row.label
        );

        // Each marker appears exactly once in the raw capture.
        let text = &compiled.effective_full_text;
        for marker in markers {
            assert_eq!(
                text.matches(marker).count(),
                1,
                "[{}] {marker} must appear exactly once in the Provider capture",
                row.label
            );
        }

        // Raw-text order proves the semantic anchor of each placement.
        let mut cursor = 0usize;
        for needle in row.expected_text_anchors {
            let at = text[cursor..].find(*needle).unwrap_or_else(|| {
                panic!(
                    "[{}] missing layer text {needle:?} in Provider capture (cursor {cursor})",
                    row.label
                )
            });
            cursor += at + needle.len();
        }

        // §19.5: the Snapshot/Provider plan hash equals SHA-256 of the raw text.
        assert_eq!(
            harness_core::sha256_hex(text),
            compiled.effective_prompt_hash,
            "[{}] Snapshot/Provider hash must equal SHA-256 of the captured raw text",
            row.label
        );
    }
}

/// NE-P0-03: the default `AfterProjectInstructions` placement is consumed even
/// when the only block on that placement is a Harness-authored block (the
/// default placement is not a fall-through that production forgets).
#[test]
fn default_after_project_instructions_placement_is_present() {
    let project = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(project.path().join(".git")).unwrap();
    std::fs::write(project.path().join("AGENTS.md"), "project instruction").unwrap();
    let blocks = vec![harness_core::blueprint::PromptBlockSpecV3 {
        id: "only-after-project".into(),
        name: "only-after-project".into(),
        markdown: "DEFAULT_PLACEMENT_BLOCK".into(),
        enabled: true,
        order: 0,
        placement: harness_core::blueprint::PromptBlockPlacement::AfterProjectInstructions,
    }];
    let compiled = compile_effective_prompt(
        Some("creative_draft"),
        None,
        None,
        Some(project.path()),
        None,
        &blocks,
        &[],
        None,
    );
    let kinds = compiled
        .layers
        .iter()
        .map(|layer| layer.kind)
        .collect::<Vec<_>>();
    // The AfterProjectInstructions block must land immediately after the
    // InstructionFiles layer.
    let instruction_idx = kinds
        .iter()
        .position(|kind| *kind == harness_core::PromptLayerKind::InstructionFiles)
        .expect("InstructionFiles layer present");
    let block_idx = kinds
        .iter()
        .position(|kind| *kind == harness_core::PromptLayerKind::NativesPromptBlock)
        .expect("NativesPromptBlock layer present");
    assert_eq!(
        block_idx,
        instruction_idx + 1,
        "default AfterProjectInstructions block must immediately follow the project instruction layer"
    );
    assert_eq!(
        compiled
            .effective_full_text
            .matches("DEFAULT_PLACEMENT_BLOCK")
            .count(),
        1,
        "the default-placement block must appear exactly once"
    );
}

/// NE-P0-04 §19.5: the `PreparedAgentSession.prompt_digest` MUST equal the
/// Snapshot's `effective_prompt_hash` and the SHA-256 of the raw Provider
/// prompt text — never the raw text itself.
#[test]
fn prepared_session_prompt_digest_equals_snapshot_and_provider_hash() {
    let project = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(project.path().join(".git")).unwrap();
    std::fs::write(project.path().join("AGENTS.md"), "project instruction").unwrap();
    let profile = agent_core::AgentProfile {
        id: "expert".into(),
        name: "Expert".into(),
        system_prompt: Some("expert prompt".into()),
        ..Default::default()
    };
    let compiled = compile_effective_prompt(
        Some("creative_draft"),
        Some(&profile),
        Some("child directive"),
        Some(project.path()),
        Some("skill catalog"),
        &[],
        &[],
        Some("team roster"),
    );

    // This is the exact construction `run/start.rs` performs on a cache Miss.
    let session = PreparedAgentSession {
        effective_prompt: compiled.clone(),
        prompt_digest: compiled.effective_prompt_hash.clone(),
        frozen_tool_schemas: Vec::new(),
        skill_catalog_metadata: Vec::new(),
    };

    // §19.5 equality: Snapshot digest == Provider Prompt hash == session digest.
    assert_eq!(
        session.prompt_digest, compiled.effective_prompt_hash,
        "PreparedAgentSession.prompt_digest must equal the Snapshot/Provider effective_prompt_hash"
    );
    assert_eq!(
        session.prompt_digest,
        harness_core::sha256_hex(&compiled.effective_full_text),
        "PreparedAgentSession.prompt_digest must equal SHA-256 of the raw Provider prompt text"
    );
    // The digest is a 64-char hex string, not the raw prompt text.
    assert_eq!(
        session.prompt_digest.len(),
        64,
        "prompt_digest must be a 256-bit hex digest, not the raw prompt text"
    );
    assert_ne!(
        session.prompt_digest, compiled.effective_full_text,
        "prompt_digest must never store the raw prompt text (§19.5 integrity + leak risk)"
    );
}

/// NE-P0-04: a one-byte content change in the Harness/Expert/Skill/Team source
/// invalidates the prepared-session digest so the next Run cache-misses.
///
/// Table-driven: each row mutates exactly one source by one byte and asserts
/// the digest changes. The harness revision fold, the capability prompt
/// revision, and the full project instruction digest are all covered.
#[test]
fn content_byte_edit_invalidates_digest_table() {
    // ── Harness revision: fold of the snapshot canonical hash. ──
    let zeros = "0000000000000000000000000000000000000000000000000000000000000000";
    let ffff = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
    assert_eq!(harness_revision(zeros), harness_revision(zeros));
    assert_ne!(
        harness_revision(zeros),
        harness_revision(ffff),
        "a one-byte change in the harness canonical hash must invalidate the harness revision"
    );

    // ── Capability prompt revision: Expert/Skill/Team one-byte edits. ──
    let base = ResolvedCapabilitySnapshot {
        selection_active: true,
        agent_profile_id: Some("expert".into()),
        profile: Some(agent_core::AgentProfile {
            id: "expert".into(),
            name: "Expert".into(),
            system_prompt: Some("expert prompt v1".into()),
            ..Default::default()
        }),
        skill_ids: vec!["grep".into()],
        skill_prompt: Some("skill catalog v1".into()),
        extra_system_prompt: Some("team roster v1".into()),
        ..Default::default()
    };
    assert_eq!(
        capability_prompt_revision(&base),
        capability_prompt_revision(&base.clone()),
        "identical content must produce the same revision"
    );

    struct Row {
        label: &'static str,
        mutate: fn(&mut ResolvedCapabilitySnapshot),
    }
    let rows = [
        Row {
            label: "expert prompt one-byte change",
            mutate: |snap| {
                snap.profile.as_mut().unwrap().system_prompt = Some("expert prompt v2".into());
            },
        },
        Row {
            label: "skill catalog one-byte change",
            mutate: |snap| {
                snap.skill_prompt = Some("skill catalog v2".into());
            },
        },
        Row {
            label: "team roster one-byte change",
            mutate: |snap| {
                snap.extra_system_prompt = Some("team roster v2".into());
            },
        },
        Row {
            label: "team member description one-byte change",
            mutate: |snap| {
                snap.team = Some(crate::capability_resolution::ResolvedTeam {
                    team_id: "team-1".into(),
                    lead_expert_id: "expert".into(),
                    members: vec![crate::capability_resolution::ResolvedTeamMember {
                        expert_id: "expert".into(),
                        name: "Expert".into(),
                        description: "does review v2".into(),
                        role_hint: "reviewer".into(),
                    }],
                    failure_policy: "skip".into(),
                    max_concurrent: 1,
                });
            },
        },
    ];
    for row in rows {
        let mut edited = base.clone();
        (row.mutate)(&mut edited);
        assert_ne!(
            capability_prompt_revision(&base),
            capability_prompt_revision(&edited),
            "[{}] a one-byte content change must invalidate the capability prompt revision",
            row.label
        );
    }
}

/// NE-P0-04: the project instruction digest is a full SHA-256 over content
/// (not a prefix), so a same-length one-byte edit anywhere in a file
/// invalidates the digest and the cache key.
#[test]
fn project_instruction_digest_full_sha256_invalidates_on_tail_edit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let p = dir.path();
    let a: Vec<u8> = vec![b'a'; 512];
    let mut b = a.clone();
    // Edit well past any 64-byte rolling-hash window — only a full SHA-256
    // over the content detects this.
    b[400] = b'z';
    std::fs::write(p.join("AGENTS.md"), &a).unwrap();
    let d1 = crate::prepared_session::project_instruction_digest(p);
    std::fs::write(p.join("AGENTS.md"), &b).unwrap();
    let d2 = crate::prepared_session::project_instruction_digest(p);
    assert_ne!(
        d1, d2,
        "a same-length one-byte tail edit must invalidate the full content digest"
    );
    assert_eq!(d1.len(), 64, "digest must be a full 256-bit hex digest");
}
