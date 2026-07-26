# Native Harness Control Plane Design

- Status: Approved for planning
- Date: 2026-07-26
- Scope: Native execution engine only
- Settings entry: Settings → Execution Engine → Native Engine

## 1. Summary

Natives will extract the current session coordination and Hook runtime into an
independently managed Harness module. The module will provide a visual control
plane for:

- the fixed Native execution topology;
- Session queue, interjection, safe-point, and drain state;
- Hook definitions, provenance, ordering, policy, and invocation traces;
- versioned Blueprint configuration;
- immutable per-Run Harness snapshots;
- Live Runs and Audit projections.

The selected design is a deep `HarnessControlPlane` module. It publishes a
small interface and hides profile inheritance, versioning, Hook discovery,
Session Actor state, plan compilation, telemetry, and audit projection.

The execution state machine remains fixed. The UI may configure only
schema-approved policy slots. It must not become a free-form workflow builder.

## 2. Existing State

The current Native path already contains real Harness behavior:

- `crates/agent-core/src/session_coordinator.rs` owns per-conversation queue,
  interjection, permission interaction, cancel-and-send, and terminal drain.
- `src-agent-daemon/src/prompt_queue_store.rs` persists queue and actor state.
- `AgentEngine` receives an optional global `SessionCoordinator` and checks it
  at safe points.
- `HookRegistry` stores opaque `HookHandler` trait objects keyed by
  `HookEvent`.
- `production_hooks.rs` discovers built-in, project, user, environment,
  command, and HTTP hooks and rebuilds a registry for each Run.
- `RuntimePanel` mixes Runtime selection, Native settings, CLI status,
  capabilities, protection settings, and Job scheduling in one page.

This works at runtime, but it lacks:

- an inspectable Hook identity and provenance model;
- versioned Harness configuration;
- immutable evidence of the effective Harness used by a Run;
- a management RPC surface;
- a unified execution topology;
- a Settings control center;
- a clean distinction between fixed stages, safe points, and Hook points.

### 2.1 Three competing Hook models (resolved in Phase 1)

The first draft of this section described only the `agent-core` Hook runtime.
Implementation found **three** Hook models in the tree, two of them unreachable:

| | `agent-core` (live) | `src-tauri/runtime/native` (orphan) | `assistant-protocol` v1 (orphan) |
|---|---|---|---|
| Event enum | `HookEvent` ×16 | `HookPoint` ×9 | `HookPoint` ×10, different naming |
| Hook identity | none | none | `id` |
| Ordering | implicit registration order | implicit registration order | `priority` |
| Failure policy | hard-coded fail-closed | non-zero exit **allowed** | `Fail`/`Skip`/`Default` |
| Tool matcher | yes | yes, plus `*suffix` | none |
| Typed conditions | none | string form `Bash(git commit:*)` | none |
| Self-description | none | `HookInfo` | is data by construction |
| Script protocol | strict: async, timeout, IO cap, non-zero denies | same protocol, lax on failure | none |

`src-tauri/src/runtime/native/` (~5.4k lines: `AgentLoop`, `CapabilityRegistry`,
`HookPipeline`, `RuleEngine`, `plugin_system`, `subagent`) was never registered
in `runtime::registry` and had no caller outside itself. A module-level
`#![allow(dead_code, unused_imports, unused_variables)]` had kept it warning-free.
`assistant-protocol`'s `HookRegistration`/`HookPoint`/`HookFailStrategy` had no
production reference and no client speaking their wire form.

Resolution:

- `agent-core` remains the **only** Hook execution authority. Its script handling
  is the strictest of the three and was left untouched.
- The data-model concepts the orphans alone carried were absorbed into
  `harness-core`: stable identity, explicit ordering, explicit failure policy,
  self-description, and typed conditions.
- `RuleEngine`'s typed `ConditionOperator` (`regex_match`, `contains`,
  `not_contains`, `equals`, `path_match`) was preferred over the orphan
  `Bash(git commit:*)` expression string, because a typed operator renders as a
  dropdown plus a field while an expression renders only as a text box — and
  rendering is the point of this project. Its `.local.md` rule loading and
  `RuleAction` were **not** absorbed; rules are not Hooks, and reviving them
  needs its own ADR.
- Both orphan trees were deleted.

