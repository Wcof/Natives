# Native Harness Control Plane Design

- Status: Implementation-ready design; Phase 1 complete, Phase 2 partial
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
- effective system-prompt composition, provenance, preview, and Natives-owned
  Prompt Blocks;
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
- an inspectable prompt plan: the effective system prompt is currently assembled
  from Skills, Agent Profile, project/user instruction files, built-in surfaces,
  team roster, and child directives inside `production.rs` without one immutable
  identity or Renderer-safe projection.

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

### 2.2 Current implementation truth (2026-07-27)

| Area | Current truth | UI readiness |
|---|---|---|
| `harness-core` | Hook definition/overlay, resolver, topology, snapshot, redaction, Session Actor are real and tested | reusable |
| Daemon persistence | migration 022 and six `harness_*` tables exist; default v1 profile is seeded | reusable after invariants below |
| RPC | the original 18 `harness.*` methods are advertised, routed, and protocol-sync tested | read/control surface exists |
| Run start | `resolve_run` exists but has **zero production callers**; `ProductionRuntime` still performs its own Hook discovery | blocking |
| Prompt assembly | still assembled directly in `production.rs`/`agent-core::context`; no Prompt Plan, snapshot, or RPC projection | blocking |
| Blueprint | schema v1 can only overlay discovered Hooks; it cannot create a Native Hook or Prompt Block | blocking |
| Telemetry/drift | not implemented; Overview honestly reports telemetry unavailable | blocking for Live Runs/Audit |
| Settings | deliberately hidden; no Native Engine Harness workspace | not started |

Therefore the current code can support the control-plane foundation, but the
old design cannot support the promised Settings feature as written. Phase 2.5,
the Prompt model, Native Hook model, and wire contracts below are required
implementation work, not optional polish.

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
10. Make prompt composition inspectable and allow versioned Natives-owned Prompt
    Blocks without taking ownership of Agent Profiles, Skills, or external
    instruction files.

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
- rewrite imported `AGENTS.md`, `CLAUDE.md`, `.agents/rules`, `.claude/rules`,
  Skill bodies, or Capability Hub Agent Profiles;
- change Job scheduling or decide the Job module's future UI;
- move assistant conversation state into Harness;
- move Provider assets or credentials into Harness;
- duplicate Capability Hub objects or make Harness a second authority for
  conversation/run capability selection;
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
  → existing capability_resolution (fail closed)
  → HarnessControlPlane.resolve_run(capability snapshot reference)
  → persist ResolvedHarnessSnapshot
  → existing Provider routing and credential lease
  → existing ProductionRuntime / AgentEngine path
```

The default migrated Blueprint must compile to behavior equivalent to the
current production path before any setting becomes editable.

`capability_resolution` and Harness resolution share one Run-start seam but
retain separate authorities. Capability Hub owns capability objects and the
conversation/run selection. Harness records the resolved capability snapshot
hash and prompt contribution in its Run evidence; it does not copy capability
objects into `harness_*` tables. This refines ADR-0016 §8, whose earlier
"Blueprint selects capability IDs" wording predates the now-live
conversation/run selection contract.

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
    │   ├── Prompts
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
- prompt composition and Prompt Blocks;
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
├── prompt.rs
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
- typed Natives-owned Hook specs and Prompt Blocks;
- deterministic effective Prompt Plan and source digest model;
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
├── prompt_sources.rs
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
- prompt-source discovery and redacted preview;
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
    prompt: CompiledPromptPlan,
    telemetry: HarnessTelemetrySink,
}
```

It provides:

- safe-point coordination;
- Hook dispatch;
- deterministic prompt assembly;
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

Profile kinds are `global_template` and `project_overlay`. `session_overlay` is
not a profile kind: a Session binding selects an already published profile.
Phase 2 currently accepts `session_overlay`; Phase 2.5 must remove that creation
path and migrate any existing rows before the Settings UI is exposed.

### 9.1.1 Blueprint schema v2

The Settings MVP requires a real schema upgrade. The current schema v1 contains
only imported-Hook overlays and cannot implement either “create Natives Hook” or
Prompt Engineering.

