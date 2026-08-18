//! Run-level frozen Hook Dispatcher (extracted from `production_hooks.rs`,
//! task-01 structure).
//!
//! One Run resolves and compiles its HookRegistry once at Run start; this
//! module freezes that registry as a shared read-only dispatcher keyed by
//! `run_id`, and pins the deterministic plan hash a Run's Hook plan runs
//! under.
//!
//! ## §19.5 plan-hash invariant
//!
//! The Snapshot, the Dispatcher, the Provider Prompt and the HookInvocation
//! trace **all** reference the same plan hash. In production this is the Run
//! snapshot's `effective_prompt_hash` — the SHA-256 the compiled Provider
//! prompt and the persisted snapshot already agree on — never the snapshot's
//! outer `canonical_hash`, which covers layers/hooks/tools the Hook plan does
//! not feed. When no snapshot is persisted (hermetic tests, pre-seam runs)
//! the dispatcher falls back to a deterministic hash of the compiled Hook
//! definitions, which is still a single value every downstream authority
//! receives through [`FrozenHookDispatcher::plan_hash`] / [`freeze_run_hooks`].

use super::*;

/// A Run's Hook plan, frozen at Run start and shared by every dispatch path.
///
/// One Run resolves and compiles its [`HookRegistry`] once; this value makes
/// that registry available, read-only, to every dispatch site in the process —
/// the engine loop, the permission gate, the notification hook, and the
/// subagent lifecycle hooks. A `hooks.json` edit made while the Run is in
/// flight never reaches this dispatcher, because discovery and resolution
/// happened at Run start. Mid-Run Hook modifications therefore only affect the
/// *next* Run, which resolves again and freezes a new plan.
///
/// Dispatch is a thin delegation to the underlying registry, which already
/// owns the `HookInvocationStarted` / `HookInvocationCompleted` telemetry
/// through the [`EventSequencer`] attached at freeze time. There is
/// deliberately no second execution engine and no second trace authority here:
/// the run event sequencer stays the single durable source for Hook traces.
#[derive(Clone)]
pub struct FrozenHookDispatcher {
    run_id: String,
    plan_hash: String,
    registry: Arc<HookRegistry>,
}

/// Keeps a Run's dispatcher registered until its starter reaches a terminal
/// result. Dropping it removes the bounded, run-scoped registry entry.
pub struct FrozenHookRunGuard {
    run_id: String,
}

impl Drop for FrozenHookRunGuard {
    fn drop(&mut self) {
        frozen_dispatchers()
            .lock()
            .expect("frozen hook dispatcher registry poisoned")
            .remove(&self.run_id);
    }
}

impl FrozenHookDispatcher {
    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    /// The frozen plan hash this dispatcher was bound to at Run start. In the
    /// production seam this is the Run snapshot's `effective_prompt_hash` —
    /// the same value the persisted snapshot, the compiled Provider prompt,
    /// and the HookInvocation trace metadata all reference (§19.5).
    pub fn plan_hash(&self) -> &str {
        &self.plan_hash
    }

    /// §19.5 plan-hash consistency guard. Asserts the frozen plan hash equals
    /// the `effective_prompt_hash` of the persisted Run snapshot, when one is
    /// persisted. Tests and pre-seam runs have no snapshot and skip the guard.
    ///
    /// The guard is a *runtime* assertion, not just a doc invariant: a drift
    /// between the Dispatcher plan hash and the Snapshot's effective prompt
    /// hash would mean the Hook trace is attributed to a plan the Provider
    /// never ran under, which §19.5 forbids.
    #[cfg(not(test))]
    pub fn assert_plan_hash_matches_snapshot(&self) {
        if let Some(snapshot) = crate::rpc::harness::repository::with_conn(|conn| {
            Ok(crate::rpc::harness::repository::get_run_snapshot(
                conn,
                &self.run_id,
            )?)
        })
        .ok()
        .flatten()
        {
            let expected = &snapshot.prompt_plan.effective_prompt_hash;
            // An empty effective_prompt_hash on a persisted snapshot means the
            // snapshot was stored before prompt compilation populated it; in
            // that case the frozen definitions-hash fallback is authoritative
            // and the guard is a no-op rather than a false alarm.
            if !expected.is_empty() {
                assert!(
                    expected == &self.plan_hash,
                    "§19.5 plan-hash invariant violated: frozen dispatcher plan_hash `{}` != snapshot effective_prompt_hash `{}` for run `{}`",
                    self.plan_hash, expected, self.run_id,
                );
            }
        }
    }

