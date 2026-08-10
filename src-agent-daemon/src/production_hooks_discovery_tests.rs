//! Discovery / compile / build tests for the Production Hook seam (extracted
//! from `production_hooks.rs`, task-01 structure).

use super::*;
use agent_core::{HookDecision, HookRegistry, HookRequest, HookResponse};
// Only the assertions need a concrete `Duration`; production code goes
// through `HookDefinition::timeout()`.
use std::time::Duration;

/// Behaviour snapshot for the pre-`discover`/`compile` split (task T0).
///
/// The registry stores opaque `Box<dyn HookHandler>`, so these tests pin
/// observable dispatch behaviour rather than internal structure. The
/// refactor must keep every assertion below byte-identical.
const ALL_EVENTS: [HookEvent; 16] = [
    HookEvent::SessionStart,
    HookEvent::SessionEnd,
    HookEvent::UserPromptSubmit,
    HookEvent::PreToolUse,
    HookEvent::PostToolUse,
    HookEvent::PostToolUseFailure,
    HookEvent::PermissionRequest,
    HookEvent::PermissionDenied,
    HookEvent::Notification,
    HookEvent::SubagentStart,
    HookEvent::SubagentStop,
    HookEvent::PreCompact,
    HookEvent::PostCompact,
    HookEvent::Stop,
    HookEvent::StopFailure,
    HookEvent::Error,
];

/// A loopback URL is rejected by `validate_http_hook_url` before any socket
/// is opened, so an `http` hook is a fast, hermetic handler-counting probe.
const PROBE_URL: &str = "http://127.0.0.1:1/hook";

struct TempProject {
    root: std::path::PathBuf,
}

impl TempProject {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("natives-hooks-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        Self { root }
    }

    fn write(&self, rel: &str, body: &str) -> &Self {
        let path = self.root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
        self
    }

    fn hooks(&self) -> HookRegistry {
        build_production_hooks_for_project(Some(&self.root))
    }
}

