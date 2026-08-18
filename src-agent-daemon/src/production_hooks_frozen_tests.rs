//! Run-level frozen Hook Dispatcher tests (extracted from `production_hooks.rs`,
//! task-01 structure).

use super::*;
use agent_core::{HookDecision, HookRegistry, HookRequest, HookResponse};
use harness_core::blueprint::{HookAdapterSpecV3, NativeHookSpecV3};
use harness_core::hooks::HookInvocationStatus;

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

fn probe_group(event: &str, command: &str) -> String {
    format!(r#"{{"hooks":{{"{event}":[{{"hooks":[{{"type":"http","url":"{command}"}}]}}]}}}}"#)
}

fn definitions_from(project: &TempProject) -> Vec<HookDefinition> {
    discover_production_hooks(Some(&project.root))
}

fn file_definitions(project: &TempProject) -> Vec<HookDefinition> {
    definitions_from(project)
        .into_iter()
        .filter(|d| d.source.scope == HookScope::Project)
        .collect()
}

/// A Hook that always fails, so failure-policy resolution can be exercised
/// without spawning processes or opening sockets.
struct FixedFailHook;

#[async_trait::async_trait]
impl HookHandler for FixedFailHook {
    async fn handle(&self, _request: HookRequest) -> HookResponse {
        HookOutcome::Failed {
            reason: "boom".into(),
        }
        .into_response()
    }

    async fn handle_outcome(&self, _request: HookRequest) -> HookOutcome {
        HookOutcome::Failed {
            reason: "boom".into(),
        }
    }
}

fn failing_registry(event: HookEvent, policy: HookFailurePolicy) -> HookRegistry {
    let mut registry = HookRegistry::new();
    registry.enable_security_fail_closed();
    let source = HookSource::builtin("fixed-fail");
    let definition = HookDefinition {
        id: HookId::new(&source, event),
        event,
        source,
        order: 0,
        matcher: None,
        conditions: Vec::new(),
        timeout_ms: 0,
        failure_policy: policy,
        kind: HookKind::Builtin {
            name: "fixed-fail".into(),
        },
    };
    registry.register_defined(definition, Box::new(FixedFailHook));
    registry
}

/// The freeze is idempotent per run: the first registration wins and a
/// later registration compiled from a mid-Run change is ignored.
#[test]
fn frozen_dispatcher_freezes_a_run_and_ignores_later_registrations() {
    let project = TempProject::new();
    project.write(
        ".natives/hooks.json",
        &probe_group("Notification", PROBE_URL),
    );
    let events = EventSequencer::memory_only();

    let first = freeze_run_hooks("run-freeze", "plan-v1", project.hooks(), events.clone());
    assert_eq!(first.plan_hash(), "plan-v1");
    assert_eq!(
        frozen_dispatcher_for_run("run-freeze")
            .expect("run frozen")
            .plan_hash(),
        "plan-v1"
    );

    let second = freeze_run_hooks("run-freeze", "plan-v2", project.hooks(), events);
    assert_eq!(
        second.plan_hash(),
        "plan-v1",
        "a second freeze for the same run must be a no-op"
    );
    assert_eq!(
        frozen_dispatcher_for_run("run-freeze")
            .expect("run frozen")
            .plan_hash(),
        "plan-v1"
    );
}

#[test]
fn frozen_dispatcher_is_removed_after_terminal_settlement() {
    let run_id = format!("run-terminal-{}", uuid::Uuid::new_v4());
    let frozen = freeze_run_hooks(
        &run_id,
        "effective-prompt-hash",
        HookRegistry::new(),
        EventSequencer::memory_only(),
    );
    let terminal = frozen.retain_until_terminal();
    assert!(frozen_dispatcher_for_run(&run_id).is_some());

    drop(terminal);

    assert!(frozen_dispatcher_for_run(&run_id).is_none());
}

#[test]
fn frozen_dispatcher_retains_resolved_native_permission_hook() {
    let native = NativeHookSpecV3 {
        id: "11111111-1111-1111-1111-111111111111".into(),
        name: "Native permission gate".into(),
        enabled: true,
        event: HookEvent::PermissionRequest,
        order: 0,
        matcher: None,
        conditions: Vec::new(),
        timeout_ms: 10_000,
        failure_policy: HookFailurePolicy::Fail,
        adapter: HookAdapterSpecV3::Http {
            url: PROBE_URL.into(),
            allow_hosts: Vec::new(),
            headers: Default::default(),
            secret_header_refs: Default::default(),
        },
        trust_confirmed: true,
    };
    let definition = HookDefinition {
        id: HookId::native(&native.id, native.event),
        event: native.event,
        source: HookSource::builtin("native-harness"),
        order: native.order,
        matcher: native.matcher.clone(),
        conditions: native.conditions.clone(),
        timeout_ms: native.timeout_ms,
        failure_policy: native.failure_policy,
        kind: native.adapter.to_hook_kind(),
    };
    let run_id = format!("run-native-{}", uuid::Uuid::new_v4());
    let frozen = freeze_run_hooks(
        &run_id,
        "effective-prompt-hash",
        compile_production_hooks_with_native(&[definition], &[native], None),
        EventSequencer::memory_only(),
    );
    let terminal = frozen.retain_until_terminal();

    assert_eq!(
        frozen.describe_event(HookEvent::PermissionRequest)[0]
            .id
            .as_str(),
        "native/11111111-1111-1111-1111-111111111111#PermissionRequest"
    );

    drop(terminal);
}

/// Every dispatched Hook emits exactly one HookInvocationStarted and one
/// HookInvocationCompleted, through the same EventSequencer the registry
/// was frozen with — a single event source, and no dangling Started.
#[tokio::test]
async fn frozen_dispatcher_emits_started_and_completed_trace_through_event_sequencer() {
    let project = TempProject::new();
    project.write(
        ".natives/hooks.json",
        &probe_group("Notification", PROBE_URL),
    );
    let events = EventSequencer::memory_only();
    let frozen = freeze_run_hooks("run-trace", "plan-trace", project.hooks(), events.clone());

    let responses = frozen
        .dispatch(HookRequest {
            event: HookEvent::Notification,
            run_id: "run-trace".into(),
            tool_name: None,
            input: serde_json::json!({}),
        })
        .await;
    // allow-all builtin + the loopback-rejected probe both dispatch.
    assert_eq!(responses.len(), 2);

    let replayed = events.replay_after("run-trace", 0);
    let started: Vec<_> = replayed
        .iter()
        .filter(|e| matches!(&e.payload, RunEventKind::HookInvocationStarted { .. }))
        .collect();
    let completed: Vec<_> = replayed
        .iter()
        .filter(|e| matches!(&e.payload, RunEventKind::HookInvocationCompleted { .. }))
        .collect();
    assert_eq!(
        started.len(),
        2,
        "every dispatched hook must emit HookInvocationStarted"
    );
    assert_eq!(
        completed.len(),
        2,
        "every dispatched hook must emit HookInvocationCompleted"
    );

    for start in &started {
        let start_trace = trace_from_started_event(start, "plan-trace").expect("started maps");
        assert_eq!(start_trace.plan_hash, "plan-trace");
        assert_eq!(start_trace.status, HookInvocationStatus::Started);
        assert!(
            completed
                .iter()
                .any(|c| trace_from_completed_event(c, "plan-trace")
                    .is_some_and(|t| t.invocation_id == start_trace.invocation_id)),
            "started invocation {} must have a completed twin",
            start_trace.invocation_id
        );
    }
}

/// Resolving through the tool-path seam compiles exactly once per run and
/// is immune to mid-Run `hooks.json` edits — the edited file only reaches a
/// *new* Run.
#[tokio::test]
async fn resolve_frozen_dispatcher_compiles_once_and_is_immune_to_mid_run_edits() {
    let project = TempProject::new();
    project.write(
        ".natives/hooks.json",
        &probe_group("Notification", PROBE_URL),
    );
    let run_id = format!("run-resolve-{}", uuid::Uuid::new_v4());
    let events = EventSequencer::memory_only();

    let first = resolve_frozen_dispatcher(&run_id, events.clone(), Some(project.root.as_path()));
    let plan_hash = first.plan_hash().to_string();
    assert_eq!(
        frozen_dispatcher_for_run(&run_id).unwrap().plan_hash(),
        plan_hash
    );

    // Mid-Run edit: add a second probe hook to the same file.
    project.write(
        ".natives/hooks.json",
        &format!(
            r#"{{"hooks":{{"Notification":[{{"hooks":[
                {{"type":"http","url":"{PROBE_URL}"}},
                {{"type":"http","url":"{PROBE_URL}"}}
            ]}}]}}}}"#
        ),
    );

    let second = resolve_frozen_dispatcher(&run_id, events, Some(project.root.as_path()));
    assert_eq!(
        second.plan_hash(),
        plan_hash,
        "a mid-Run edit must not change the frozen plan hash"
    );
    let responses = second
        .dispatch(HookRequest {
            event: HookEvent::Notification,
            run_id: run_id.clone(),
            tool_name: None,
            input: serde_json::json!({}),
        })
        .await;
    assert_eq!(
        responses.len(),
        2,
        "the frozen dispatcher still dispatches the pre-edit handler set"
    );
}

