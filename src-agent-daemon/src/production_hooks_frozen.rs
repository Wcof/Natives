//! Run-level frozen Hook Dispatcher (extracted from `production_hooks.rs`,
//! task-01 structure).
//!
//! One Run resolves and compiles its HookRegistry once at Run start; this
//! module freezes that registry as a shared read-only dispatcher keyed by
//! `run_id`, and pins the deterministic plan hash a Run's Hook plan runs
//! under.

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

impl FrozenHookDispatcher {
    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    /// The frozen plan hash this dispatcher was bound to at Run start. In the
    /// production seam this is the Run snapshot's canonical hash — the same
    /// value the persisted snapshot, the compiled Provider prompt, and the
    /// HookInvocation trace metadata all reference.
    pub fn plan_hash(&self) -> &str {
        &self.plan_hash
    }

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
/// This is the seam the permission and notification tool paths call. It is the
/// single source of truth for the whole process: the first call for a `run_id`
/// compiles the HookRegistry (discovery + resolution) exactly once and caches
/// it; every later call — however many tool invocations, subagent spawns, or
/// notifications — reuses that same frozen registry and never re-scans
/// `hooks.json`.
///
/// The plan hash prefers the Run snapshot's canonical hash, so the Dispatcher,
/// the Snapshot, the Provider Prompt and the HookInvocation trace all agree on
/// the same plan. When no snapshot is persisted (hermetic tests, pre-seam
/// runs) it falls back to a deterministic hash of the compiled definitions.
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
    freeze_run_hooks(run_id, &plan_hash, registry, events)
}

/// Best-effort canonical plan hash from the Run's persisted Harness snapshot.
///
/// Same source as `harness.run.getSnapshot`: the snapshot's canonical hash.
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
                .map(|snapshot| snapshot.canonical_hash()),
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