impl Drop for TempProject {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

async fn dispatch(hooks: &HookRegistry, event: HookEvent, tool: Option<&str>) -> Vec<HookResponse> {
    hooks
        .dispatch(HookRequest {
            event,
            run_id: "run".into(),
            tool_name: tool.map(str::to_string),
            input: serde_json::json!({}),
        })
        .await
}

/// `build_production_hooks_for_project` reads two ambient env vars. Tests
/// must account for them rather than assume a clean environment.
fn env_hook_count(var: &str) -> usize {
    usize::from(
        std::env::var(var)
            .ok()
            .is_some_and(|v| !v.trim().is_empty()),
    )
}

fn probe_group(event: &str, command: &str) -> String {
    format!(r#"{{"hooks":{{"{event}":[{{"hooks":[{{"type":"http","url":"{command}"}}]}}]}}}}"#)
}

#[tokio::test]
async fn builtin_defaults_allow_every_event_without_project_hooks() {
    let hooks = build_production_hooks_for_project(None);
    for event in ALL_EVENTS {
        let extra = match event {
            HookEvent::PreToolUse => env_hook_count("NATIVES_HOOK_CMD"),
            HookEvent::PostToolUse => env_hook_count("NATIVES_HOOK_HTTP"),
            _ => 0,
        };
        let responses = dispatch(&hooks, event, Some("read_file")).await;
        assert_eq!(
            responses.len(),
            1 + extra,
            "event {event:?} should have exactly one builtin default handler"
        );
        assert!(
            matches!(responses[0].decision, HookDecision::Allow),
            "event {event:?} builtin default must Allow"
        );
    }
}

#[tokio::test]
async fn security_fail_closed_is_enabled_after_build() {
    assert!(build_production_hooks_for_project(None).fail_closed_security);
}

#[tokio::test]
async fn builtin_default_dispatches_before_project_hooks() {
    let project = TempProject::new();
    project.write(".claude/hooks.json", &probe_group("PreToolUse", PROBE_URL));
    let responses = dispatch(&project.hooks(), HookEvent::PreToolUse, Some("read_file")).await;

    assert_eq!(responses.len(), 2 + env_hook_count("NATIVES_HOOK_CMD"));
    assert!(
        matches!(responses[0].decision, HookDecision::Allow),
        "builtin allow-all must be registered before project hooks"
    );
    assert!(matches!(responses[1].decision, HookDecision::Deny { .. }));
}

#[tokio::test]
async fn every_candidate_project_file_is_loaded() {
    let project = TempProject::new();
    for rel in [
        ".claude/hooks.json",
        ".claude/settings.json",
        ".claude/settings.local.json",
        ".agents/hooks.json",
        ".agents/settings.json",
        ".grok/hooks.json",
        ".natives/hooks.json",
    ] {
        project.write(rel, &probe_group("Notification", PROBE_URL));
    }
    let responses = dispatch(&project.hooks(), HookEvent::Notification, None).await;
    assert_eq!(
        responses.len(),
        8,
        "one builtin default plus seven candidate project files"
    );
}

#[tokio::test]
async fn unknown_event_names_are_ignored() {
    let project = TempProject::new();
    project.write(
        ".natives/hooks.json",
        &probe_group("BogusEventName", PROBE_URL),
    );
    for event in ALL_EVENTS {
        let extra = match event {
            HookEvent::PreToolUse => env_hook_count("NATIVES_HOOK_CMD"),
            HookEvent::PostToolUse => env_hook_count("NATIVES_HOOK_HTTP"),
            _ => 0,
        };
        assert_eq!(
            dispatch(&project.hooks(), event, None).await.len(),
            1 + extra,
            "unknown event name must not register a handler on {event:?}"
        );
    }
}

#[tokio::test]
async fn snake_case_event_aliases_are_accepted() {
    let project = TempProject::new();
    project.write(
        ".natives/hooks.json",
        &probe_group("pre_compact", PROBE_URL),
    );
    assert_eq!(
        dispatch(&project.hooks(), HookEvent::PreCompact, None)
            .await
            .len(),
        2
    );
}

#[tokio::test]
async fn bare_object_without_hooks_wrapper_is_parsed() {
    let project = TempProject::new();
    project.write(
        ".natives/hooks.json",
        &format!(r#"{{"Notification":[{{"hooks":[{{"type":"http","url":"{PROBE_URL}"}}]}}]}}"#),
    );
    assert_eq!(
        dispatch(&project.hooks(), HookEvent::Notification, None)
            .await
            .len(),
        2
    );
}

#[tokio::test]
async fn group_without_inner_hooks_array_is_a_single_handler() {
    let project = TempProject::new();
    project.write(
        ".natives/hooks.json",
        &format!(r#"{{"hooks":{{"Notification":[{{"type":"http","url":"{PROBE_URL}"}}]}}}}"#),
    );
    assert_eq!(
        dispatch(&project.hooks(), HookEvent::Notification, None)
            .await
            .len(),
        2
    );
}

#[tokio::test]
async fn malformed_and_incomplete_entries_are_skipped() {
    let project = TempProject::new();
    project
        .write(".claude/hooks.json", "{ not json at all")
        .write(
            ".claude/settings.json",
            r#"{"hooks":{"Notification":[{"hooks":[{"type":"command"}]}]}}"#,
        )
        .write(
            ".agents/hooks.json",
            r#"{"hooks":{"Notification":[{"hooks":[{"type":"command","command":"   "}]}]}}"#,
        )
        .write(
            ".grok/hooks.json",
            r#"{"hooks":{"Notification":[{"hooks":[{"type":"http"}]}]}}"#,
        )
        .write(
            ".natives/hooks.json",
            r#"{"hooks":{"Notification":[{"hooks":[{"type":"websocket","url":"ws://x"}]}]}}"#,
        );
    assert_eq!(
        dispatch(&project.hooks(), HookEvent::Notification, None)
            .await
            .len(),
        1,
        "invalid JSON, missing command, blank command, missing url, and \
         unknown handler type must all be skipped"
    );
}

#[tokio::test]
async fn matcher_scopes_handlers_to_tool_names() {
    let project = TempProject::new();
    project.write(
        ".natives/hooks.json",
        &format!(
            r#"{{"hooks":{{"Notification":[
                {{"matcher":"Bash|run_*","hooks":[{{"type":"http","url":"{PROBE_URL}"}}]}},
                {{"matcher":"*","hooks":[{{"type":"http","url":"{PROBE_URL}"}}]}}
            ]}}}}"#
        ),
    );
    let hooks = project.hooks();
    assert_eq!(
        dispatch(&hooks, HookEvent::Notification, Some("Bash"))
            .await
            .len(),
        3
    );
    assert_eq!(
        dispatch(&hooks, HookEvent::Notification, Some("run_command"))
            .await
            .len(),
        3
    );
    assert_eq!(
        dispatch(&hooks, HookEvent::Notification, Some("read_file"))
            .await
            .len(),
        2,
        "only the wildcard matcher applies to an unmatched tool"
    );
}