```text
HarnessBlueprintV2
├── schema_version = 2
├── hook_semantics_version
├── hook_overlays[]       # overlays for discovered/read-only Hooks
├── native_hooks[]        # complete Natives-owned Hook definitions
└── prompt_blocks[]       # Natives-owned system-prompt fragments
```

- `hook_overlays` may change only enable/order/matcher/timeout/failure policy.
  It cannot carry executable, URL, trust, or source fields.
- `native_hooks` uses stable generated UUIDs, strict typed command/HTTP adapter
  configuration, typed conditions, and explicit trust. Renaming does not change
  identity.
- `prompt_blocks` uses stable generated UUIDs, name, enabled state, order,
  placement, and Markdown content.
- Every structure uses `deny_unknown_fields`. Duplicate IDs and references to
  unknown/locked Hooks fail validation.
- Schema v1 remains readable. Publishing the first v2 edit creates a new
  immutable version; stored v1 versions and old Run snapshots are never
  rewritten.

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

Snapshot insertion is idempotent only when the existing snapshot has the same
canonical hash. A retry that resolves a different hash for the same `run_id`
returns `HARNESS_SNAPSHOT_CONFLICT`; `INSERT ... ON CONFLICT DO NOTHING` without
hash verification is forbidden.

## 10. Persistence Model

All tables live in Daemon-owned `assistant.db`. Migrations are incremental,
WAL remains enabled, and foreign keys declare cascade behavior.

### `harness_profile`

Logical profile or overlay identity:

- `id`
- `name`
- `description`
- `kind` (`global_template`, `project_overlay`)
- `project_id` when scoped
- `current_published_version_id`
- timestamps

### `harness_draft`

Editable state:

- `profile_id`
- `base_version_id`
- `document_json`
- optional `source_candidate_json` (bounded redacted manifest metadata only)
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
- effective Prompt Plan hash, layer/source digests, and redacted bounded
  previews;
- capability snapshot hash/reference, never capability bodies or secrets;
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
- The published source manifest stores stable source metadata, content digest,
  source policy, and a redacted structural projection. It does not copy raw
  prompt files, credentials, or arbitrary executable bytes into
  `assistant.db`.
- Later file changes create Source Drift and a draft candidate. They do not
  mutate an active published version.
- Legacy environment Hooks are represented as read-only sources during
  migration, materialized into the resolved snapshot, and never hidden.

Claude CLI and Codex CLI Harness configuration is read-only in Natives. The UI
states which external Runtime owns it and how the user can edit it externally.

Source policy is explicit:

- `tracked`: the next Run uses the current source, records its exact digest in
  that Run snapshot, and surfaces drift. This is the default for imported
  instruction/prompt files.
- `pinned`: a digest mismatch fails Run resolution until the drift is
  acknowledged and published. This is the default for enabled external
  command/HTTP Hook definitions and referenced command files.

An acknowledgement starts from the current published version. If an unrelated
Draft already exists, it returns a revision conflict and never overwrites that
Draft. Active Runs always keep their compiled in-memory source regardless of
later drift.

#### 12.2.1 Tracked drift Draft candidate

Tracked drift reconciliation is owned by `HarnessControlPlane`; source
discovery only reports one observed manifest. Reconciliation compares the
complete observed source set with the published manifest and applies the whole
set in one transaction:

1. no mismatch: keep all sources `current`;
2. repeated observation of the same published version and same mismatched
   source/digest set: return the existing result without another Draft or audit
   row;
3. one or more mismatches with no Draft: clone the current published document
   into one Draft, set its `base_version_id` to that published version, attach
   the complete mismatched set as candidate metadata, and emit one
   `source_drift` invalidation;
4. mismatch while any other Draft already exists: leave that Draft
   byte-for-byte unchanged and return `HARNESS_DRAFT_CONFLICT` with its
   revision;
5. if the published version changes during reconciliation: retry from the new
   published version or return a revision conflict; never create a candidate
   based on stale published state.

Candidate metadata contains only profile ID, base version ID, source ID,
published digest, observed digest, policy, observed timestamp, and redacted
structural manifest. It contains no raw source body. The candidate does not
alter executable Blueprint fields by itself: the user reviews the source diff,
then `harness.source.acknowledgeDrift` records the observed digest in the Draft
manifest using the expected Draft revision. Publish remains the only operation
that changes the active manifest.

