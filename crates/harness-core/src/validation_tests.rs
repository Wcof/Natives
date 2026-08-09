//! Blueprint validation unit tests (extracted from `validation.rs`).

use super::*;

use super::*;
use crate::blueprint::{
    HookAdapterSpecV3, HookSemanticsVersion, NativeHookSpecV3, PromptBlockPlacement,
    PromptBlockSpecV3, BLUEPRINT_SCHEMA_VERSION,
};
use crate::hooks::{HookEvent, HookFailurePolicy, HookKind, HookScope, HookSource};

fn definition(source: HookSource, event: HookEvent) -> HookDefinition {
    HookDefinition {
        id: HookId::new(&source, event),
        event,
        source,
        order: 0,
        matcher: None,
        conditions: Vec::new(),
        timeout_ms: 10_000,
        failure_policy: HookFailurePolicy::Fail,
        kind: HookKind::Builtin { name: "x".into() },
    }
}

fn project_hook() -> HookDefinition {
    definition(
        HookSource::file(HookScope::Project, ".claude/settings.json", 0, 0),
        HookEvent::PostToolUse,
    )
}

fn builtin_hook() -> HookDefinition {
    definition(HookSource::builtin("allow-all"), HookEvent::PreToolUse)
}

fn doc(hooks: Vec<HookOverlay>) -> HarnessBlueprint {
    HarnessBlueprint {
        schema_version: BLUEPRINT_SCHEMA_VERSION,
        hook_semantics_version: HookSemanticsVersion::LegacyV1,
        prompt_semantics_version: crate::blueprint::PromptSemanticsVersion::LegacyV1,
        hooks,
        hook_overlays: Vec::new(),
        native_hooks: Vec::new(),
        prompt_blocks: Vec::new(),
        builtin_prompt_replacements: Vec::new(),
    }
}

fn native(adapter: HookAdapterSpecV3) -> NativeHookSpecV3 {
    NativeHookSpecV3 {
        id: "11111111-1111-1111-1111-111111111111".into(),
        name: "Gate".into(),
        enabled: true,
        event: HookEvent::PreToolUse,
        order: 0,
        matcher: Some("*".into()),
        conditions: Vec::new(),
        timeout_ms: 10_000,
        failure_policy: HookFailurePolicy::Fail,
        adapter,
        trust_confirmed: true,
    }
}

fn native_doc(adapter: HookAdapterSpecV3) -> HarnessBlueprint {
    let mut document = HarnessBlueprint::default();
    document.native_hooks.push(native(adapter));
    document
}

#[test]
fn the_empty_document_is_publishable() {
    let report = validate(&HarnessBlueprint::default(), &[project_hook()]);
    assert!(report.findings.is_empty());
    assert!(report.is_publishable());
}

#[test]
fn overlaying_a_locked_hook_blocks_publish() {
    let hook = builtin_hook();
    let mut overlay = HookOverlay::new(hook.id.clone());
    overlay.enabled = Some(false);
    let report = validate(&doc(vec![overlay]), &[hook]);
    assert!(!report.is_publishable());
    assert_eq!(
        report.errors().map(|f| f.code.as_str()).collect::<Vec<_>>(),
        vec!["harness.overlay_on_locked_hook"]
    );
}

#[test]
fn an_unknown_hook_warns_but_still_publishes() {
    let mut overlay = HookOverlay::new(HookId::new(
        &HookSource::file(HookScope::Project, ".claude/absent.json", 0, 0),
        HookEvent::PreToolUse,
    ));
    overlay.timeout_ms = Some(5_000);
    let report = validate(&doc(vec![overlay]), &[project_hook()]);
    assert!(
        report.is_publishable(),
        "a missing source is not a broken document"
    );
    assert_eq!(
        report
            .findings
            .iter()
            .map(|f| f.code.as_str())
            .collect::<Vec<_>>(),
        vec!["harness.unknown_hook"]
    );
}

#[test]
fn timeouts_outside_the_production_clamp_are_errors() {
    let hook = project_hook();
    for bad in [0, 1, MAX_TIMEOUT_MS + 1] {
        let mut overlay = HookOverlay::new(hook.id.clone());
        overlay.timeout_ms = Some(bad);
        let report = validate(&doc(vec![overlay]), std::slice::from_ref(&hook));
        assert!(!report.is_publishable(), "{bad} should be rejected");
    }
    let mut ok = HookOverlay::new(hook.id.clone());
    ok.timeout_ms = Some(30_000);
    assert!(validate(&doc(vec![ok]), &[hook]).is_publishable());
}

#[test]
fn a_blank_matcher_is_an_error_not_a_silent_wildcard() {
    let hook = project_hook();
    let mut overlay = HookOverlay::new(hook.id.clone());
    overlay.matcher = Some("   ".into());
    let report = validate(&doc(vec![overlay]), &[hook]);
    assert_eq!(
        report.errors().map(|f| f.code.as_str()).collect::<Vec<_>>(),
        vec!["harness.blank_matcher"]
    );
}