// ── Discovery layer (task T3) ──────────────────────────────────────────
//
// These assert facts the pre-split code could not express at all, because a
// registered Hook was an opaque `Box<dyn HookHandler>`.

fn definitions_from(project: &TempProject) -> Vec<HookDefinition> {
    discover_production_hooks(Some(&project.root))
}

fn file_definitions(project: &TempProject) -> Vec<HookDefinition> {
    definitions_from(project)
        .into_iter()
        .filter(|d| d.source.scope == HookScope::Project)
        .collect()
}

#[test]
fn discovery_yields_one_builtin_default_per_event_first() {
    let defs = discover_production_hooks(None);
    let builtins: Vec<_> = defs
        .iter()
        .filter(|d| d.source.scope == HookScope::Builtin)
        .collect();

    assert_eq!(builtins.len(), 16);
    assert_eq!(
        builtins.iter().map(|d| d.event).collect::<Vec<_>>(),
        HookEvent::ALL.to_vec(),
        "builtin defaults must be discovered in canonical event order"
    );
    for def in builtins {
        assert_eq!(def.order, 0, "builtin default must dispatch first");
        assert_eq!(
            def.id.as_str(),
            format!("builtin/allow-all#{}", def.event.as_str())
        );
    }
}

#[test]
fn discovery_records_file_group_and_entry_provenance() {
    let project = TempProject::new();
    project.write(
        ".claude/settings.json",
        &format!(
            r#"{{"hooks":{{"PostToolUse":[
                {{"matcher":"Edit","hooks":[
                    {{"type":"http","url":"{PROBE_URL}"}},
                    {{"type":"http","url":"{PROBE_URL}"}}
                ]}},
                {{"matcher":"Write","hooks":[{{"type":"http","url":"{PROBE_URL}"}}]}}
            ]}}}}"#
        ),
    );
    let defs = file_definitions(&project);

    assert_eq!(defs.len(), 3);
    assert_eq!(
        defs.iter().map(|d| d.id.as_str()).collect::<Vec<_>>(),
        vec![
            "project/.claude/settings.json#PostToolUse[0]/0",
            "project/.claude/settings.json#PostToolUse[0]/1",
            "project/.claude/settings.json#PostToolUse[1]/0",
        ]
    );
    assert_eq!(
        defs.iter()
            .map(|d| d.matcher.as_deref())
            .collect::<Vec<_>>(),
        vec![Some("Edit"), Some("Edit"), Some("Write")]
    );
    // The builtin default already claimed ordinal 0 for this event.
    assert_eq!(
        defs.iter().map(|d| d.order).collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
}

#[test]
fn timeout_is_clamped_between_one_and_six_hundred_seconds() {
    let project = TempProject::new();
    project.write(
        ".natives/hooks.json",
        &format!(
            r#"{{"hooks":{{"Notification":[{{"hooks":[
                {{"type":"http","url":"{PROBE_URL}","timeout":0}},
                {{"type":"http","url":"{PROBE_URL}","timeout":9999}},
                {{"type":"http","url":"{PROBE_URL}","timeout":30}},
                {{"type":"http","url":"{PROBE_URL}"}}
            ]}}]}}}}"#
        ),
    );
    assert_eq!(
        file_definitions(&project)
            .iter()
            .map(|d| d.timeout_ms)
            .collect::<Vec<_>>(),
        vec![1_000, 600_000, 30_000, 10_000],
        "clamp to [1s, 600s] with a 10s default"
    );
}

#[test]
fn command_hooks_are_shell_wrapped_unless_args_are_explicit() {
    let project = TempProject::new();
    project.write(
        ".natives/hooks.json",
        r#"{"hooks":{"Notification":[{"hooks":[
            {"type":"command","command":"echo hi"},
            {"type":"command","command":"/usr/bin/echo","args":["hi"]}
        ]}]}}"#,
    );
    let defs = file_definitions(&project);
    assert_eq!(defs.len(), 2);

    let expected_wrapper: (&str, Vec<String>) = if cfg!(windows) {
        ("cmd.exe", vec!["/C".into(), "echo hi".into()])
    } else {
        ("/bin/sh", vec!["-lc".into(), "echo hi".into()])
    };
    assert_eq!(
        defs[0].kind,
        HookKind::Command {
            program: expected_wrapper.0.to_string(),
            args: expected_wrapper.1,
            trusted: true,
        }
    );
    assert_eq!(
        defs[1].kind,
        HookKind::Command {
            program: "/usr/bin/echo".into(),
            args: vec!["hi".into()],
            trusted: true,
        }
    );
}

