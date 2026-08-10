//! Migration SQL constants (W9 split from storage/migrations.rs).
//!
//! The ordered registry `migrations::ALL` in `migrations.rs` remains the single
//! source of truth; this file only holds the versioned SQL strings for its
//! domain. Editing an applied migration still fails closed via checksum.

const MIGRATION_011: &str = "
ALTER TABLE subagent_route_policy
    ADD COLUMN last_parent_heartbeat_at TEXT;
";

/// Migration 012: SessionCoordinator durable state.
///
/// - `session_actor` stores per-conversation coordination fields that must
///   survive daemon restart (pending interjection, cancel-and-send target,
///   pending interaction, drain policy, version).
/// - `prompt_queue.status` makes queue item lifecycle durable so recovery
///   never re-executes a sent/running item as a silent duplicate.
const MIGRATION_012: &str = "
CREATE TABLE IF NOT EXISTS session_actor (
    conversation_id TEXT PRIMARY KEY
        REFERENCES conversation(id) ON DELETE CASCADE,
    active_run_id TEXT,
    running_prompt_id TEXT,
    pending_interjection TEXT,
    pending_interaction_id TEXT,
    cancel_and_send_id TEXT,
    cancel_requested INTEGER NOT NULL DEFAULT 0,
    drain_on_finish INTEGER NOT NULL DEFAULT 1,
    version INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

ALTER TABLE prompt_queue ADD COLUMN status TEXT NOT NULL DEFAULT 'queued';

CREATE INDEX IF NOT EXISTS idx_prompt_queue_status
    ON prompt_queue(conversation_id, status, position);
CREATE INDEX IF NOT EXISTS idx_session_actor_updated
    ON session_actor(updated_at);
";

/// Migration 013: Run revision for CAS commits (task-02).
///
/// Every status transition increments `revision`. `RunManager::commit_transition`
/// updates with `WHERE id=? AND revision=?` so late outcomes cannot overwrite
/// Cancelling/Cancelled/other terminals.
const MIGRATION_013: &str = "
ALTER TABLE run ADD COLUMN revision INTEGER NOT NULL DEFAULT 0;
";

/// Migration 014: Event identity + dual sequences (task-08).
///
/// - `event_id` stable UUID/ULID idempotency key
/// - `run_event.id` remains `global_sequence` (AUTOINCREMENT)
/// - existing `sequence` column is the per-run sequence (`run_sequence` on wire)
const MIGRATION_014: &str = "
ALTER TABLE run_event ADD COLUMN event_id TEXT;
UPDATE run_event
   SET event_id = 'legacy:' || run_id || ':' || sequence
 WHERE event_id IS NULL OR event_id = '';
CREATE UNIQUE INDEX IF NOT EXISTS idx_run_event_event_id ON run_event(event_id);
";

/// Migration 015: Stable ProjectIdentity (task-10).
///
/// Paths are attributes. Runs/conversations gain `project_id` UUID column;
/// `project_path` remains a diagnostic snapshot.
const MIGRATION_015: &str = "
CREATE TABLE IF NOT EXISTS project_identity (
    project_id TEXT PRIMARY KEY,
    canonical_path TEXT NOT NULL,
    filesystem_fingerprint TEXT NOT NULL,
    identity_version INTEGER NOT NULL DEFAULT 1,
    verified_at INTEGER NOT NULL DEFAULT 0,
    orphaned INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_project_identity_path
    ON project_identity(canonical_path)
    WHERE orphaned = 0;

ALTER TABLE run ADD COLUMN project_id TEXT;
ALTER TABLE conversation ADD COLUMN project_identity_id TEXT;
";

/// Migration 018: Side-effect ledger for workspace restore / rewind semantics (task-07).
///
/// Minimal durable records of tool side-effects. Not a universal transaction
/// framework — only tracks what restore/preview can honestly claim.
const MIGRATION_018: &str = "
CREATE TABLE IF NOT EXISTS side_effect_record (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL,
    tool_call_id TEXT,
    category TEXT NOT NULL CHECK(category IN (
        'workspace_file', 'database', 'process', 'network', 'git', 'mcp', 'external'
    )),
    target_summary TEXT NOT NULL DEFAULT '',
    reversible INTEGER NOT NULL DEFAULT 0,
    compensation_id TEXT,
    checkpoint_id TEXT,
    artifact_id TEXT,
    coverage_note TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_side_effect_run ON side_effect_record(run_id, created_at);
";

/// Migration 016 (Agent B / task-09): structured tool grants.
///
/// Legacy coarse `tool_grant` rows keep policy_version=0 and are ignored for
/// reuse. New grants bind project identity, permission class, path/argument
/// constraints, session/run scope, expiry, and policy version.
const MIGRATION_016: &str = "
-- Mark existing coarse grants as legacy so they cannot auto-authorize.
UPDATE tool_grant SET scope = COALESCE(scope, '') WHERE 1=1;

CREATE TABLE IF NOT EXISTS tool_grant_v2 (
    id TEXT PRIMARY KEY,
    project_id TEXT,
    project_identity_version TEXT,
    project_fingerprint TEXT,
    tool_name TEXT NOT NULL,
    permission_class TEXT NOT NULL DEFAULT 'unknown',
    path_scope_json TEXT NOT NULL DEFAULT 'null',
    argument_constraint_json TEXT NOT NULL DEFAULT 'null',
    conversation_id TEXT,
    run_id TEXT,
    session_id TEXT,
    scope TEXT NOT NULL DEFAULT 'once' CHECK(scope IN ('once', 'this_run', 'session', 'project')),
    expires_at TEXT,
    policy_version INTEGER NOT NULL DEFAULT 1,
    created_by TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    revoked_at TEXT,
    -- Audit-only; never stores secrets (constraint summary / redacted pattern).
    constraint_summary TEXT
);
CREATE INDEX IF NOT EXISTS idx_tool_grant_v2_lookup
    ON tool_grant_v2(tool_name, project_id, policy_version);
CREATE INDEX IF NOT EXISTS idx_tool_grant_v2_conversation
    ON tool_grant_v2(conversation_id, tool_name);

-- Expire legacy coarse grants (policy_version 0 semantics via grant_type always + empty scope).
UPDATE tool_grant SET expires_at = datetime('now')
 WHERE expires_at IS NULL
   AND (scope IS NULL OR scope = '' OR grant_type = 'always');
";

/// Migration 017 (Agent B / task-11): durable subagent budget ledger snapshot.
///
/// Runtime reservations are still in-memory; this table records per-run budget
/// counters for restart Interrupted recovery and audit. Active children follow
/// parent Interrupted semantics (task-04/02) — no Future resume.
const MIGRATION_017: &str = "
CREATE TABLE IF NOT EXISTS subagent_budget_ledger (
    run_id TEXT PRIMARY KEY,
    parent_run_id TEXT,
    tree_root_run_id TEXT,
    depth INTEGER NOT NULL DEFAULT 0,
    concurrent_reserved INTEGER NOT NULL DEFAULT 0,
    tokens_used INTEGER NOT NULL DEFAULT 0,
    tool_calls_used INTEGER NOT NULL DEFAULT 0,
    max_tokens INTEGER,
    max_tool_calls INTEGER,
    failure_policy TEXT NOT NULL DEFAULT 'isolate',
    status TEXT NOT NULL DEFAULT 'active',
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_subagent_budget_parent
    ON subagent_budget_ledger(parent_run_id);
CREATE INDEX IF NOT EXISTS idx_subagent_budget_tree
    ON subagent_budget_ledger(tree_root_run_id);
";

/// Migration 019: stable pagination indexes for GUI snapshots.
const MIGRATION_019: &str = "
CREATE INDEX IF NOT EXISTS idx_conversation_updated_id
    ON conversation(updated_at DESC, id DESC);
CREATE INDEX IF NOT EXISTS idx_message_conversation_created_id
    ON message(conversation_id, created_at DESC, id DESC);
CREATE INDEX IF NOT EXISTS idx_message_block_message_sort
    ON message_block(message_id, sort_order ASC, id ASC);
";

/// Migration 020: durable provider-route circuit state. No credentials live here.
const MIGRATION_020: &str = "
CREATE TABLE IF NOT EXISTS provider_route_health (
    route_key TEXT PRIMARY KEY,
    consecutive_failures INTEGER NOT NULL DEFAULT 0,
    open_until_ms INTEGER,
    half_open_in_flight INTEGER NOT NULL DEFAULT 0,
    in_flight INTEGER NOT NULL DEFAULT 0,
    last_selected_at TEXT,
    last_error TEXT,
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
";

/// Migration 021: capability library (ADR-0016).
///
/// Authoritative storage for skills metadata, MCP connector configs, experts
/// and expert teams. Secrets NEVER live in this database: `env_json` values may
/// hold `secret:<id>` references resolved by the Host-side encrypted store,
/// and header validation rejects plaintext Authorization values at the RPC
/// boundary.
///
/// Also, the historical `DROP TABLE IF EXISTS mcp_server_config` was removed
/// during the DATA-001 remediation (R-D3 forbids DROP TABLE in active
/// migrations). `mcp_server_config` was created by migration 004 with zero
/// readers or writers ever shipped, and that CREATE was removed from 004 as
/// well — fresh databases never create the dead table, so no DROP is needed
/// anywhere. Databases that applied the old migrations already had it dropped
/// by the historical 021.
///
/// Merge hazard, recorded because the runner cannot detect it: 021 and 022 were
/// written on two parallel branches, and the Harness branch (022) ran first on
/// some development databases. `run_migrations` gates on `MAX(version)`, not on
/// row membership, so any database that recorded 22 before 021 existed will skip
/// 021 forever and never report an error — the `capability_*` tables simply are
/// not there, and every `capability.*` RPC then fails on a missing table.
/// Fresh databases and any database at version <= 20 are unaffected. Recovery on
/// an affected database is manual: `DELETE FROM _daemon_schema_version WHERE
/// version = 22;` and reopen (022 is `CREATE TABLE IF NOT EXISTS` throughout, so
/// re-running it is safe; the two `ALTER TABLE` statements below are not, which
/// is why 021 must never be re-run against a database that already applied it).