Known remainder, deliberately out of scope: `assistant-protocol`'s
`SkillManifest` is also unreferenced, and `AgentRuntime`'s execution surface
(`stream`/`interrupt`/`dispose`) has no caller now that CLI runtimes are used
only for capability metadata. Neither is a competing Hook model; both belong to
the Phase 5 cleanup.

## 3. Goals

### 3.1 Product goals

1. Make the full Native execution topology visible.
2. Show exactly which stages have Hook points and which Hooks are attached.
3. Show which Hooks ran, were unmatched, modified data, denied execution,
   failed, or timed out.
4. Allow safe Hook configuration for future Runs.
5. Support global templates, project overlays, and session selection.
6. Support Draft → Validate → Diff → Publish → Rollback.
7. Bind every Run to an immutable, persisted Harness snapshot.
8. Aggregate all Native-owned execution settings under Native Engine.
9. Preserve external CLI Harness ownership.

### 3.2 Engineering goals

1. Replace scattered Harness state with a deep module and a small interface.
2. Preserve a single authority for Run lifecycle, engine execution, tool
   safety, events, and persistence.
3. Migrate incrementally from current behavior without a big-bang rewrite.
4. Keep execution and configuration behavior independently testable.
5. Keep runtime data growth, subscriptions, and Renderer work bounded.

## 4. Non-goals

The MVP will not:

- provide a free-form drag-and-drop workflow engine;
- allow live Hook enable/disable, reorder, or replacement in an active Run;
- allow users to delete or reconnect fixed engine stages;
- make Provider routing, permission, context, subagent, or concurrency policy
  editable;
- edit Claude CLI or Codex CLI Harness configuration;
- change Job scheduling or decide the Job module's future UI;
- move assistant conversation state into Harness;
- move Provider assets or credentials into Harness;
- introduce a second Run lifecycle or event authority.

## 5. Hard Compatibility Constraints

This work modifies the Native execution engine. It must not alter the existing
Assistant or Provider integration contracts.

### 5.1 Assistant integration

- Existing Assistant calls to `run.start`, conversation methods, queue
  methods, permission methods, and event subscription remain valid.
- `run.start` gains no new required field.
- `runtime_id = native` keeps its current meaning.
- Harness profile and session selection are managed through separate
  `harness.*` methods and resolved inside the Daemon.
- Existing Run lifecycle and message events retain their current semantics.
- New Harness events are additive protocol events.
- Protocol decoders retain an unknown-event passthrough so Assistant views
  that do not understand Harness events can safely ignore them.
- No Assistant reducer becomes an authority for Harness configuration.

### 5.2 Provider integration

- `EngineProvider` remains unchanged.
- `RoutedProvider`, provider selection, model selection, fallback, credential
  leasing, streaming, and usage reporting remain unchanged in the MVP.
- Provider configuration is displayed as read-only topology metadata.
- `HarnessControlPlane` does not read or write Provider asset databases.
- Provider credentials never enter Harness Blueprint, snapshots, events, or
  Renderer state.

### 5.3 Stable insertion seam

Harness resolution occurs inside the Daemon after the existing `run.start`
validation and before `ProductionRuntime` creates `AgentEngine`.

```text
Assistant run.start
  → existing RunManager validation
  → HarnessControlPlane.resolve_run
  → persist ResolvedHarnessSnapshot
  → existing Provider routing and credential lease
  → existing ProductionRuntime / AgentEngine path
```

The default migrated Blueprint must compile to behavior equivalent to the
current production path before any setting becomes editable.

## 6. Product Information Architecture

The Settings sidebar retains one Execution Engine entry.

```text
Settings
└── Execution Engine
    ├── Runtime Overview
    ├── Native Engine
    │   ├── Overview
    │   ├── Blueprint
    │   ├── Hooks
    │   ├── Live Runs
    │   └── Audit
    ├── Claude CLI (read-only ownership view)
    └── Codex CLI (read-only ownership view)
```

Native Engine aggregates:

- Native capabilities;
- tools and protection;
- permissions;
- context and compaction;
- self-heal and doom-loop protection;
- Engine Capabilities;
- Harness topology;
- Hooks;
- Live Runs;
- Audit.

The existing standalone Engine Capabilities Settings section is folded into
Native Engine.

Job scheduling is removed from `RuntimePanel` and excluded from this project.
Jobs are an independent module and are not owned by the Native execution
engine.