The tracked Run that detected drift still uses the observed source and records
its digest in the frozen Run snapshot. Candidate creation failure is surfaced
but does not replace that Run's already resolved plan. Pinned drift remains
fail-closed; it may be acknowledged into a Draft through the same
revision-checked path, but the Run cannot start until that Draft is published.

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

### 12.5 Natives-owned Hook definitions

The existing `HookOverlay` is intentionally unable to carry an executable or
URL. Therefore Hook creation cannot be implemented by widening that type.
Blueprint v2 adds a separate `NativeHookSpec`:

- stable UUID, display name, event, enabled, order, matcher, typed conditions;
- timeout and ordinary failure policy;
- adapter: `command { program, args, working_directory_policy, trusted }` or
  `http { url, allow_hosts }`;
- source is always `native`, locked is always false;
- command trust elevation requires an explicit publish confirmation and audit
  entry;
- program/path, HTTP allowlist, timeout, and security-sensitive failure policy
  are validated before publish and again at Run resolution.

Imported Hooks remain read-only definitions plus sparse overlays. A Native Hook
may be removed from a Draft, but a published version and every Run snapshot keep
the complete historical definition.

## 12.6 Prompt Engineering

Prompt Engineering is part of Native Harness because prompt composition is an
execution-plan concern. It does not absorb the authorities that own individual
sources.

### 12.6.1 Source ownership

| Prompt source | Authority | Harness UI |
|---|---|---|
| Built-in surface instructions | Native Engine code | read-only, locked |
| Capability expert/profile prompt | Capability Hub | read-only reference |
| Skill catalog contribution | Capability Hub / Skill runtime | read-only reference |
| User/project `AGENTS.md`, `CLAUDE.md`, rules | filesystem owner | read-only, drift-aware |
| Team roster/delegation text | capability resolution | read-only, generated |
| Child Run directive | parent Run | read-only, Run-scoped |
| Natives Prompt Block | Harness Blueprint | editable and versioned |

Harness must never write imported files or duplicate Agent Profiles/Skills.
“Edit source” opens or reveals the owning surface; only Natives Prompt Blocks
are edited in this workspace.

### 12.6.2 Fixed Prompt Plan

The current assembly order is first captured as `prompt_semantics_version =
legacy_v1` with a golden parity test. A later ordering change requires a new
semantics version and explicit publish, exactly like Hook semantics.

Each Prompt Plan layer records:

- stable source ID, source kind, owner, scope, order, and enabled/locked state;
- content digest and contribution character/token estimate;
- redacted bounded preview for Renderer/audit;
- full text only in the in-memory compiled plan used by `AgentEngine`.

The persisted Run snapshot stores the effective raw-prompt SHA-256 plus the
redacted structural plan. It does not persist the raw combined system prompt or
secret-like source text. This proves which prompt executed without creating a
second plaintext cache.

### 12.6.3 Prompt Blocks

`PromptBlockSpec` contains:

- stable UUID, name, Markdown content, enabled, order;
- placement: `before_profile`, `after_profile`,
  `after_project_instructions`, or `final`;
- scope inherited from its published Profile layer.

Prompt Blocks affect future Native Runs only. They cannot grant tools, change
permissions, broaden path scope, choose credentials, or bypass Capability
Gateway. Empty blocks, duplicate IDs, oversized content, invalid placement, and
locked-layer edits fail validation. MVP limits a block to 64 KiB and the
effective Natives-owned contribution to 256 KiB before model token budgeting.

### 12.6.4 Preview

Prompt preview resolves the same source manifest and ordering function used by
Run start. It returns the layer list, digests, estimates, warnings, and redacted
text. Preview never becomes execution evidence; only the snapshot persisted at
Run start is authoritative.

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

### 13.1 Hook invocation telemetry seam

`HookRegistry::dispatch_outcomes` is the only execution seam that observes
every real handler invocation. The registry uses the Run's `EventSequencer` to
emit:

- `HookInvocationStarted` immediately after matcher/condition gates and
  immediately before `handle_outcome`;