    /// Test builds never touch the harness store: the consistency guard is a
    /// no-op so hermetic tests do not depend on ambient DB env vars.
    #[cfg(test)]
    pub fn assert_plan_hash_matches_snapshot(&self) {}

    /// Every Hook in the frozen registry, canonical event order then dispatch
    /// order. This is the catalog the control plane renders, so what a user
    /// sees a Run "attached" is provably what it dispatches.
    pub fn describe(&self) -> Vec<HookDefinition> {
        self.registry.describe()
    }

    pub fn describe_event(&self, event: HookEvent) -> Vec<HookDefinition> {
        self.registry.describe_event(event)
    }

    pub fn events_covered(&self) -> Vec<HookEvent> {
        self.registry.events_covered()
    }

    pub fn fail_closed_security(&self) -> bool {
        self.registry.fail_closed_security
    }

    /// Retain this Run's frozen dispatcher through terminal settlement.
    pub fn retain_until_terminal(&self) -> FrozenHookRunGuard {
        FrozenHookRunGuard {
            run_id: self.run_id.clone(),
        }
    }

    /// Dispatch, resolving each Hook's failure through its
    /// [`HookFailurePolicy`]. Security-sensitive events stay fail-closed
    /// regardless of policy.
    ///
    /// HookInvocationStarted/Completed telemetry flows through the same
    /// [`EventSequencer`] the registry was frozen with — a single event source.
    pub async fn dispatch_outcomes(&self, request: HookRequest) -> Vec<HookOutcome> {
        self.registry.dispatch_outcomes(request).await
    }

    pub async fn dispatch(&self, request: HookRequest) -> Vec<HookResponse> {
        self.registry.dispatch(request).await
    }

    pub async fn observe(&self, request: HookRequest) -> Result<(), agent_core::ObserveResult> {
        self.registry.observe(request).await
    }

    /// Fold Hook outcomes into a single permission verdict, fail-closed.
    pub fn aggregate_permission(outcomes: &[HookOutcome]) -> agent_core::PermissionAggregate {
        HookRegistry::aggregate_permission(outcomes)
    }

    /// Aggregate allow/deny for a subagent gate: any Deny wins.
    pub fn aggregate_allow(responses: &[HookResponse]) -> Result<(), String> {
        HookRegistry::aggregate_allow(responses)
    }
}

/// Process-wide, Run-scoped frozen dispatchers, keyed by `run_id`.
///
/// Two writers converge on this table: the Run-start seam
/// ([`freeze_run_hooks`]) and the lazy fallback ([`resolve_frozen_dispatcher`]).
/// Insertion is idempotent — the first registration for a `run_id` wins and
/// later calls are no-ops — which is exactly what makes a dispatcher frozen:
/// a mid-Run `hooks.json` change can never replace it.
fn frozen_dispatchers() -> &'static Mutex<HashMap<String, FrozenHookDispatcher>> {
    static REGISTRY: OnceLock<Mutex<HashMap<String, FrozenHookDispatcher>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Freeze a Run's compiled registry as its shared read-only dispatcher.
///
/// Idempotent per `run_id`: the first freeze wins, so a second call with a
/// registry compiled from a mid-Run configuration change is ignored.
///
/// `events` is attached to the registry at freeze time so every dispatch from
/// the tool paths emits HookInvocation telemetry through the same
/// [`EventSequencer`] the engine uses.
pub fn freeze_run_hooks(
    run_id: &str,
    plan_hash: &str,
    registry: HookRegistry,
    events: EventSequencer,
) -> FrozenHookDispatcher {
    let mut map = frozen_dispatchers()
        .lock()
        .expect("frozen hook dispatcher registry poisoned");
    if let Some(existing) = map.get(run_id) {
        return existing.clone();
    }
    let frozen = FrozenHookDispatcher {
        run_id: run_id.to_string(),
        plan_hash: plan_hash.to_string(),
        registry: Arc::new(registry.with_events(events)),
    };
    map.insert(run_id.to_string(), frozen.clone());
    frozen
}

/// The Run's frozen dispatcher, when the Run has been frozen.
pub fn frozen_dispatcher_for_run(run_id: &str) -> Option<FrozenHookDispatcher> {
    frozen_dispatchers()
        .lock()
        .ok()
        .and_then(|map| map.get(run_id).cloned())
}