The Native Engine workspace may use a wider layout than ordinary Settings
forms. Blueprint and Live Runs require a three-pane layout and must not be
constrained to the current 920px content width.

## 7. Module Architecture

### 7.1 Pure Harness module

Create `crates/harness-core/` as the pure domain and runtime coordination
module. It must not depend on Daemon RPC, SQLite, Renderer, Host, Provider
adapters, or platform process implementations.

Suggested internal modules:

```text
crates/harness-core/src/
├── lib.rs
├── blueprint.rs
├── profile.rs
├── validation.rs
├── resolver.rs
├── snapshot.rs
├── topology.rs
├── session_actor.rs
├── run_harness.rs
├── hooks/
│   ├── mod.rs
│   ├── definition.rs
│   ├── binding.rs
│   ├── decision.rs
│   └── dispatch.rs
└── telemetry.rs
```

Responsibilities:

- typed Blueprint schema;
- scope precedence and overlay resolution;
- canonical serialization and hashing;
- validation and diff models;
- immutable snapshot model;
- fixed Native topology model;
- current `SessionCoordinator` behavior;
- Hook identity, ordering, matching, decision aggregation, and trace models;
- per-Run `RunHarness` interface.

### 7.2 Daemon production module

Create `src-agent-daemon/src/harness/` for production adapters and assembly.

```text
src-agent-daemon/src/harness/
├── mod.rs
├── control_plane.rs
├── repository.rs
├── migrations.rs
├── source_discovery.rs
├── source_manifest.rs
├── hook_adapters.rs
├── telemetry_sink.rs
├── audit_projection.rs
├── rpc.rs
└── compatibility.rs
```

Responsibilities:

- `HarnessControlPlane` production implementation;
- SQLite persistence in Daemon-owned `assistant.db`;
- Hook file and environment discovery;
- command and HTTP Hook adapters;
- source drift detection;
- protocol v2 handlers;
- Run event persistence;
- startup recovery;
- temporary compatibility adapters during migration.

Phase 2 landed `mod.rs`, `repository.rs`, `control_plane.rs`, and
`projection.rs`; `source_discovery` and `source_manifest` stay folded into the
existing `production_hooks.rs` two-stage discovery, and `telemetry_sink` /
`audit_projection` belong to Phase 3. The `mod` declaration currently sits in
`rpc.rs` behind `#[path = "harness/mod.rs"]`, so the module path is
`crate::rpc::harness` while the files are where this section puts them.
That is a boundary artefact of parallel work on `lib.rs`, not a design
position: promoting it is one line in `lib.rs` plus a rename at its call sites.

### 7.3 Existing authority remains

| Module | Authority retained |
|---|---|
| `RunManager` | Run create/start/cancel/retry/terminal lifecycle |
| `AgentEngine` | Fixed single-Run execution loop |
| `CapabilityGateway` | Schema, path scope, permission, execution, audit |
| `conversation_store` | Assistant conversation and message state |
| Provider modules | Provider assets, routing, credentials, streaming |
| `run_events` | Runtime event authority |
| `HarnessControlPlane` | Harness configuration, resolution, snapshot, topology, Hook metadata |

## 8. Harness Interfaces

`HarnessControlPlane` exposes a small conceptual interface:

```rust
trait HarnessControlPlane {
    fn validate(&self, draft: HarnessDraft) -> ValidationReport;
    fn publish(&self, request: PublishRequest) -> PublishedVersion;
    fn resolve_run(&self, context: RunHarnessContext) -> ResolvedHarnessSnapshot;
    fn inspect(&self, query: HarnessInspectionQuery) -> HarnessInspection;
}
```

Persistence, source discovery, Hook adapters, and audit projection are internal
seams. Callers must not orchestrate them.

`RunHarness` is the execution-facing interface:

```rust
struct RunHarness {
    snapshot: ResolvedHarnessSnapshot,
    session: SessionActorHandle,
    hooks: CompiledHookRuntime,
    telemetry: HarnessTelemetrySink,
}
```

It provides:

- safe-point coordination;
- Hook dispatch;
- stable snapshot identity;
- structured stage and Hook telemetry.

`AgentEngine` must receive a valid `RunHarness`. The current optional
`session_harness` field and global lookup are removed after migration.

## 9. Configuration and Version Model

### 9.1 Resolution precedence

Effective configuration is resolved in this order:

1. locked built-in topology and safety invariants;
2. published global template;
3. optional published project overlay;
4. optional session selection of a published overlay;
5. runtime capability validation.

Overlays are typed and sparse. They are not arbitrary JSON merge patches.
Unknown fields fail validation. Locked fields cannot be overridden.

Session selection references a published profile/version. It does not create an
untracked session-local configuration copy.

### 9.2 Publishing

```text
Draft
  → schema and invariant validation
  → source and trust validation
  → diff against current published version
  → atomic publish
  → immutable version + canonical hash
```

Publishing uses optimistic concurrency through a draft `revision`. A stale
revision returns a conflict and must not overwrite another editor.

Rollback republishes the selected old document as a new immutable version. It
does not move a mutable "current version" pointer backward without history.

### 9.3 Run resolution

At Run start:

1. resolve exact published layer versions;
2. discover and validate runtime capabilities;
3. compile Hook bindings;
4. canonicalize the effective document;
5. compute its SHA-256 hash;
6. persist `harness_run_snapshot`;
7. construct `RunHarness`;
8. start the existing Provider and Engine path.

Failure before snapshot persistence means the Run does not execute.

## 10. Persistence Model

All tables live in Daemon-owned `assistant.db`. Migrations are incremental,
WAL remains enabled, and foreign keys declare cascade behavior.

### `harness_profile`

Logical profile or overlay identity:

- `id`
- `name`
- `description`
- `kind` (`global_template`, `project_overlay`, `session_overlay`)
- `project_id` when scoped
- `current_published_version_id`
- timestamps

### `harness_draft`

Editable state:

- `profile_id`
- `base_version_id`
- `document_json`
- `revision`
- `updated_at`

### `harness_version`

Immutable published state:

- `id`
- `profile_id`
- monotonically increasing `version_number`
- `parent_version_id`
- `document_json`
- `canonical_hash`
- `source_manifest_json`
- `validation_summary_json`
- `created_at`

### `harness_binding`

Scope selection:

- `scope_type` (`global`, `project`, `session`)
- `scope_id`
- `profile_id`
- optional pinned `version_id`
- binding mode (`follow_published`, `pinned`)
- `updated_at`

### `harness_run_snapshot`

Execution evidence:

- `run_id`
- resolved layer/version references
- `snapshot_json`
- `canonical_hash`
- `topology_version`
- `hook_semantics_version`
- `resolved_at`

### `harness_audit`

Configuration audit:

- publish;
- rollback;
- binding change;
- source drift acknowledgement;
- actor and timestamp;
- redacted change summary.

Runtime Hook traces do not use this table. They remain in `run_events`.

## 11. Native Execution Topology

The topology is versioned but fixed by code:

| Stage | Hook points | Safe points |
|---|---|---|
| Session | `SessionStart`, `UserPromptSubmit` | — |
| Context | — in MVP | — |
| Provider | — in MVP | `ProviderBatchBoundary` |
| Tool Gate | `PreToolUse` | `BeforeTool` |
| Permission | `PermissionRequest`, `PermissionDenied` | `AfterPermissionResolved` |
| Tool Execute | `PostToolUse`, `PostToolUseFailure` | `AfterTool` |
| Subagent | `SubagentStart`, `SubagentStop` | — |
| Compact | `PreCompact`, `PostCompact` | — |
| Stop | `Stop`, `StopFailure` | — |
| Terminal | `SessionEnd`, `Error` | — |
| Cross-stage | `Notification` | — |

The UI distinguishes:

- **Stage**: fixed Engine execution phase;
- **Safe Point**: point where Session queue/interjection coordination is safe;
- **Hook Point**: point where Hook definitions may execute.

Users cannot delete, reorder, or reconnect stages.

## 12. Hook Model

### 12.1 Hook definition

Every Hook has inspectable metadata:

- stable `id`;
- display name;
- Hook event;
- kind (`builtin`, `command`, `http`, future adapter kind);
- source kind (`built_in`, `native`, `imported`, `environment`);
- source URI/path and source digest;
- matcher;
- order;
- enabled state;
- timeout;
- trust state;
- failure mode;
- locked state;
- redacted adapter configuration.

### 12.2 Source ownership

- Built-in safety Hooks are locked.
- Natives Hooks are editable and versioned in Blueprint.
- `.claude`, `.agents`, `.grok`, and `.natives` source files remain read-only.
- A Profile may store enable, order, matcher, timeout, and failure-policy
  overlays without rewriting the source file.