- `HookInvocationCompleted` exactly once after the handler returns, times out,
  fails, or is cancelled, after applying the published failure policy.

Both events share a stable per-dispatch invocation ID and include the snapshot
Hook ID, dispatch ordinal, Hook event, source identity, redacted summaries, and
structured outcome. Duration is measured by a monotonic clock. Matcher misses
do not invoke handlers and do not emit per-Hook rows; the trace projection
derives them from the frozen snapshot and dispatch input.

`agent-core` reuses its existing `EventSequencer`; it does not add a second
telemetry interface or depend on Daemon storage. The compiled `HookRegistry`
receives the same sequencer already owned by `AgentEngine`, and
`dispatch_outcomes` becomes fallible so persistence failure can reach every
caller. `EventSequencer` persists each `run_event` before broadcasting it. A
failed `HookInvocationStarted` persistence stops before calling the handler. A
failed completion persistence interrupts the Run before its next side effect.
Handler panic/cancellation must still produce a terminal completion event when
the process remains able to persist one.

Resolution may emit `HarnessPlanResolved`, but it must not fabricate Hook
invocation rows. `harness.trace.list` is a paginated, redacted projection over
`run_events`; `harness_hook_trace` must not become a second durable authority.
If a materialized trace table is retained for query performance, it is a
rebuildable projection populated from committed `run_events`, never written
directly by resolution or dispatch.

Configuration/UI invalidation uses one bounded Daemon broadcaster exposed by
`harness.subscribe`. It emits only small change notices (`published`,
`binding_changed`, `source_drift`, `trace_updated`, `reset_required`), not
configuration documents. Audit rows and `run_events` remain the durable truth;
after reconnect or `reset_required`, the client refetches the selected
projection. The Host adapter owns the subscription and forwards Tauri events,
so React components do not poll.

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
- `harness.prompt.preview`
- `harness.source.list`
- `harness.source.acknowledgeDrift`
- `harness.subscribe`
- `harness.hook.catalog`
- `harness.run.getSnapshot`
- `harness.trace.list`
- `harness.audit.list`
- `harness.audit.export`

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

The Daemon routes the family by the `harness.` prefix rather than by literal
top-level match arms, so a method cannot be advertised and left unroutable.
The original 18-method baseline is pinned to exactly the advertised set by
`the_harness_prefix_and_the_advertised_harness_family_agree`, and every method
is driven through the real `handle_rpc` by
`harness_surface_is_advertised_and_really_routed`. That second test also
refuses the handler's own "unsupported harness method" reply, which the generic
fail-closed sweep cannot see. Phase 3 must extend those same guards for every
new method above before advertising it.

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

### 14.1 Required wire contracts

Rust protocol types in `assistant-protocol` are the wire authority; TypeScript
types are generated/checked projections. Handlers must not accept an
undocumented bag of JSON.

| Method family | Required request fields | Required result/behavior |
|---|---|---|
| overview/topology/catalog/prompt preview | optional `project_id`, `project_path`, `conversation_id`; all three are validated as one scope | resolved layers plus schema/semantics versions and structured issues |
| profile list/get/create/archive | cursor/limit where listing; create accepts only `global_template` or `project_overlay` | typed profile/version summaries; no archived profile may be newly bound |
| draft get/save/validate/diff/publish | `profile_id`; save/publish require expected `revision` | conflict includes current revision and a reloadable server draft; publish is atomic |
| version list/rollback | `profile_id`, cursor/limit; rollback requires `version_id` and expected current version | rollback publishes forward as a new version |
| binding get/set | `scope_type`, `scope_id`; set requires profile, mode, optional pinned version | project scope uses stable ProjectIdentity UUID; session scope uses conversation ID |
| source list/acknowledge | scope + cursor/limit; acknowledgement requires source ID, observed digest, and expected Draft revision | tracked reconciliation creates the Draft candidate; acknowledgement revision-checks and accepts its manifest into that Draft plus an audit row; neither mutates published source material |
| subscribe | optional cursor and bounded wait | small invalidation notices plus next cursor; reconnect may return `reset_required` |
| run snapshot | `run_id` | `resolved=false` only for Runs created before migration or never started; a post-migration started Native Run without a snapshot is an integrity error |
| trace/audit list | cursor/limit and typed filters | stable cursor pagination, bounded redacted summaries, no full payloads |
| audit export | typed filters plus explicit export format | Daemon streams/returns a bounded redacted artifact reference; Renderer never rebuilds an unbounded export in memory |