/// Resolve the Run's frozen dispatcher, compiling once when it is not frozen.
///
/// This is the seam the permission, notification, and subagent tool paths
/// call. It is the single source of truth for the whole process: the first
/// call for a `run_id` compiles the HookRegistry (discovery + resolution)
/// exactly once and caches it; every later call — however many tool
/// invocations, subagent spawns, or notifications — reuses that same frozen
/// registry and never re-scans `hooks.json`.
///
/// The plan hash prefers the Run snapshot's `effective_prompt_hash`, so the
/// Dispatcher, the Snapshot, the Provider Prompt and the HookInvocation trace
/// all agree on the same plan (§19.5). When no snapshot is persisted (hermetic
/// tests, pre-seam runs) it falls back to a deterministic hash of the compiled
/// definitions, which is still a single value every downstream authority
/// receives through [`FrozenHookDispatcher::plan_hash`].
pub fn resolve_frozen_dispatcher(
    run_id: &str,
    events: EventSequencer,
    project: Option<&Path>,
) -> FrozenHookDispatcher {
    if let Some(existing) = frozen_dispatcher_for_run(run_id) {
        return existing;
    }
    let definitions = discover_production_hooks(project);
    let plan_hash =
        snapshot_plan_hash_for_run(run_id).unwrap_or_else(|| definitions_plan_hash(&definitions));
    let registry = compile_production_hooks(&definitions, project);
    let frozen = freeze_run_hooks(run_id, &plan_hash, registry, events);
    // §19.5 plan-hash invariant guard: when a snapshot is persisted, the frozen
    // dispatcher's plan hash must equal the snapshot's effective_prompt_hash.
    // The guard is a runtime assertion (no-op in test builds and when the
    // snapshot has no populated effective_prompt_hash).
    frozen.assert_plan_hash_matches_snapshot();
    frozen
}

/// Resolve the Run's frozen dispatcher for the SubagentStart / SubagentStop
/// lifecycle hooks, **without** re-scanning `hooks.json` on every spawn or
/// terminal settlement.
///
/// This is the seam the subagent tool paths should call instead of
/// [`build_production_hooks_for_project`]: it shares the same Run-scoped frozen
/// registry the permission gate and the notification hook already use, so a
/// mid-Run `hooks.json` edit only reaches the *next* Run (§19.5). A subagent
/// spawn that fires SubagentStart and a terminal settlement that fires
/// SubagentStop both dispatch through the registry frozen at Run start, and
/// both emit HookInvocationStarted / Completed telemetry through the same
/// [`EventSequencer`] the engine uses.
///
/// `parent_run_id` is the Run whose frozen plan governs the subagent
/// lifecycle; `project` is the project root for the lazy compile fallback.
pub fn resolve_frozen_dispatcher_for_subagent(
    parent_run_id: &str,
    events: EventSequencer,
    project: Option<&Path>,
) -> FrozenHookDispatcher {
    // Same compilation, same cache, same idempotent freeze. The subagent
    // paths get the same read-only dispatcher the permission gate got.
    resolve_frozen_dispatcher(parent_run_id, events, project)
}

/// Best-effort plan hash from the Run's persisted Harness snapshot.
///
/// Returns the snapshot's **`effective_prompt_hash`** — the SHA-256 of the
/// compiled Provider prompt — not the outer `canonical_hash`, so the
/// Dispatcher, the Snapshot, the Provider Prompt and the HookInvocation trace
/// all reference the same plan (§19.5). The outer canonical hash covers
/// layers/hooks/tools the Hook plan does not feed and would diverge from the
/// Provider's own effective hash; using it was the first-wave bug this fixes.
///
/// Any persistence or DB failure degrades to `None`; the caller then uses the
/// deterministic definitions hash instead.
///
/// Test builds never touch the harness store: hermetic tests have no snapshot
/// row and must not depend on ambient DB env vars or the store lock.
#[cfg(not(test))]
fn snapshot_plan_hash_for_run(run_id: &str) -> Option<String> {
    crate::rpc::harness::repository::with_conn(|conn| {
        Ok(
            crate::rpc::harness::repository::get_run_snapshot(conn, run_id)?
                .map(|snapshot| snapshot.prompt_plan.effective_prompt_hash.clone()),
        )
    })
    .ok()
    .flatten()
}

#[cfg(test)]
fn snapshot_plan_hash_for_run(_run_id: &str) -> Option<String> {
    None
}

/// Deterministic plan hash for a definition set — the lazy fallback used when
/// no Run snapshot is persisted.
fn definitions_plan_hash(definitions: &[HookDefinition]) -> String {
    harness_core::sha256_hex(&serde_json::to_string(definitions).unwrap_or_default())
}

/// §19.5 frozen-hook-runtime tests: cross-Run plan invariance, trace
/// completeness, plan-hash consistency, and fail-closed security.
#[cfg(test)]
mod frozen_hook_runtime_tests {
    use super::*;