- Source content is materialized into the published version's source manifest.
- Later file changes create Source Drift and a draft candidate. They do not
  mutate an active published version.
- Legacy environment Hooks are represented as read-only sources during
  migration, materialized into the resolved snapshot, and never hidden.

Claude CLI and Codex CLI Harness configuration is read-only in Natives. The UI
states which external Runtime owns it and how the user can edit it externally.

### 12.3 Deterministic execution

Hooks execute sequentially by:

1. locked priority band;
2. configured order;
3. stable Hook ID as a tie-breaker.

Decision semantics:

- each Hook receives the payload produced by the previous Hook;
- `Deny` terminates the Hook point;
- `Modify` updates the payload for subsequent Hooks;
- `Inject` appends messages in execution order;
- `Rewake` is aggregated with logical OR;
- security Hook failures and timeouts are fail closed;
- ordinary Hook failure policy is explicit in the published version.

An active Run uses only its compiled Hook bindings. Publishing, source drift,
or UI edits cannot change them.

### 12.4 Hook semantics compatibility

`ResolvedHarnessSnapshot` records a non-editable `hook_semantics_version`.

- Migrated compatibility profiles initially use `legacy_v1`, which preserves
  the current Hook dispatch and aggregation behavior.
- The approved sequential behavior is `sequential_v2`.
- Extraction, metadata introduction, and a semantics upgrade must not land as
  one untestable change.
- Moving a profile from `legacy_v1` to `sequential_v2` requires validation,
  an explicit published version, and a visible diff.
- Active Runs never change semantics version.

New Natives-owned profiles use `sequential_v2`. Automatically migrated
profiles remain on `legacy_v1` until the parity suite passes and the migration
is explicitly published.

## 13. Event and Audit Model

Runtime events remain single-authority `run_events`. Additive events:

- `HarnessPlanResolved`
- `HarnessStageStarted`
- `HarnessStageCompleted`
- `HookDispatchStarted`
- `HookInvocationStarted`
- `HookInvocationCompleted`
- `HookDispatchCompleted`

Unmatched Hooks are derived by comparing the immutable snapshot and dispatch
input. They do not each create a persistent event, preventing event explosion.

Every Hook event contains:

- run and Hook identity;
- Hook event and source;
- status and decision;
- duration;
- redacted input/output summary;
- structured error category;
- truncation metadata.

Event payloads are redacted before persistence. Credentials, leased keys,
environment secrets, and sensitive fields never enter Renderer state, logs,
exports, or snapshots.

Live Runs and Audit read projections of the same event source. They do not
create a second runtime log.

## 14. Protocol and Renderer Data Flow

New protocol methods use a `harness.*` namespace:

- `harness.overview`
- `harness.topology`
- `harness.profile.list`
- `harness.profile.get`
- `harness.profile.create`
- `harness.profile.archive`
- `harness.draft.get`
- `harness.draft.save`
- `harness.draft.validate`
- `harness.draft.diff`
- `harness.draft.publish`
- `harness.version.list`
- `harness.version.rollback`
- `harness.binding.get`
- `harness.binding.set`
- `harness.hook.catalog`
- `harness.run.getSnapshot`
- `harness.audit.list`

`harness.topology` was added during Phase 2 implementation. The first draft
folded the execution topology into `harness.overview`, but the two have
different shapes and different costs: Overview is a small header the Settings
landing page always loads, while the topology is a per-stage graph with Hook
attachment counts that only the Blueprint workspace needs. Merging them would
have made every Overview load pay for a graph nobody was looking at, against
第 18 节's "only the selected Run builds a detailed topology projection".

Existing `run.list`, `run.get`, and subscription methods are reused for Live
Runs. Harness must not duplicate them.

Methods are advertised only when callable. Unsupported partial deployments
fail honestly.

The Daemon routes the family by the `harness.` prefix rather than by eighteen
literal match arms, so a method cannot be advertised and left unroutable. The
prefix is pinned to exactly the advertised set by
`the_harness_prefix_and_the_advertised_harness_family_agree`, and every method
is driven through the real `handle_rpc` by
`harness_surface_is_advertised_and_really_routed`. That second test also
refuses the handler's own "unsupported harness method" reply, which the generic
fail-closed sweep cannot see.