/// `failurePolicy` / `failure_policy` is read from `hooks.json`; unknown
/// or missing values stay fail-closed `Fail`.
#[test]
fn failure_policy_field_is_parsed_from_hooks_json() {
    let project = TempProject::new();
    project.write(
        ".natives/hooks.json",
        &format!(
            r#"{{"hooks":{{"Notification":[{{"hooks":[
                {{"type":"http","url":"{PROBE_URL}","failurePolicy":"skip"}},
                {{"type":"http","url":"{PROBE_URL}","failure_policy":"default"}},
                {{"type":"http","url":"{PROBE_URL}"}},
                {{"type":"http","url":"{PROBE_URL}","failurePolicy":"bogus"}}
            ]}}]}}}}"#
        ),
    );
    let policies: Vec<_> = file_definitions(&project)
        .iter()
        .map(|d| d.failure_policy)
        .collect();
    assert_eq!(
        policies,
        vec![
            HookFailurePolicy::Skip,
            HookFailurePolicy::Default,
            HookFailurePolicy::Fail,
            HookFailurePolicy::Fail,
        ]
    );
}

/// The frozen dispatcher resolves a handler failure through the Hook's
/// failure policy: Skip drops it, Default allows it, and a security event
/// stays fail-closed no matter what the policy says.
#[tokio::test]
async fn frozen_dispatcher_applies_failure_policy_skip_default_and_security_override() {
    let events = EventSequencer::memory_only();

    let frozen = freeze_run_hooks(
        "run-skip",
        "plan-skip",
        failing_registry(HookEvent::Notification, HookFailurePolicy::Skip),
        events.clone(),
    );
    let responses = frozen
        .dispatch(HookRequest {
            event: HookEvent::Notification,
            run_id: "run-skip".into(),
            tool_name: None,
            input: serde_json::json!({}),
        })
        .await;
    assert!(
        responses.is_empty(),
        "Skip must drop the failed hook: {responses:?}"
    );

    let frozen = freeze_run_hooks(
        "run-default",
        "plan-default",
        failing_registry(HookEvent::Notification, HookFailurePolicy::Default),
        events.clone(),
    );
    let responses = frozen
        .dispatch(HookRequest {
            event: HookEvent::Notification,
            run_id: "run-default".into(),
            tool_name: None,
            input: serde_json::json!({}),
        })
        .await;
    assert_eq!(responses.len(), 1);
    assert!(
        matches!(responses[0].decision, HookDecision::Allow),
        "Default must resolve a failure to Allow"
    );

    let frozen = freeze_run_hooks(
        "run-sec",
        "plan-sec",
        failing_registry(HookEvent::PreToolUse, HookFailurePolicy::Default),
        events.clone(),
    );
    let responses = frozen
        .dispatch(HookRequest {
            event: HookEvent::PreToolUse,
            run_id: "run-sec".into(),
            tool_name: Some("write_file".into()),
            input: serde_json::json!({}),
        })
        .await;
    assert!(
        matches!(responses[0].decision, HookDecision::Deny { .. }),
        "a security event must stay fail-closed even with Default policy"
    );
}