#[test]
fn discovery_covers_all_seven_project_candidates_in_load_order() {
    let project = TempProject::new();
    for [dir, file] in PROJECT_CANDIDATES {
        project.write(
            &format!("{dir}/{file}"),
            &probe_group("Notification", PROBE_URL),
        );
    }
    assert_eq!(
        file_definitions(&project)
            .iter()
            .map(|d| d.source.origin.clone())
            .collect::<Vec<_>>(),
        PROJECT_CANDIDATES
            .iter()
            .map(|[dir, file]| format!("{dir}/{file}"))
            .collect::<Vec<_>>()
    );
}

#[test]
fn every_discovered_hook_has_a_unique_identity() {
    let project = TempProject::new();
    for [dir, file] in PROJECT_CANDIDATES {
        project.write(
            &format!("{dir}/{file}"),
            &format!(
                r#"{{"hooks":{{"Notification":[{{"hooks":[
                    {{"type":"http","url":"{PROBE_URL}"}},
                    {{"type":"http","url":"{PROBE_URL}"}}
                ]}}]}}}}"#
            ),
        );
    }
    let ids: Vec<_> = definitions_from(&project)
        .into_iter()
        .map(|d| d.id)
        .collect();
    let unique: std::collections::BTreeSet<_> = ids.iter().cloned().collect();
    assert_eq!(ids.len(), unique.len(), "hook identities must not collide");
}

#[tokio::test]
async fn discovery_order_is_dispatch_order() {
    let project = TempProject::new();
    project.write(
        ".natives/hooks.json",
        &format!(
            r#"{{"hooks":{{"Notification":[{{"hooks":[
                {{"type":"http","url":"{PROBE_URL}"}},
                {{"type":"http","url":"ftp://example.test/hook"}}
            ]}}]}}}}"#
        ),
    );
    let defs = definitions_from(&project);
    let notification: Vec<_> = defs
        .iter()
        .filter(|d| d.event == HookEvent::Notification)
        .collect();
    assert_eq!(notification.len(), 3);

    let responses = dispatch(
        &compile_production_hooks(&defs, Some(&project.root)),
        HookEvent::Notification,
        None,
    )
    .await;
    assert_eq!(responses.len(), notification.len());
    assert!(matches!(responses[0].decision, HookDecision::Allow));
    // Distinct denial reasons prove the two file hooks kept their order.
    let reason_of = |i: usize| match &responses[i].decision {
        HookDecision::Deny { reason } => reason.clone(),
        other => panic!("expected Deny, got {other:?}"),
    };
    assert!(reason_of(1).contains("private/loopback"));
    assert!(reason_of(2).contains("only http/https"));
}

#[tokio::test]
async fn compiling_discovered_definitions_matches_the_direct_build() {
    let project = TempProject::new();
    project
        .write(".claude/hooks.json", &probe_group("PreToolUse", PROBE_URL))
        .write(
            ".natives/hooks.json",
            &probe_group("Notification", PROBE_URL),
        );

    let direct = build_production_hooks_for_project(Some(&project.root));
    let composed = compile_production_hooks(&definitions_from(&project), Some(&project.root));

    for event in HookEvent::ALL {
        assert_eq!(
            dispatch(&direct, event, Some("Bash")).await.len(),
            dispatch(&composed, event, Some("Bash")).await.len(),
            "handler count diverged on {event:?}"
        );
    }
    assert_eq!(direct.fail_closed_security, composed.fail_closed_security);
}

#[test]
fn unknown_builtin_names_compile_to_nothing_rather_than_panicking() {
    let source = HookSource::builtin("from-a-newer-daemon");
    let definition = HookDefinition {
        id: HookId::new(&source, HookEvent::Notification),
        event: HookEvent::Notification,
        source,
        order: 0,
        matcher: None,
        conditions: Vec::new(),
        timeout_ms: 0,
        failure_policy: HookFailurePolicy::Fail,
        kind: HookKind::Builtin {
            name: "from-a-newer-daemon".into(),
        },
    };
    let hooks = compile_production_hooks(&[definition], None);
    assert!(hooks.fail_closed_security);
}