All list methods use a common `{ items, next_cursor }` envelope and cap `limit`
at 200. Mutations return the new `revision`/version identity. Error codes are
stable lowercase protocol codes. The Renderer does not infer behavior from
English error strings.

Scope validation rules:

- `project_id` is the stable ProjectIdentity UUID; `project_path` is used only
  for source discovery and must resolve to that identity.
- A mismatch returns `HARNESS_SCOPE_MISMATCH`.
- Missing global binding, dangling existing bindings, archived profiles, and
  missing pinned versions fail Run resolution; an absent optional project or
  session binding means inheritance and remains valid.
- Inspection may omit project/session scope and show the global layer, but
  mutation methods never invent an identity as a side effect.

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

### Prompts

- effective Prompt Plan in exact assembly order;
- source owner, scope, digest, contribution estimate, and drift state;
- redacted preview for imported/read-only sources;
- create/edit/reorder/enable Natives Prompt Blocks;
- validation and diff before publish;
- selected Run comparison: snapshot Prompt Plan versus current Draft.

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
| Existing snapshot hash differs on retry | Native Run does not start; integrity conflict |
| Enabled Hook cannot compile | Publish or Run resolution fails according to stage |
| Native Hook trust/adapter validation fails | Reject publish |
| Prompt source/Prompt Block cannot compile | Reject publish or Run resolution |
| Security Hook timeout/failure | Fail closed |
| Ordinary Hook timeout/failure | Apply published failure policy |
| Tracked source drift | Warn and offer draft candidate; active Run unchanged |
| Pinned source drift | Fail Run resolution until acknowledged and published |
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
- `HARNESS_SNAPSHOT_CONFLICT`
- `HARNESS_SCOPE_MISMATCH`
- `HARNESS_PROMPT_COMPILE_FAILED`
- `HARNESS_NATIVE_HOOK_INVALID`

## 17. Security and Privacy

- Locked safety Hooks cannot be disabled or reordered out of their priority
  band.
- Harness cannot bypass Capability Gateway, broaden path scope, or grant
  credentials.
- Command Hooks retain process sandbox, timeout, cancellation, working
  directory, and trust checks.
- HTTP Hooks retain allowlist and SSRF protections.
- Imported files are treated as untrusted configuration until validated.
- Imported prompt sources are read-only; previews and diffs are redacted before
  crossing the protocol seam.
- Raw effective system prompts are not persisted. Run evidence stores a hash,
  source digests, estimates, and bounded redacted previews.
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
- Blueprint graph, Prompt preview, trace inspector, and Audit are lazy loaded.
- No Harness work enters the initial Shell bundle unnecessarily.
- Hook trace payloads are truncated and never duplicate full model/tool
  payloads.
- Prompt preview has explicit byte limits and is recomputed only on user action,
  Draft change, or source-change event; typing does not rescan the filesystem.
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

### Phase 2: control plane and snapshots — **partial**

- Add Daemon Harness persistence and migrations. ✅ Migration **022** — six
  tables (`harness_profile`, `harness_version`, `harness_draft`,
  `harness_binding`, `harness_run_snapshot`, `harness_audit`) in the
  Daemon-owned `assistant.db`, WAL and foreign keys unchanged. 021 belongs to
  the capability-library workstream; Harness deliberately skips it.
- Create default published profiles. ✅ `repository::ensure_defaults` seeds
  `harness.global.default` at v1 with `HarnessBlueprint::default()` — no
  overlays — plus the `global` binding. The empty document is the only one that
  provably compiles to today's behaviour, so the seed cannot change semantics.
- Resolve and persist snapshots at Run start. ❌ `control_plane::resolve_run`
  resolves, redacts, persists, and returns a `RunHarnessPlan`; it is fully
  tested. The **call site** in `run_manager.rs::start()` is not wired, because
  that file was owned by another workstream in this round. Until it is,
  `harness.run.getSnapshot` reports `resolved: false` rather than inventing
  evidence. This is not a cosmetic call-site remainder: until the Run start
  path consumes the returned compiled plan, published Harness configuration
  does not affect execution and snapshots are not execution evidence.