Renderer data flow:

```text
Native Engine UI
  → tauri-adapter
  → Host ExecutionAuthority façade
  → protocol v2 / UDS
  → Daemon HarnessControlPlane
```

Renderer never opens SQLite, reads project Hook files, executes Hooks, or
connects directly to the Daemon socket.

## 15. MVP Workspaces

### Overview

- active profile and version;
- Harness health;
- Native capability summary;
- source drift and validation warnings;
- recent Runs;
- links to relevant topology nodes.

### Blueprint

- fixed Native execution topology;
- global/project/session scope selector;
- stage inspector;
- read-only policy slots in MVP;
- Draft, Validate, Diff, Publish, and Rollback.

### Hooks

- catalog across all sources;
- source and trust filters;
- enable/disable overlay;
- order, matcher, timeout, and ordinary failure policy;
- create/edit Natives-owned Hooks;
- source drift status.

### Live Runs

- paginated Run list;
- topology state projection;
- Hook invocation details;
- queue and permission state;
- allowed live actions: cancel, interject, permission response.

It does not allow live configuration mutation.

### Audit

- publish and rollback history;
- binding changes;
- Run snapshot lookup;
- Hook trace search;
- redacted export.

## 16. Failure Handling

| Failure | Required behavior |
|---|---|
| Invalid Draft | Reject publish; active version unchanged |
| Draft revision conflict | Return conflict and diff; never overwrite |
| Missing/invalid Harness binding | Native Run does not start |
| Snapshot persistence failure | Native Run does not start |
| Enabled Hook cannot compile | Publish or Run resolution fails according to stage |
| Security Hook timeout/failure | Fail closed |
| Ordinary Hook timeout/failure | Apply published failure policy |
| Source file drift | Warn and create draft candidate; active version unchanged |
| Critical event persistence failure | Stop before new side effects; fail/interrupt Run |
| Daemon restart | Restore profiles/bindings; never silently re-execute a Run |
| UI IPC failure | Loading/error/success state; classified user-visible error |

Error codes are structured and mapped through `classifyError`, including:

- `HARNESS_DRAFT_CONFLICT`
- `HARNESS_VALIDATION_FAILED`
- `HARNESS_SNAPSHOT_PERSIST_FAILED`
- `HARNESS_HOOK_COMPILE_FAILED`
- `HARNESS_SOURCE_DRIFT`
- `HARNESS_EVENT_PERSIST_FAILED`

## 17. Security and Privacy

- Locked safety Hooks cannot be disabled or reordered out of their priority
  band.
- Harness cannot bypass Capability Gateway, broaden path scope, or grant
  credentials.
- Command Hooks retain process sandbox, timeout, cancellation, working
  directory, and trust checks.
- HTTP Hooks retain allowlist and SSRF protections.
- Imported files are treated as untrusted configuration until validated.
- Renderer receives only redacted, typed data.
- Hook stdout/stderr and payload previews are bounded and redacted.
- Secret-like fields are removed before persistence, not merely hidden by UI.
- Configuration writes remain Daemon-owned and broadcast through the existing
  state/event path.

## 18. Performance Design

- Live Runs and Audit are paginated and windowed.
- UI lists over 200 rows are virtualized.
- Stream events are batched per frame.
- Only the selected Run builds a detailed topology projection.
- Blueprint graph, trace inspector, and Audit are lazy loaded.
- No Harness work enters the initial Shell bundle unnecessarily.
- Hook trace payloads are truncated and never duplicate full model/tool
  payloads.
- Session Actors, Run projections, caches, and subscriptions have explicit
  capacity and terminal cleanup.
- No Renderer or Tauri synchronous command performs Hook discovery or file IO.
- Real-time updates use events, not polling.
- Hidden pages pause expensive projections and clean subscriptions.

The implementation must satisfy existing budgets:

- hot IPC p95 ≤ 50ms;
- click/input feedback p95 ≤ 100ms;
- cached page switch p95 ≤ 300ms;
- individual main-thread tasks ≤ 50ms;
- `npm run perf:check`.

## 19. Migration Strategy

### Phase 1: behavior-preserving extraction — **done**

- Add `harness-core`. ✅ `hooks/definition.rs` + `session_actor.rs`.
- Move Session Actor behavior with existing tests. ✅ 829 lines and 12 tests
  moved; `agent-core::session_coordinator` is now a re-export shim.