/// Phase 1 acceptance: a built registry can answer, for any project, which
/// Hooks are attached, where each came from, in what order, what it matches,
/// how long it may run, and whether it is trusted.
#[test]
fn built_registry_answers_the_full_provenance_question() {
    let project = TempProject::new();
    project
        .write(
            ".claude/settings.json",
            r#"{"hooks":{"PreToolUse":[{"matcher":"Bash|run_*","hooks":[
                {"type":"command","command":"./scripts/audit.sh","timeout":45}
            ]}]}}"#,
        )
        .write(
            ".natives/hooks.json",
            &format!(
                r#"{{"hooks":{{"PreToolUse":[{{"hooks":[
                    {{"type":"http","url":"{PROBE_URL}"}}
                ]}}]}}}}"#
            ),
        );

    let described = build_production_hooks_for_project(Some(&project.root))
        .describe_event(HookEvent::PreToolUse);

    let env_extra = env_hook_count("NATIVES_HOOK_CMD");
    assert_eq!(described.len(), 3 + env_extra);

    // 1. builtin default, first
    assert_eq!(described[0].source.scope, HookScope::Builtin);
    assert_eq!(described[0].order, 0);

    // 2. the project command hook, with file, group, and entry provenance
    let audit = &described[1];
    assert_eq!(
        audit.id.as_str(),
        "project/.claude/settings.json#PreToolUse[0]/0"
    );
    assert_eq!(audit.source.scope, HookScope::Project);
    assert_eq!(audit.source.origin, ".claude/settings.json");
    assert_eq!(audit.source.group_index, Some(0));
    assert_eq!(audit.source.entry_index, Some(0));
    assert_eq!(audit.order, 1);
    assert_eq!(audit.matcher.as_deref(), Some("Bash|run_*"));
    assert_eq!(audit.timeout(), Duration::from_secs(45));
    assert_eq!(audit.failure_policy, HookFailurePolicy::Fail);
    assert!(
        matches!(&audit.kind, HookKind::Command { trusted, .. } if *trusted),
        "project command hooks are trusted"
    );

    // 3. the second file's http hook, ordered after it
    assert_eq!(described[2].source.origin, ".natives/hooks.json");
    assert_eq!(described[2].order, 2);
    assert!(matches!(described[2].kind, HookKind::Http { .. }));
}

#[test]
fn describe_covers_every_event_that_has_a_handler() {
    let hooks = build_production_hooks_for_project(None);
    let described = hooks.describe();
    assert_eq!(
        described.iter().map(|d| d.event).collect::<Vec<_>>(),
        HookEvent::ALL.to_vec(),
        "the sixteen builtin defaults must all be describable"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn loads_claude_settings_matchers_and_permission_request() {
    let project = std::env::temp_dir().join(format!("natives-hooks-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(project.join(".claude")).unwrap();
    std::fs::write(
        project.join(".claude").join("settings.json"),
        r#"{
          "hooks": {
            "PreToolUse": [{
              "matcher": "Bash|run_command",
              "hooks": [{ "type": "command", "command": "/usr/bin/false || /usr/bin/true" }]
            }],
            "PermissionRequest": [{
              "hooks": [{ "type": "command", "command": "/usr/bin/true" }]
            }]
          }
        }"#,
    )
    .unwrap();
    let hooks = build_production_hooks_for_project(Some(&project));

    let unmatched = hooks
        .dispatch(HookRequest {
            event: HookEvent::PreToolUse,
            run_id: "run".into(),
            tool_name: Some("read_file".into()),
            input: serde_json::json!({}),
        })
        .await;
    let matched = hooks
        .dispatch(HookRequest {
            event: HookEvent::PreToolUse,
            run_id: "run".into(),
            tool_name: Some("run_command".into()),
            input: serde_json::json!({}),
        })
        .await;
    let permission = hooks
        .dispatch(HookRequest {
            event: HookEvent::PermissionRequest,
            run_id: "run".into(),
            tool_name: Some("write_file".into()),
            input: serde_json::json!({}),
        })
        .await;

    let _ = std::fs::remove_dir_all(&project);
    assert_eq!(unmatched.len(), 1);
    assert_eq!(matched.len(), 2);
    assert_eq!(permission.len(), 2);
    assert!(HookRegistry::aggregate_allow(&matched).is_ok());
}