#[test]
fn an_overlay_that_sets_nothing_warns() {
    let hook = project_hook();
    let report = validate(&doc(vec![HookOverlay::new(hook.id.clone())]), &[hook]);
    assert!(report.is_publishable());
    assert_eq!(
        report
            .findings
            .iter()
            .map(|f| f.code.as_str())
            .collect::<Vec<_>>(),
        vec!["harness.empty_overlay"]
    );
}

#[test]
fn native_adapters_reject_incomplete_configuration() {
    let cases = [
        native_doc(HookAdapterSpecV3::Command {
            program: " ".into(),
            args: Vec::new(),
            working_dir_policy: Default::default(),
            secret_env_refs: Default::default(),
            trusted: true,
            mode: Default::default(),
        }),
        native_doc(HookAdapterSpecV3::Http {
            url: " ".into(),
            allow_hosts: Vec::new(),
            headers: Default::default(),
            secret_header_refs: Default::default(),
        }),
        native_doc(HookAdapterSpecV3::McpTool {
            server_id: " ".into(),
            tool_name: " ".into(),
            input_template: Some("{".into()),
        }),
        native_doc(HookAdapterSpecV3::Prompt {
            template: " ".into(),
            model_override: None,
        }),
        native_doc(HookAdapterSpecV3::Agent {
            prompt: " ".into(),
            model_override: None,
            max_steps: 0,
            readonly_tools: Vec::new(),
        }),
    ];
    for document in cases {
        assert!(!validate(&document, &[]).is_publishable());
    }
}

#[test]
fn native_hook_common_fields_and_prompt_blocks_are_validated() {
    let mut document = native_doc(HookAdapterSpecV3::Prompt {
        template: "allow or deny".into(),
        model_override: None,
    });
    document.native_hooks[0].name = " ".into();
    document.native_hooks[0].matcher = Some(" ".into());
    document.native_hooks[0].timeout_ms = 1;
    document.prompt_blocks.push(PromptBlockSpecV3 {
        id: "22222222-2222-2222-2222-222222222222".into(),
        name: " ".into(),
        markdown: " ".into(),
        enabled: true,
        order: 0,
        placement: PromptBlockPlacement::Final,
    });
    let report = validate(&document, &[]);
    let codes = report
        .errors()
        .map(|finding| finding.code.as_str())
        .collect::<Vec<_>>();
    assert!(codes.contains(&"harness.blank_native_hook_name"));
    assert!(codes.contains(&"harness.blank_matcher"));
    assert!(codes.contains(&"harness.timeout_out_of_range"));
    assert!(codes.contains(&"harness.blank_prompt_block_name"));
    assert!(codes.contains(&"harness.blank_prompt_block"));
}

#[test]
fn diff_includes_native_hooks_prompt_blocks_and_prompt_semantics() {
    let before = HarnessBlueprint::default();
    let mut after = native_doc(HookAdapterSpecV3::Prompt {
        template: "allow or deny".into(),
        model_override: None,
    });
    after.prompt_semantics_version = crate::blueprint::PromptSemanticsVersion::SequentialV2;
    after.prompt_blocks.push(PromptBlockSpecV3 {
        id: "22222222-2222-2222-2222-222222222222".into(),
        name: "Rules".into(),
        markdown: "Be safe.".into(),
        enabled: true,
        order: 0,
        placement: PromptBlockPlacement::Final,
    });
    let fields = diff(&before, &after)
        .into_iter()
        .map(|change| change.field)
        .collect::<Vec<_>>();
    assert!(fields.contains(&"prompt_semantics_version".into()));
    assert!(fields.contains(&"native_hooks".into()));
    assert!(fields.contains(&"prompt_blocks".into()));
}

#[test]
fn diff_reports_added_removed_and_changed_fields() {
    let hook = project_hook();
    let mut before = HookOverlay::new(hook.id.clone());
    before.timeout_ms = Some(1_000);
    before.matcher = Some("Edit".into());
    let mut after = HookOverlay::new(hook.id.clone());
    after.timeout_ms = Some(2_000);
    after.enabled = Some(false);

    let changes = diff(&doc(vec![before]), &doc(vec![after]));
    assert_eq!(
        changes
            .iter()
            .map(|c| (c.field.as_str(), c.from.clone(), c.to.clone()))
            .collect::<Vec<_>>(),
        vec![
            ("enabled", serde_json::Value::Null, serde_json::json!(false)),
            (
                "matcher",
                serde_json::json!("Edit"),
                serde_json::Value::Null
            ),
            (
                "timeout_ms",
                serde_json::json!(1_000),
                serde_json::json!(2_000)
            ),
        ]
    );
}

#[test]
fn diff_reports_a_semantics_move_first() {
    let after = HarnessBlueprint {
        hook_semantics_version: HookSemanticsVersion::SequentialV2,
        ..HarnessBlueprint::default()
    };
    let changes = diff(&HarnessBlueprint::default(), &after);
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].field, "hook_semantics_version");
    assert!(changes[0].hook_id.is_none());
}

#[test]
fn identical_documents_diff_to_nothing() {
    let hook = project_hook();
    let mut overlay = HookOverlay::new(hook.id);
    overlay.order = Some(3);
    let document = doc(vec![overlay]);
    assert!(diff(&document, &document).is_empty());
}