- Keep Settings UI hidden. ✅ No Renderer work beyond the `AssistantMethod`
  union in `src/lib/assistant-protocol/types.ts`, which the `protocol:check`
  gate requires to stay bidirectionally equal to Rust `ALL_METHODS`.

`harness-core` gained `topology`, `blueprint`, `resolver`, `validation`,
`snapshot`, and `redaction`. Scope decisions taken during implementation:

- **The topology records a trigger site per Hook point, not just a stage.**
  Writing the stage table from 第 11 节 exposed that five events are not
  dispatched by the engine loop: `PermissionRequest`,
  `PermissionDenied`, and `Notification` fire from the Daemon's
  `production_tools`; `SubagentStart` / `SubagentStop` are now also dispatched
  from the Daemon task-tool path. `harness_topology_truth.rs` re-derives the
  whole map from the engine sources on every run, so the constant cannot drift
  into a brochure.
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

### Phase 2.5: execution seam and schema v2 — **required before UI**

- Wire `HarnessControlPlane.resolve_run` after capability resolution and before
  Provider lease/Hook compilation.
- Compile the exact returned Hook resolution; remove the second discovery pass
  from `ProductionRuntime`.
- Persist and hash-check the snapshot before Provider or tool side effects.
- Introduce Blueprint schema v2 (`hook_overlays`, `native_hooks`,
  `prompt_blocks`) while keeping v1 readable.
- Extract current system-prompt assembly into a pure Prompt Plan and prove
  `legacy_v1` parity with golden tests.
- Record capability snapshot reference/hash in Harness snapshot without
  changing Capability Hub ownership.
- Make missing bindings/versions fail closed and enforce stable
  ProjectIdentity/path agreement.
- Stop accepting new `session_overlay` profiles.
- Only after all above pass may `harness.run.getSnapshot` treat a started
  post-migration Native Run without a snapshot as an integrity error.

### Phase 3: telemetry and source manifests

- Keep existing Hook identities/redaction and add persisted source manifests,
  Hook/Prompt source drift detection, and acknowledgement workflow.
- Add bounded additive Harness/Hook events and the paginated trace projection.
- Enforce ordinary Hook failure policy only after a parity test pins
  `legacy_v1`; enable `sequential_v2` only through explicit publish.
- Verify event volume and performance before enabling UI.

### Phase 4: Settings MVP

- Replace the current mixed `RuntimePanel` structure.
- Aggregate Native settings and Engine Capabilities.
- Add Overview, Blueprint, Hooks, Prompts, Live Runs, and Audit.
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
- Blueprint v1 read compatibility and explicit v2 publish migration;
- Native Hook spec validation, stable identity, and adapter restrictions;
- Hook ordering, matching, sequential payload modification, and decisions;
- Prompt Plan `legacy_v1` parity, Prompt Block ordering/limits, source digests,
  and deterministic effective-prompt hash;
- security failure behavior;
- Session Actor queue, interjection, cancel-and-send, terminal drain, and race
  tests.

### Daemon

- incremental SQLite migrations and foreign keys;
- atomic publish and rollback;
- optimistic edit conflicts;
- source discovery and drift;
- tracked drift creates one idempotent Draft candidate, preserves an existing
  Draft, and detects published-version/Draft revision races;
- startup recovery without silent re-execution;
- snapshot persistence before engine start;
- idempotent same-hash snapshot retry and different-hash conflict;
- ProjectIdentity/path mismatch rejection;
- missing profile/version/binding rejection;
- RPC implemented/advertised parity;
- typed request/response round trips and common cursor pagination;
- redaction before event persistence.

### Engine integration

- Run binds the exact resolved snapshot;
- Engine compiles Hooks and Prompt Plan from the same resolution that produced
  that snapshot; no second discovery/assembly pass;
- active Run is unaffected by later publish;
- all Hook points and safe points fire at the expected stage;
- each matched handler produces one persisted-before-broadcast start/completion
  pair, including failure, timeout, cancellation, and security fail-close;