- Introduce Hook definition/binding types. ✅ `HookDefinition` carries `HookId`,
  `HookSource`, `order`, `matcher`, `conditions`, `timeout_ms`,
  `HookFailurePolicy`, and `HookKind` (with `trusted` on command hooks).
- Keep compatibility adapters for current call sites. ✅ Every existing import
  path still resolves; `SessionHarness`/`HarnessAction` aliases retained.
- Prove the default compiled `RunHarness` matches current behavior. ✅
  `production_hooks.rs` split into `discover_production_hooks` (data) and
  `compile_production_hooks` (registry), guarded by a behaviour-snapshot suite
  written *before* the split.

`HookRegistry` now stores definition and handler together and exposes
`describe()` / `describe_event()`. Dispatch consults the definition's matcher
and conditions in addition to the handler's own matcher, so the catalog cannot
misreport what actually runs.

Scope decisions taken during implementation:

- Hook invocation timing (`duration_ms`) was **not** added here; telemetry is
  Phase 3 by this document's own sequencing.
- `tool_pattern_matches` was moved verbatim. Two quirks — a `None` tool name
  matching only the whole pattern `"*"`, and no `*suffix` support — are pinned
  by test rather than fixed, because changing a security-relevant matcher is a
  behaviour change that needs its own review and has no current demand.
- A pre-existing `agent-core::subagents::FailurePolicy` (`Isolate`/`FailFast`/
  `RequireAll`) forced the Hook one to be named `HookFailurePolicy`; two
  unrelated concepts must not share a domain term.
- `HookFailurePolicy` is recorded but not yet enforced: every discovered Hook
  uses `Fail`, matching today's behaviour. Making it effective is Phase 4 work,
  alongside the UI that sets it.

### Phase 2: control plane and snapshots — **done, one call site pending**

- Add Daemon Harness persistence and migrations. ✅ Migration **022** — six
  tables (`harness_profile`, `harness_version`, `harness_draft`,
  `harness_binding`, `harness_run_snapshot`, `harness_audit`) in the
  Daemon-owned `assistant.db`, WAL and foreign keys unchanged. 021 belongs to
  the capability-library workstream; Harness deliberately skips it.
- Create default published profiles. ✅ `repository::ensure_defaults` seeds
  `harness.global.default` at v1 with `HarnessBlueprint::default()` — no
  overlays — plus the `global` binding. The empty document is the only one that
  provably compiles to today's behaviour, so the seed cannot change semantics.
- Resolve and persist snapshots at Run start. ⚠️ `control_plane::resolve_run`
  resolves, redacts, persists, and returns a `RunHarnessPlan`; it is fully
  tested. The **call site** in `run_manager.rs::start()` is not wired, because
  that file was owned by another workstream in this round. Until it is,
  `harness.run.getSnapshot` reports `resolved: false` rather than inventing
  evidence.
- Keep Settings UI hidden. ✅ No Renderer work beyond the `AssistantMethod`
  union in `src/lib/assistant-protocol/types.ts`, which the `protocol:check`
  gate requires to stay bidirectionally equal to Rust `ALL_METHODS`.

`harness-core` gained `topology`, `blueprint`, `resolver`, `validation`,
`snapshot`, and `redaction`. Scope decisions taken during implementation:

- **The topology records a trigger site per Hook point, not just a stage.**
  Writing the stage table from 第 11 节 exposed that five of the sixteen events
  are not dispatched by the engine loop: `PermissionRequest`,
  `PermissionDenied`, and `Notification` fire from the Daemon's
  `production_tools`, and `SubagentStart` / `SubagentStop` are dispatched by
  **nothing at all**. A stage table alone would present two inert points as
  configurable. `harness_topology_truth.rs` re-derives the whole map from the
  engine sources on every run, so the constant cannot drift into a brochure.
- **`legacy_v1` does not reorder.** Discovery order is today's dispatch order,
  so the resolver leaves it alone; only `sequential_v2` sorts by locked band,
  configured order, and Hook id. Re-sorting under `legacy_v1` would have been a
  behaviour change smuggled in under a configuration feature.
- **The overlay schema cannot carry an executable.** `HookOverlay` has exactly
  the five fields 第 12.2 节 permits, and `deny_unknown_fields` makes `program`,
  `args`, `url`, and `trusted` parse errors rather than ignored keys. A test
  asserts each one is rejected, so the property survives a future field being
  added carelessly.
