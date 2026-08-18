//! Migration SQL constants (W9 split from storage/migrations.rs).
//!
//! The ordered registry `migrations::ALL` in `migrations.rs` remains the single
//! source of truth; this file only holds the versioned SQL strings for its
//! domain. Editing an applied migration still fails closed via checksum.

pub(crate) const MIGRATION_021: &str = "
CREATE TABLE IF NOT EXISTS capability_skill (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    scope TEXT NOT NULL DEFAULT 'user' CHECK(scope IN ('user','project')),
    project_id TEXT,
    dir_path TEXT NOT NULL,
    content_hash TEXT,
    category TEXT,
    tags_json TEXT NOT NULL DEFAULT '[]',
    enabled INTEGER NOT NULL DEFAULT 1,
    trusted INTEGER NOT NULL DEFAULT 0,
    source TEXT NOT NULL DEFAULT 'scan'
        CHECK(source IN ('scan','import_zip','import_dir','import_git')),
    source_ref TEXT,
    engine_targets_json TEXT NOT NULL DEFAULT '[\"native\"]',
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(scope, name, dir_path)
);
CREATE INDEX IF NOT EXISTS idx_capability_skill_category
    ON capability_skill(category);

CREATE TABLE IF NOT EXISTS capability_mcp_server (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    transport TEXT NOT NULL CHECK(transport IN ('stdio','http','sse')),
    command TEXT,
    args_json TEXT NOT NULL DEFAULT '[]',
    env_json TEXT NOT NULL DEFAULT '{}',
    url TEXT,
    headers_json TEXT NOT NULL DEFAULT '{}',
    auth_mode TEXT NOT NULL DEFAULT 'none' CHECK(auth_mode IN ('none','bearer','oauth')),
    oauth_config_json TEXT NOT NULL DEFAULT '{}',
    trusted INTEGER NOT NULL DEFAULT 0,
    enabled INTEGER NOT NULL DEFAULT 1,
    source TEXT NOT NULL DEFAULT 'manual' CHECK(source IN ('manual','import_json','hub')),
    hub_ref TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS capability_expert (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    system_prompt TEXT NOT NULL,
    tools_json TEXT NOT NULL DEFAULT '[]',
    disallowed_tools_json TEXT NOT NULL DEFAULT '[]',
    permission_mode TEXT,
    skills_json TEXT NOT NULL DEFAULT '[]',
    provider_id TEXT,
    key_id TEXT,
    model_id TEXT,
    params_json TEXT NOT NULL DEFAULT '{}',
    enabled INTEGER NOT NULL DEFAULT 1,
    source TEXT NOT NULL DEFAULT 'manual'
        CHECK(source IN ('manual','import_md','host_migration')),
    source_path TEXT,
    content_hash TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS capability_expert_team (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    strategy TEXT NOT NULL DEFAULT 'parallel'
        CHECK(strategy IN ('parallel','sequential','coordinator')),
    failure_policy TEXT NOT NULL DEFAULT 'isolate'
        CHECK(failure_policy IN ('isolate','fail_fast','require_all')),
    max_concurrent INTEGER NOT NULL DEFAULT 3,
    coordinator_expert_id TEXT REFERENCES capability_expert(id) ON DELETE SET NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS capability_expert_team_member (
    team_id TEXT NOT NULL REFERENCES capability_expert_team(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    expert_id TEXT NOT NULL REFERENCES capability_expert(id) ON DELETE CASCADE,
    role_hint TEXT NOT NULL DEFAULT '',
    task_template TEXT NOT NULL DEFAULT '',
    PRIMARY KEY(team_id, position)
);
CREATE INDEX IF NOT EXISTS idx_capability_team_member_expert
    ON capability_expert_team_member(expert_id);

CREATE TABLE IF NOT EXISTS capability_mcp_hub_cache (
    registry_name TEXT PRIMARY KEY,
    payload_json TEXT NOT NULL,
    etag TEXT,
    fetched_at TEXT NOT NULL DEFAULT (datetime('now'))
);

ALTER TABLE conversation ADD COLUMN capability_selection_json TEXT;
ALTER TABLE run ADD COLUMN capability_snapshot_json TEXT;
";

/// Migration 022: Harness control plane (design 第 10 节).
///
/// Six tables implementing the frozen configuration hierarchy — global
/// template, project overlay, session selection — plus the immutable evidence
/// a Run leaves behind. Nothing here stores a credential: a Blueprint carries
/// only Hook overlays, and a snapshot carries redacted adapter configuration
/// (`harness_core::redaction`).
///
/// Foreign keys and cascades, deliberately:
///
/// - draft / version / binding cascade from `harness_profile`: a deleted
///   profile must not leave a draft that publishes into nothing.
/// - `harness_run_snapshot.run_id` cascades from `run`: snapshots are Run
///   evidence and have no meaning once the Run row is gone.
/// - `harness_profile.current_published_version_id` is deliberately **not** a
///   foreign key. It and `harness_version.profile_id` would form a cycle that
///   SQLite can only resolve with deferred constraints, and an atomic publish
///   already maintains the pointer inside one transaction.
/// - `harness_audit` has no foreign keys at all: an audit trail that
///   disappears when the thing it audits is deleted is not an audit trail.
pub(crate) const MIGRATION_022: &str = "
CREATE TABLE IF NOT EXISTS harness_profile (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    kind TEXT NOT NULL CHECK(kind IN (
        'global_template', 'project_overlay', 'session_overlay'
    )),
    project_id TEXT,
    current_published_version_id TEXT,
    archived_at TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_harness_profile_kind
    ON harness_profile(kind, project_id);

CREATE TABLE IF NOT EXISTS harness_version (
    id TEXT PRIMARY KEY,
    profile_id TEXT NOT NULL REFERENCES harness_profile(id) ON DELETE CASCADE,
    version_number INTEGER NOT NULL,
    parent_version_id TEXT REFERENCES harness_version(id) ON DELETE SET NULL,
    document_json TEXT NOT NULL,
    canonical_hash TEXT NOT NULL,
    source_manifest_json TEXT NOT NULL DEFAULT '{}',
    validation_summary_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(profile_id, version_number)
);
CREATE INDEX IF NOT EXISTS idx_harness_version_profile
    ON harness_version(profile_id, version_number DESC);

CREATE TABLE IF NOT EXISTS harness_draft (
    profile_id TEXT PRIMARY KEY REFERENCES harness_profile(id) ON DELETE CASCADE,
    base_version_id TEXT REFERENCES harness_version(id) ON DELETE SET NULL,
    document_json TEXT NOT NULL,
    revision INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS harness_binding (
    scope_type TEXT NOT NULL CHECK(scope_type IN ('global', 'project', 'session')),
    scope_id TEXT NOT NULL,
    profile_id TEXT NOT NULL REFERENCES harness_profile(id) ON DELETE CASCADE,
    version_id TEXT REFERENCES harness_version(id) ON DELETE SET NULL,
    mode TEXT NOT NULL DEFAULT 'follow_published' CHECK(mode IN (
        'follow_published', 'pinned'
    )),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (scope_type, scope_id)
);

CREATE TABLE IF NOT EXISTS harness_run_snapshot (
    run_id TEXT PRIMARY KEY REFERENCES run(id) ON DELETE CASCADE,
    global_version_id TEXT,
    project_version_id TEXT,
    session_version_id TEXT,
    snapshot_json TEXT NOT NULL,
    canonical_hash TEXT NOT NULL,
    topology_version INTEGER NOT NULL,
    hook_semantics_version TEXT NOT NULL,
    resolved_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_harness_run_snapshot_hash
    ON harness_run_snapshot(canonical_hash);

CREATE TABLE IF NOT EXISTS harness_audit (
    id TEXT PRIMARY KEY,
    action TEXT NOT NULL CHECK(action IN (
        'profile_create', 'profile_archive', 'publish', 'rollback',
        'binding_change', 'source_drift_ack'
    )),
    profile_id TEXT,
    version_id TEXT,
    scope_type TEXT,
    scope_id TEXT,
    actor TEXT NOT NULL DEFAULT 'user',
    summary_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_harness_audit_created
    ON harness_audit(created_at DESC, id DESC);
CREATE INDEX IF NOT EXISTS idx_harness_audit_profile
    ON harness_audit(profile_id, created_at DESC);
";

pub(crate) const MIGRATION_023: &str = "
CREATE TABLE IF NOT EXISTS harness_source_manifest (
    source_id TEXT PRIMARY KEY,
    profile_id TEXT,
    digest TEXT NOT NULL,
    mode TEXT NOT NULL DEFAULT 'tracked' CHECK(mode IN ('tracked','pinned')),
    status TEXT NOT NULL DEFAULT 'current' CHECK(status IN ('current','drifted')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_harness_source_manifest_status
    ON harness_source_manifest(status, updated_at DESC);
CREATE TABLE IF NOT EXISTS harness_hook_trace (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL,
    hook_id TEXT NOT NULL,
    phase TEXT NOT NULL,
    status TEXT NOT NULL,
    duration_ms INTEGER,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_harness_hook_trace_run
    ON harness_hook_trace(run_id, created_at DESC, id DESC);
";

/// Migration 024: bounded metadata for an automatically discovered source drift.
pub(crate) const MIGRATION_024: &str = "
ALTER TABLE harness_draft ADD COLUMN source_candidate_json TEXT;
";

/// Migration 025: stable ProjectIdentity registration table.
pub(crate) const MIGRATION_025: &str = "
CREATE TABLE IF NOT EXISTS harness_project_identity (
    project_id TEXT PRIMARY KEY,
    canonical_path TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_harness_project_identity_path
    ON harness_project_identity(canonical_path);
";

/// Migration 026: bounded, replayable Harness invalidation notices.
///
/// Configuration documents and Hook payloads never enter this table. Durable
/// audit/run events remain authoritative; notices only tell clients which
/// projection to refetch after a change.
pub(crate) const MIGRATION_026: &str = "
CREATE TABLE IF NOT EXISTS harness_notice (
    cursor INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT NOT NULL CHECK(kind IN (
        'published', 'binding_changed', 'source_drift',
        'trace_updated', 'reset_required'
    )),
    profile_id TEXT,
    run_id TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_harness_notice_created
    ON harness_notice(created_at DESC, cursor DESC);
";

/// Migration 027: make notices atomic with their audit/run-event authority.
///
/// Kept separate from 026 so databases that observed the initial notice-table
/// migration while this feature was under development still receive triggers.
pub(crate) const MIGRATION_027: &str = "
CREATE TRIGGER IF NOT EXISTS trg_harness_audit_notice
AFTER INSERT ON harness_audit
BEGIN
    INSERT INTO harness_notice(kind, profile_id)
    VALUES(
        CASE NEW.action
            WHEN 'binding_change' THEN 'binding_changed'
            WHEN 'source_drift_ack' THEN 'source_drift'
            ELSE 'published'
        END,
        NEW.profile_id
    );
END;
CREATE TRIGGER IF NOT EXISTS trg_harness_trace_notice
AFTER INSERT ON run_event
WHEN NEW.event_type IN ('hook_invocation_started', 'hook_invocation_completed')
BEGIN
    INSERT INTO harness_notice(kind, run_id)
    VALUES('trace_updated', NEW.run_id);
END;
CREATE TRIGGER IF NOT EXISTS trg_harness_notice_bound
AFTER INSERT ON harness_notice
BEGIN
    DELETE FROM harness_notice
     WHERE cursor <= (SELECT COALESCE(MAX(cursor), 0) - 1000 FROM harness_notice);
END;
";

/// Migration 028: durable Agent Core turn/message/context and recovery seams.
///
/// All additions are nullable or have defaults so old daemon databases remain
/// readable. The existing conversation/message/event tables remain the single
/// authority; these columns make the typed runtime identity recoverable rather
/// than keeping it only in memory.
pub(crate) const MIGRATION_028: &str = "
CREATE TABLE IF NOT EXISTS turn (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES run(id) ON DELETE CASCADE,
    sequence INTEGER NOT NULL,
    status TEXT NOT NULL DEFAULT 'committed'
        CHECK(status IN ('started','committed','failed','cancelled','abandoned')),
    stop_reason TEXT,
    input_tokens INTEGER,
    output_tokens INTEGER,
    reasoning_tokens INTEGER,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    completed_at TEXT,
    UNIQUE(run_id, sequence)
);
CREATE INDEX IF NOT EXISTS idx_turn_run_sequence ON turn(run_id, sequence);

ALTER TABLE message ADD COLUMN turn_id TEXT REFERENCES turn(id) ON DELETE SET NULL;
ALTER TABLE message ADD COLUMN run_id TEXT REFERENCES run(id) ON DELETE SET NULL;
ALTER TABLE message ADD COLUMN legacy_marker TEXT;
ALTER TABLE message ADD COLUMN truncated INTEGER NOT NULL DEFAULT 0;
ALTER TABLE message ADD COLUMN stop_reason TEXT;
ALTER TABLE message_block ADD COLUMN artifact_id TEXT;
ALTER TABLE message_block ADD COLUMN truncated INTEGER NOT NULL DEFAULT 0;
ALTER TABLE run_event ADD COLUMN turn_id TEXT;
ALTER TABLE run_event ADD COLUMN message_id TEXT;
ALTER TABLE context_snapshot ADD COLUMN conversation_id TEXT;
ALTER TABLE context_snapshot ADD COLUMN branch_id TEXT;
ALTER TABLE context_snapshot ADD COLUMN turn_id TEXT;
ALTER TABLE context_snapshot ADD COLUMN source_revision INTEGER NOT NULL DEFAULT 0;
ALTER TABLE context_snapshot ADD COLUMN input_message_ids TEXT NOT NULL DEFAULT '[]';
ALTER TABLE context_snapshot ADD COLUMN summary_message_id TEXT;
ALTER TABLE context_snapshot ADD COLUMN replaced_range TEXT;
ALTER TABLE context_snapshot ADD COLUMN algorithm_version TEXT NOT NULL DEFAULT 'mechanical-v1';
ALTER TABLE context_snapshot ADD COLUMN provider_context_window INTEGER;
ALTER TABLE context_snapshot ADD COLUMN artifact_reference TEXT;

ALTER TABLE prompt_queue ADD COLUMN kind TEXT NOT NULL DEFAULT 'follow_up';
ALTER TABLE prompt_queue ADD COLUMN drain_mode TEXT NOT NULL DEFAULT 'all';
ALTER TABLE prompt_queue ADD COLUMN lease_token TEXT;
ALTER TABLE prompt_queue ADD COLUMN lease_run_id TEXT;
ALTER TABLE prompt_queue ADD COLUMN leased_at TEXT;
ALTER TABLE prompt_queue ADD COLUMN consumed_turn_id TEXT;
CREATE INDEX IF NOT EXISTS idx_prompt_queue_lease
    ON prompt_queue(conversation_id, status, lease_run_id, position);

ALTER TABLE checkpoint ADD COLUMN turn_id TEXT;
ALTER TABLE checkpoint ADD COLUMN active_context_snapshot_id TEXT;
ALTER TABLE checkpoint ADD COLUMN side_effect_ledger_cursor TEXT;
ALTER TABLE side_effect_record ADD COLUMN turn_id TEXT;
ALTER TABLE side_effect_record ADD COLUMN side_effect_class TEXT;
ALTER TABLE side_effect_record ADD COLUMN status TEXT NOT NULL DEFAULT 'planned'
    CHECK(status IN ('planned','started','completed','failed','cancelled','uncertain'));
ALTER TABLE side_effect_record ADD COLUMN replay_safe INTEGER NOT NULL DEFAULT 0;
ALTER TABLE side_effect_record ADD COLUMN idempotency_key TEXT;
ALTER TABLE side_effect_record ADD COLUMN external_reference TEXT;
CREATE INDEX IF NOT EXISTS idx_side_effect_run_status
    ON side_effect_record(run_id, status, created_at);

CREATE TABLE IF NOT EXISTS resume_plan (
    id TEXT PRIMARY KEY,
    source_run_id TEXT NOT NULL REFERENCES run(id) ON DELETE CASCADE,
    new_run_id TEXT REFERENCES run(id) ON DELETE SET NULL,
    action TEXT NOT NULL,
    checkpoint_id TEXT,
    status TEXT NOT NULL DEFAULT 'planned'
        CHECK(status IN ('planned','approved','blocked','executed')),
    unresolved_effects_json TEXT NOT NULL DEFAULT '[]',
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    resolved_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_resume_plan_source ON resume_plan(source_run_id, created_at DESC);
";

pub(crate) const MIGRATION_029: &str = "
-- Subagent route-restart scope: the durable session must carry the original
-- child scope (project identity, permission ceiling, profile, step budget,
-- tool allowlist) so a route switch restores them exactly instead of guessing
-- ask / max_steps=15 and dropping project identity.
ALTER TABLE subagent_session ADD COLUMN project_path TEXT;
ALTER TABLE subagent_session ADD COLUMN project_id TEXT;
ALTER TABLE subagent_session ADD COLUMN project_identity_version INTEGER;
ALTER TABLE subagent_session ADD COLUMN permission_profile TEXT;
ALTER TABLE subagent_session ADD COLUMN agent_profile_id TEXT;
ALTER TABLE subagent_session ADD COLUMN max_steps INTEGER;
ALTER TABLE subagent_session ADD COLUMN tool_allowlist_json TEXT;
";

/// Migration 030 (TASK-004 / G01): per-run ledger sequence so the checkpoint
/// cursor can store a real side-effect watermark instead of the last event
/// sequence. Existing rows keep a NULL sequence (legacy) and are never claimed
/// safe; `continue_run` already fails closed without a ledger watermark.
pub(crate) const MIGRATION_030: &str = "
ALTER TABLE side_effect_record ADD COLUMN ledger_sequence INTEGER;
CREATE INDEX IF NOT EXISTS idx_side_effect_ledger_sequence ON side_effect_record(run_id, ledger_sequence);
";

/// Migration 031: D03/G01 — add the per-row `replay_contract` and backfill it.
///
/// New intents populate `replay_contract` at write time
/// (`side_effect_ledger::replay_contract_for`). Rows already in the table get:
/// - `legacy_unverifiable` when they predate `ledger_sequence` (MIGRATION_030),
///   so they are never claimed safe — their ledger prefix is unprovable;
/// - `never` for checkpoint-covered workspace files;
/// - `confirm` for everything else (external outcome unknown without the
///   original handler).
pub(crate) const MIGRATION_031: &str = "
ALTER TABLE side_effect_record ADD COLUMN replay_contract TEXT;
UPDATE side_effect_record
   SET replay_contract = CASE
       WHEN ledger_sequence IS NULL THEN 'legacy_unverifiable'
       WHEN side_effect_class = 'workspace_file' THEN 'never'
       ELSE 'confirm'
   END;
";

/// Migration 032: TASK-005 (B03) — idempotent conversation projection.
///
/// `projection_watermark` tracks how far a run's committed turns have been
/// projected into the conversation tables; `projection_quarantine` records
/// events/turns the projector explicitly isolated instead of silently
/// skipping or overwriting.
pub(crate) const MIGRATION_032: &str = "
CREATE TABLE IF NOT EXISTS projection_watermark (
    projector TEXT NOT NULL,
    run_id TEXT NOT NULL REFERENCES run(id) ON DELETE CASCADE,
    event_sequence INTEGER NOT NULL,
    turn_count INTEGER NOT NULL DEFAULT 0,
    digest TEXT NOT NULL,
    compat_hits INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (projector, run_id)
);
CREATE INDEX IF NOT EXISTS idx_projection_watermark_run ON projection_watermark(projector, run_id);

CREATE TABLE IF NOT EXISTS projection_quarantine (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    projector TEXT NOT NULL,
    run_id TEXT NOT NULL,
    turn_id TEXT,
    message_id TEXT,
    reason TEXT NOT NULL,
    detail TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
";

/// Migration 033: TASK-008 (B05) — permission decision outbox.
///
/// A resolved decision writes its authoritative event delivery intent here in
/// the SAME transaction as the `interaction` row; the RPC/delivery step then
/// appends the run event and wakes the waiter, and marks the row delivered. An
/// undelivered row is replayed at daemon start, so a decision is never lost
/// and the waiter is never woken ahead of its durable event.
pub(crate) const MIGRATION_033: &str = "
CREATE TABLE IF NOT EXISTS interaction_outbox (
    id TEXT PRIMARY KEY,
    interaction_id TEXT NOT NULL,
    run_id TEXT,
    conversation_id TEXT,
    kind TEXT NOT NULL,
    response TEXT NOT NULL,
    delivered INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_interaction_outbox_delivered ON interaction_outbox(delivered);
";

/// T02 (G01): make ledger sequences unique per run so concurrent tool effects
/// can never collide on the same watermark. The per-run allocation serializes
/// on the DataStore connection mutex; this partial unique index backstops any
/// cross-connection race while leaving legacy NULL-sequence rows (already
/// `legacy_unverifiable`) exempt.
pub(crate) const MIGRATION_034: &str = "
CREATE UNIQUE INDEX IF NOT EXISTS idx_side_effect_run_sequence
    ON side_effect_record(run_id, ledger_sequence)
    WHERE ledger_sequence IS NOT NULL;
";

/// Migration 035: Creative proposal facts (T06).
///
/// When a `creative_proposal` tool call completes, the Daemon persists a typed
/// proposal fact here — durable across restart, so the Host can pull pending
/// facts over UDS and rebuild its approval inbox even after the daemon or the
/// Host restarted. `proposal_id` is the stable Daemon-generated id (never
/// agent-supplied); `status` tracks the fact lifecycle (pending/approved//// rejected/expired/failed) so a fact is never re-served after it is decided.
pub(crate) const MIGRATION_035: &str = "
CREATE TABLE IF NOT EXISTS creative_proposal_fact (
    proposal_id TEXT PRIMARY KEY,
    envelope_version INTEGER NOT NULL,
    run_id TEXT NOT NULL,
    turn_id TEXT,
    tool_call_id TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_proposal_fact_status ON creative_proposal_fact(status);
";

/// Migration 036 (T05): durable subagent budget / reservation ledger.
///
/// Reservation lifecycle, used tokens/cost, max budgets, failure policy,
/// retry accounting, and the spawn-time scope snapshot all live on the
/// `subagent_session` row so a daemon restart restores them exactly. The
/// `reservation_released` flag makes slot release exactly-once (a terminal
/// child or a restart marks it, never twice). `scope_snapshot_json` is the
/// full spawn-time scope for tightening checks on route restarts.
pub(crate) const MIGRATION_036: &str = "
ALTER TABLE subagent_session ADD COLUMN tokens_used INTEGER NOT NULL DEFAULT 0;
ALTER TABLE subagent_session ADD COLUMN cost_usd REAL NOT NULL DEFAULT 0;
ALTER TABLE subagent_session ADD COLUMN max_tokens INTEGER;
ALTER TABLE subagent_session ADD COLUMN max_cost_usd REAL;
ALTER TABLE subagent_session ADD COLUMN failure_policy TEXT NOT NULL DEFAULT 'isolate';
ALTER TABLE subagent_session ADD COLUMN max_retries INTEGER NOT NULL DEFAULT 0;
ALTER TABLE subagent_session ADD COLUMN retry_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE subagent_session ADD COLUMN reservation_released INTEGER NOT NULL DEFAULT 0;
ALTER TABLE subagent_session ADD COLUMN reserved_at TEXT;
ALTER TABLE subagent_session ADD COLUMN released_at TEXT;
ALTER TABLE subagent_session ADD COLUMN tree_root_run_id TEXT;
ALTER TABLE subagent_session ADD COLUMN depth INTEGER NOT NULL DEFAULT 0;
ALTER TABLE subagent_session ADD COLUMN scope_snapshot_json TEXT NOT NULL DEFAULT '{}';
CREATE INDEX IF NOT EXISTS idx_subagent_session_reservation
    ON subagent_session(parent_run_id, reservation_released);
CREATE INDEX IF NOT EXISTS idx_subagent_session_tree_usage
    ON subagent_session(tree_root_run_id, reservation_released);
";

/// Migration 037: fold the previously untracked `ensure_run_metadata_columns`
/// repair into a versioned migration (P0-023/P0-024). All statements are
/// idempotent ALTERs; the runner tolerates duplicate-column on re-entry and
/// fails closed only when the postcondition (`run.parent_run_id`) is absent
/// after apply.
pub(crate) const MIGRATION_037: &str = "
ALTER TABLE run ADD COLUMN parent_run_id TEXT;
ALTER TABLE run ADD COLUMN agent_profile_id TEXT;
ALTER TABLE run ADD COLUMN key_id TEXT;
ALTER TABLE run ADD COLUMN permission_profile TEXT NOT NULL DEFAULT 'ask';
ALTER TABLE run ADD COLUMN project_path TEXT;
ALTER TABLE run ADD COLUMN retry_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE run ADD COLUMN idempotency_key TEXT;
ALTER TABLE run ADD COLUMN retry_of_run_id TEXT;
ALTER TABLE run ADD COLUMN retry_of_turn_id TEXT;
ALTER TABLE run ADD COLUMN continued_from_run_id TEXT;
ALTER TABLE run ADD COLUMN branch_id TEXT;
ALTER TABLE run ADD COLUMN branch_parent_message_id TEXT;
ALTER TABLE run ADD COLUMN checkpoint_id TEXT;
ALTER TABLE run ADD COLUMN resume_of_run_id TEXT;
CREATE INDEX IF NOT EXISTS idx_run_idempotency_key ON run(idempotency_key);
ALTER TABLE conversation ADD COLUMN branch_id TEXT;
ALTER TABLE conversation ADD COLUMN parent_conversation_id TEXT;
ALTER TABLE conversation ADD COLUMN branch_parent_message_id TEXT;
ALTER TABLE side_effect_record ADD COLUMN resource TEXT;
ALTER TABLE side_effect_record ADD COLUMN started_at TEXT;
ALTER TABLE side_effect_record ADD COLUMN completed_at TEXT;
ALTER TABLE resume_plan ADD COLUMN decision TEXT NOT NULL DEFAULT 'RequiresUserConfirmation';
";

/// Migration 038: creative draft tables move under the Daemon-authoritative
/// assistant.db (W3 P0-2). The capability gateway previously opened the
/// Host-owned natives.db directly; the draft metadata now lives in the daemon
/// store so no daemon process path touches the Host database. Idempotent.
pub(crate) const MIGRATION_038: &str = "
CREATE TABLE IF NOT EXISTS creative_drafts (
    draft_id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    intent TEXT NOT NULL,
    conversation_id TEXT,
    origin_module_id TEXT,
    current_revision INTEGER NOT NULL DEFAULT 0,
    state TEXT NOT NULL DEFAULT 'drafting'
        CHECK(state IN ('drafting','generating','ready','publishing','published','archived')),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS creative_draft_revisions (
    draft_id TEXT NOT NULL REFERENCES creative_drafts(draft_id) ON DELETE CASCADE,
    revision INTEGER NOT NULL,
    content_hash TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (draft_id, revision)
);
";

/// 039 — 审计收口 #9：唯一 global Harness。
/// 历史 project/session bindings 标记 inactive（数据保留只读，不 DROP），
/// 运行时解析只使用 current global（resolve_layers 已收口）。幂等：重入时
/// ALTER 存在性检查 + UPDATE 仅命中未标记行。
pub(crate) const MIGRATION_039: &str = "
ALTER TABLE harness_binding ADD COLUMN inactive_at TEXT;
UPDATE harness_binding SET inactive_at = datetime('now')
 WHERE scope_type IN ('project','session') AND inactive_at IS NULL;
";

/// Migration 040: persist the complete run execution and project identity.
/// Nullable columns preserve compatibility with already-created runs.
pub(crate) const MIGRATION_040: &str = "
ALTER TABLE run ADD COLUMN project_identity_version INTEGER;
ALTER TABLE run ADD COLUMN effort TEXT;
ALTER TABLE run ADD COLUMN runtime_id TEXT;
";