    /// A project temp-dir with a hooks.json probe the loopback URL so
    /// `http` hooks are rejected fast (validate_http_hook_url) — a hermetic
    /// handler-counting probe with no sockets or processes.
    struct TempProject {
        root: std::path::PathBuf,
    }
    impl TempProject {
        fn new() -> Self {
            let root = std::env::temp_dir()
                .join(format!("natives-frozen-runtime-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&root).unwrap();
            Self { root }
        }
        fn write(&self, rel: &str, body: &str) -> &Self {
            let path = self.root.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, body).unwrap();
            self
        }
        fn root(&self) -> &std::path::Path {
            &self.root
        }
    }
    impl Drop for TempProject {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    const PROBE_URL: &str = "http://127.0.0.1:1/hook";

    fn probe_group(event: &str, url: &str) -> String {
        format!(r#"{{"hooks":{{"{event}":[{{"hooks":[{{"type":"http","url":"{url}"}}]}}]}}}}"#)
    }

    /// §19.5 cross-Run plan invariance: a Run freezes its plan at start, and a
    /// mid-Run `hooks.json` edit never reaches the in-flight Run — only the
    /// *next* Run sees the edited plan. This is the freeze immutability test
    /// from the other direction: two different runs get two different plans
    /// and one run's edit cannot stomp the other.
    #[tokio::test]
    async fn mid_run_hook_edit_does_not_affect_an_already_frozen_run() {
        let project = TempProject::new();
        project.write(
            ".natives/hooks.json",
            &probe_group("Notification", PROBE_URL),
        );
        let events = EventSequencer::memory_only();

        // Run A freezes its plan.
        let run_a = format!("run-a-{}", uuid::Uuid::new_v4());
        let frozen_a = resolve_frozen_dispatcher(&run_a, events.clone(), Some(project.root()));
        let plan_a = frozen_a.plan_hash().to_string();
        let responses_a = frozen_a
            .dispatch(agent_core::HookRequest {
                event: HookEvent::Notification,
                run_id: run_a.clone(),
                tool_name: None,
                input: serde_json::json!({}),
            })
            .await;
        // allow-all builtin + loopback-rejected probe.
        assert_eq!(
            responses_a.len(),
            2,
            "run A must dispatch its pre-edit handler set"
        );

        // Mid-Run edit: double the probe hooks.
        project.write(
            ".natives/hooks.json",
            &format!(
                r#"{{"hooks":{{"Notification":[{{"hooks":[
                    {{"type":"http","url":"{PROBE_URL}"}},
                    {{"type":"http","url":"{PROBE_URL}"}}
                ]}}]}}}}"#
            ),
        );

        // Run A must still dispatch the pre-edit plan — the freeze is immutable.
        let frozen_a_again =
            resolve_frozen_dispatcher(&run_a, events.clone(), Some(project.root()));
        assert_eq!(
            frozen_a_again.plan_hash(),
            plan_a,
            "a mid-Run edit must not change an already-frozen run's plan hash"
        );
        let responses_a_again = frozen_a_again
            .dispatch(agent_core::HookRequest {
                event: HookEvent::Notification,
                run_id: run_a.clone(),
                tool_name: None,
                input: serde_json::json!({}),
            })
            .await;
        assert_eq!(
            responses_a_again.len(),
            2,
            "the already-frozen run must still dispatch its pre-edit handler set"
        );

        // Run B starts *after* the edit and gets the new plan.
        let run_b = format!("run-b-{}", uuid::Uuid::new_v4());
        let frozen_b = resolve_frozen_dispatcher(&run_b, events, Some(project.root()));
        assert_ne!(
            frozen_b.plan_hash(),
            plan_a,
            "a new run started after the edit must get a new plan hash"
        );
        let responses_b = frozen_b
            .dispatch(agent_core::HookRequest {
                event: HookEvent::Notification,
                run_id: run_b.clone(),
                tool_name: None,
                input: serde_json::json!({}),
            })
            .await;
        // allow-all builtin + two loopback-rejected probes.
        assert_eq!(
            responses_b.len(),
            3,
            "the new run must dispatch the post-edit handler set"
        );
    }

    /// §19.5 trace completeness: every dispatched Hook emits a Started/Completed
    /// pair through the same EventSequencer, and every Started has a Completed
    /// twin with the same invocation_id. There are no dangling Started events.
    #[tokio::test]
    async fn trace_completeness_started_and_completed_pairs_exist() {
        let project = TempProject::new();
        project.write(
            ".natives/hooks.json",
            &probe_group("Notification", PROBE_URL),
        );
        let events = EventSequencer::memory_only();
        let run_id = format!("run-trace-{}", uuid::Uuid::new_v4());
        let frozen = freeze_run_hooks(
            &run_id,
            "plan-trace-completeness",
            build_production_hooks_for_project(Some(project.root())),
            events.clone(),
        );

        let responses = frozen
            .dispatch(agent_core::HookRequest {
                event: HookEvent::Notification,
                run_id: run_id.clone(),
                tool_name: None,
                input: serde_json::json!({}),
            })
            .await;
        let dispatched = responses.len();
        assert!(dispatched > 0, "at least one hook must dispatch");

        let replayed = events.replay_after(&run_id, 0);
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
            dispatched,
            "every dispatched hook must emit HookInvocationStarted"
        );
        assert_eq!(
            completed.len(),
            dispatched,
            "every dispatched hook must emit HookInvocationCompleted"
        );

        // Every Started must have a Completed twin with the same invocation_id.
        for start in &started {
            let started_payload = match &start.payload {
                RunEventKind::HookInvocationStarted { invocation_id, .. } => invocation_id.clone(),
                _ => continue,
            };
            let has_twin = completed.iter().any(|c| match &c.payload {
                RunEventKind::HookInvocationCompleted { invocation_id, .. } => {
                    *invocation_id == started_payload
                }
                _ => false,
            });
            assert!(
                has_twin,
                "started invocation {started_payload} must have a completed twin"
            );
        }
    }

    /// §19.5 plan-hash consistency: the Dispatcher's plan_hash, the
    /// HookInvocation trace's plan_hash, and the value passed to freeze_run_hooks
    /// are all the same string. The trace mapping receives the dispatcher's
    /// plan_hash and the round-trip must preserve it.
    #[tokio::test]
    async fn plan_hash_is_consistent_between_dispatcher_and_trace() {
        let project = TempProject::new();
        project.write(
            ".natives/hooks.json",
            &probe_group("Notification", PROBE_URL),
        );
        let events = EventSequencer::memory_only();
        let plan_hash = "plan-consistency-12345";
        let run_id = format!("run-consistency-{}", uuid::Uuid::new_v4());
        let frozen = freeze_run_hooks(
            &run_id,
            plan_hash,
            build_production_hooks_for_project(Some(project.root())),
            events.clone(),
        );

        // Dispatcher plan_hash == freeze value.
        assert_eq!(frozen.plan_hash(), plan_hash);

        let _responses = frozen
            .dispatch(agent_core::HookRequest {
                event: HookEvent::Notification,
                run_id: run_id.clone(),
                tool_name: None,
                input: serde_json::json!({}),
            })
            .await;

        // Trace plan_hash == dispatcher plan_hash.
        let replayed = events.replay_after(&run_id, 0);
        for event in &replayed {
            if matches!(&event.payload, RunEventKind::HookInvocationStarted { .. }) {
                let trace = trace_from_started_event(event, frozen.plan_hash())
                    .expect("started event must map to a trace");
                assert_eq!(
                    trace.plan_hash, plan_hash,
                    "trace plan_hash must equal the dispatcher's frozen plan_hash"
                );
            }
            if matches!(&event.payload, RunEventKind::HookInvocationCompleted { .. }) {
                let trace = trace_from_completed_event(event, frozen.plan_hash())
                    .expect("completed event must map to a trace");
                assert_eq!(
                    trace.plan_hash, plan_hash,
                    "trace plan_hash must equal the dispatcher's frozen plan_hash"
                );
            }
        }
    }

    /// §19.5 fail-closed: a security-sensitive event (PreToolUse) with no
    /// matching handler in the frozen registry must deny, not silently allow.
    /// The freeze does not weaken the fail-closed security override.
    #[tokio::test]
    async fn frozen_dispatcher_fail_closed_on_security_event_with_no_handler() {
        let events = EventSequencer::memory_only();
        let mut registry = agent_core::HookRegistry::new();
        registry.enable_security_fail_closed();
        // No handler registered for PreToolUse.
        let frozen = freeze_run_hooks("run-fail-closed", "plan-fail-closed", registry, events);
        let outcomes = frozen
            .dispatch_outcomes(agent_core::HookRequest {
                event: HookEvent::PreToolUse,
                run_id: "run-fail-closed".into(),
                tool_name: Some("write_file".into()),
                input: serde_json::json!({}),
            })
            .await;
        assert_eq!(
            outcomes.len(),
            1,
            "a security event with no handler must produce a fail-closed outcome"
        );
        assert!(
            matches!(
                &outcomes[0],
                HookOutcome::Decided(HookResponse {
                    decision: HookDecision::Deny { .. }
                })
            ),
            "the fail-closed outcome must be a Deny, got {:?}",
            outcomes[0]
        );
    }
}