- **Redaction happens on the way into the snapshot, not at the RPC edge.**
  There is therefore one path into persistence rather than two, and
  `RunHarnessPlan` keeps the redacted `snapshot` and the live `resolution`
  visibly separate — compiling Hooks from a redacted URL would have fired a
  different HTTP request than the user configured.
- **Rollback publishes forward.** Restoring v1 creates v3 with v1's content and
  hash. A Run snapshot references `version_id`, so rewinding a pointer would
  make an old Run's evidence describe a document it never used.
- Telemetry stayed out. `harness.overview` returns
  `telemetry: { available: false, reason: "phase_3" }` rather than an empty
  array a caller would read as "nothing ever ran".

### Phase 3: telemetry and source manifests

- Add Hook identities, source manifests, redaction, drift detection, and
  additive Run events.
- Verify event volume and performance before enabling UI.

### Phase 4: Settings MVP

- Replace the current mixed `RuntimePanel` structure.
- Aggregate Native settings and Engine Capabilities.
- Add Overview, Blueprint, Hooks, Live Runs, and Audit.
- Remove Job UI from RuntimePanel without redesigning the Job module.

### Phase 5: compatibility cleanup

- Remove old `session_harness` shims and global naming.
- Remove per-Run opaque Hook registry assembly.
- Remove deprecated Settings sections only after navigation migration.

At no phase may Assistant and Provider integrations be migrated at the same
time as Harness execution semantics. Each phase first proves parity at the
existing seams.

## 20. Testing Strategy

### `harness-core`

- global/project/session precedence;
- typed sparse overlays and locked-field rejection;
- canonical serialization and stable hash;
- validation and diff;
- Hook ordering, matching, sequential payload modification, and decisions;
- security failure behavior;
- Session Actor queue, interjection, cancel-and-send, terminal drain, and race
  tests.

### Daemon

- incremental SQLite migrations and foreign keys;
- atomic publish and rollback;
- optimistic edit conflicts;
- source discovery and drift;
- startup recovery without silent re-execution;
- snapshot persistence before engine start;
- RPC implemented/advertised parity;
- redaction before event persistence.

### Engine integration

- Run binds the exact resolved snapshot;
- active Run is unaffected by later publish;
- all Hook points and safe points fire at the expected stage;
- security failure closes execution;
- Provider routing and `EngineProvider` behavior are unchanged;
- existing Assistant `run.start` path is unchanged.

### Renderer

- Settings navigation and wide Native workspace;
- loading/error/success states;
- Blueprint validation, diff, publish, rollback;
- Hook provenance and overlay controls;
- Live Runs projection and bounded rendering;
- Audit search and redaction;
- event subscription cleanup;
- Chinese and English strings.

### End-to-end acceptance

1. Publish Harness v1.
2. Start a Native Run and persist the v1 snapshot.
3. Observe stage and Hook traces.
4. Create and publish v2 while the Run is active.
5. Confirm the active Run remains on v1.
6. Start a new Run and confirm it uses v2.
7. Roll back v1 content as v3.
8. Confirm the next Run uses v3.
9. Restart the Daemon and confirm profiles, bindings, snapshots, and audit
   remain available without re-executing Runs.
10. Confirm Claude/Codex Harness views are read-only.
11. Confirm no credential or raw sensitive environment value appears in DB,
    events, Renderer memory, logs, or export.

Before handoff, run:

- `npm run typecheck`
- `npm run lint`
- relevant frontend and Rust tests
- `npm run perf:check`

## 21. Approved Decisions

- Control depth: observe and configure future Runs; no live mutation.
- Scope: full Native execution topology with fixed state machine.
- Settings: Execution Engine → Native Engine.
- Configuration hierarchy: global template → project overlay → session
  selection.
- UI: Overview, Blueprint, Hooks, Live Runs, Audit.
- Hook files: read-only source plus Profile overlay.
- External CLI: read-only ownership view.
- Audit: redacted details with explicit expansion; secrets never shown.
- Publishing: Draft → Validate → Diff → Publish → Rollback.
- MVP: Hooks editable; other engine policy slots read-only.
- Jobs: separate module, excluded.
- Engineering approach: deep Harness control-plane module.
- Compatibility: no Assistant or Provider integration changes.