- unmatched Hooks create no per-Hook event and trace pagination is derived from
  `run_events`;
- security failure closes execution;
- Provider routing and `EngineProvider` behavior are unchanged;
- existing Assistant `run.start` path is unchanged.

### Renderer

- Settings navigation and wide Native workspace;
- loading/error/success states;
- Blueprint validation, diff, publish, rollback;
- Hook provenance and overlay controls;
- Native Hook creation/trust confirmation;
- Prompt source provenance, redacted preview, Prompt Block editing, and
  snapshot-versus-Draft comparison;
- Live Runs projection and bounded rendering;
- Audit search and redaction;
- event subscription cleanup;
- Chinese and English strings.

### End-to-end acceptance

1. Publish a v2 Harness containing one Natives Prompt Block and one harmless
   Natives Hook.
2. Start a Native Run and persist the exact v2 Hook/Prompt snapshot before the
   first Provider call.
3. Observe stage, Hook traces, and the redacted Prompt Plan/hash.
4. Publish v3 while the Run is active.
5. Confirm the active Run remains on v2.
6. Start a new Run and confirm it uses v3.
7. Roll back v2 content as v4.
8. Confirm the next Run uses v4.
9. Restart the Daemon and confirm profiles, bindings, snapshots, and audit
   remain available without re-executing Runs.
10. Confirm Claude/Codex Harness views are read-only.
11. Confirm no credential or raw sensitive environment value appears in DB,
    events, Renderer memory, logs, or export.
12. Delete or move an imported Hook/prompt source and confirm Source Drift
    creates a Draft candidate while an active Run remains unchanged.
13. Retry the same `run_id` with a different resolved hash and confirm execution
    is rejected before side effects.

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
- UI: Overview, Blueprint, Hooks, Prompts, Live Runs, Audit.
- Hook files: read-only source plus Profile overlay; Natives-owned Hooks are
  complete typed definitions in Blueprint v2.
- Prompt sources: imported/capability sources read-only; Natives Prompt Blocks
  editable and versioned.
- External CLI: read-only ownership view.
- Audit: redacted details with explicit expansion; secrets never shown.
- Publishing: Draft → Validate → Diff → Publish → Rollback.
- MVP: Hook overlays, Natives-owned Hooks, and Natives Prompt Blocks editable;
  other engine policy slots read-only.
- Jobs: separate module, excluded.
- Engineering approach: deep Harness control-plane module.
- Compatibility: no Assistant wire-contract or Provider-module changes;
  Run-start internals consume one combined capability/Harness resolution.

## 22. GPT-5.6 Implementation Handoff

Implement in these mergeable slices; do not start Renderer work before slices
1–3 pass:

1. **Core schema and parity** — Blueprint v2, Native Hook specs, Prompt Plan,
   v1 reader, validation/diff/hash tests, and current prompt-assembly golden
   parity.
2. **Run-start authority** — compose capability resolution and
   `resolve_run`, verify stable project identity, persist/hash-check snapshot,
   and pass the returned Hook/Prompt plan into `ProductionRuntime`.
3. **Source and telemetry truth** — source manifests/drift, additive persisted
   events, trace/audit cursor projections, protocol types, and advertised-route
   parity.
4. **Settings shell** — replace mixed `RuntimePanel`, preserve one Execution
   Engine navigation entry, add lazy Native Engine routes and three-state data
   loading.
5. **Workspaces** — Overview, Blueprint, Hooks, Prompts, Live Runs, Audit;
   virtualize/window large lists and clean all subscriptions.
6. **Cleanup** — remove compatibility shims and old Settings sections only
   after the new path passes end-to-end acceptance.

Stop the implementation and update this design rather than guessing if any of
these invariants cannot be met:

- one Run resolution cannot produce both the executable plan and its snapshot;
- a UI control has no typed Blueprint field or callable protocol method;
- an imported/capability-owned object would need to be copied into Harness
  storage;
- a started post-migration Native Run can execute without a persisted Harness
  snapshot;
- raw prompt content, Hook secrets, credentials, or environment secrets would
  cross into persisted events or Renderer state.
