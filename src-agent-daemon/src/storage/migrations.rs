//! Database migrations for the Agent Daemon.
//!
//! Migrations are versioned, sequential, and idempotent.
//! Each migration is a complete SQL string that can be executed as a batch.

/// All migrations: (version, SQL) tuples, ordered by version.
pub const ALL: &[(i64, &str)] = &[
    (1, MIGRATION_001),
    (2, MIGRATION_002),
    (3, MIGRATION_003),
    (4, MIGRATION_004),
    (5, MIGRATION_005),
    (6, MIGRATION_006),
    (7, MIGRATION_007),
    (8, MIGRATION_008),
    (9, MIGRATION_009),
    (10, MIGRATION_010),
    (11, MIGRATION_011),
    (12, MIGRATION_012),
    (13, MIGRATION_013),
    (14, MIGRATION_014),
    (15, MIGRATION_015),
    (16, MIGRATION_016),
    (17, MIGRATION_017),
    (18, MIGRATION_018),
    (19, MIGRATION_019),
    (20, MIGRATION_020),
    // 021 belongs to the capability-library workstream and lands on its own
    // branch. Harness starts at 022 so the two never claim the same number.
    (22, MIGRATION_022),
];

/// Migration 001: Core schema — conversations, messages, runs, events.
///
/// Note: Daemon schema progress is tracked in `_daemon_schema_version`
/// (see `DataStore::run_migrations`). Host continues to own `_schema_version`
/// for `assistant_*` tables on the same file. Do not reintroduce a shared
/// version table here.
const MIGRATION_001: &str = "
CREATE TABLE IF NOT EXISTS conversation (
    id TEXT PRIMARY KEY,
    mode TEXT NOT NULL DEFAULT 'chat' CHECK(mode IN ('chat', 'agent', 'goal')),
    project_id TEXT,
    title TEXT NOT NULL DEFAULT '',
    provider_id TEXT NOT NULL,
    model_id TEXT NOT NULL,
    permission_profile_id TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    archived_at TEXT
);

CREATE TABLE IF NOT EXISTS message (
    id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL REFERENCES conversation(id) ON DELETE CASCADE,
    parent_message_id TEXT REFERENCES message(id) ON DELETE SET NULL,
    role TEXT NOT NULL CHECK(role IN ('system', 'user', 'assistant')),
    status TEXT NOT NULL DEFAULT 'complete' CHECK(status IN ('sending', 'streaming', 'complete', 'failed', 'interrupted')),
    input_tokens INTEGER,
    output_tokens INTEGER,
    reasoning_tokens INTEGER,
    cost_usd REAL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS message_block (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    message_id TEXT NOT NULL REFERENCES message(id) ON DELETE CASCADE,
    sort_order INTEGER NOT NULL DEFAULT 0,
    block_type TEXT NOT NULL,
    block_json TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS run (
    id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL REFERENCES conversation(id) ON DELETE CASCADE,
    status TEXT NOT NULL DEFAULT 'queued' CHECK(status IN (
        'created', 'queued', 'preparing', 'running', 'waiting_permission',
        'waiting_subagent', 'cancelling', 'completed', 'failed', 'cancelled', 'interrupted'
    )),
    trigger_message_id TEXT REFERENCES message(id) ON DELETE SET NULL,
    provider_id TEXT NOT NULL,
    model_id TEXT NOT NULL,
    started_at TEXT,
    finished_at TEXT,
    error_code TEXT,
    step_count INTEGER DEFAULT 0,
    max_steps INTEGER DEFAULT 50,
    token_budget INTEGER,
    total_input_tokens INTEGER DEFAULT 0,
    total_output_tokens INTEGER DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS run_event (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id TEXT NOT NULL REFERENCES run(id) ON DELETE CASCADE,
    sequence INTEGER NOT NULL,
    event_type TEXT NOT NULL,
    payload TEXT NOT NULL,
    timestamp TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(run_id, sequence)
);

CREATE INDEX IF NOT EXISTS idx_run_event_run_sequence ON run_event(run_id, sequence);
";

/// Migration 002: Tool calls, permissions, and artifacts.
const MIGRATION_002: &str = "
CREATE TABLE IF NOT EXISTS tool_call (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES run(id) ON DELETE CASCADE,
    parent_tool_call_id TEXT REFERENCES tool_call(id) ON DELETE SET NULL,
    name TEXT NOT NULL,
    input TEXT NOT NULL,
    output TEXT,
    status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN (
        'pending', 'running', 'completed', 'failed', 'rejected'
    )),
    is_error INTEGER NOT NULL DEFAULT 0,
    duration_ms INTEGER,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS permission_request (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES run(id) ON DELETE CASCADE,
    tool_call_id TEXT NOT NULL REFERENCES tool_call(id) ON DELETE CASCADE,
    tool_name TEXT NOT NULL,
    reason TEXT NOT NULL,
    input TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending', 'approved', 'rejected', 'expired')),
    scope TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    responded_at TEXT
);

CREATE TABLE IF NOT EXISTS artifact (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES run(id) ON DELETE CASCADE,
    conversation_id TEXT NOT NULL REFERENCES conversation(id) ON DELETE CASCADE,
    source_tool TEXT NOT NULL,
    path TEXT NOT NULL,
    sha256 TEXT NOT NULL,
    size INTEGER NOT NULL DEFAULT 0,
    mime_type TEXT NOT NULL DEFAULT 'application/octet-stream',
    label TEXT,
    kind TEXT NOT NULL DEFAULT 'file' CHECK(kind IN (
        'file', 'patch', 'report', 'screenshot', 'export', 'structured_result', 'error'
    )),
    local_path TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
";

/// Migration 003: Context snapshots and provider configuration.
const MIGRATION_003: &str = "
CREATE TABLE IF NOT EXISTS context_snapshot (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES run(id) ON DELETE CASCADE,
    sequence INTEGER NOT NULL,
    snapshot_type TEXT NOT NULL,
    token_count INTEGER NOT NULL DEFAULT 0,
    summary TEXT,
    snapshot_json TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS provider (
    id TEXT PRIMARY KEY,
    provider_type TEXT NOT NULL CHECK(provider_type IN (
        'openai', 'anthropic', 'gemini', 'deepseek', 'openai_compatible', 'ollama'
    )),
    display_name TEXT NOT NULL,
    api_base_url TEXT NOT NULL,
    organization_id TEXT,
    project_id TEXT,
    proxy_url TEXT,
    timeout_secs INTEGER DEFAULT 60,
    default_model TEXT,
    health_status TEXT NOT NULL DEFAULT 'unknown' CHECK(health_status IN (
        'unknown', 'verified', 'unverified'
    )),
    last_test_at TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS provider_key (
    id TEXT PRIMARY KEY,
    provider_id TEXT NOT NULL REFERENCES provider(id) ON DELETE CASCADE,
    masked_key TEXT NOT NULL,
    label TEXT,
    is_active INTEGER NOT NULL DEFAULT 1,
    last_test_at TEXT,
    last_test_ok INTEGER,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS model_cache (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    provider_id TEXT NOT NULL REFERENCES provider(id) ON DELETE CASCADE,
    model_id TEXT NOT NULL,
    display_name TEXT,
    context_window INTEGER NOT NULL DEFAULT 4096,
    max_output INTEGER NOT NULL DEFAULT 4096,
    capabilities_json TEXT NOT NULL DEFAULT '{}',
    source TEXT NOT NULL DEFAULT 'api_discovery' CHECK(source IN (
        'api_discovery', 'cache', 'preset', 'manual'
    )),
    discovered_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(provider_id, model_id)
);
";

/// Migration 004: Extensions and permissions.
const MIGRATION_004: &str = "
CREATE TABLE IF NOT EXISTS extension (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    version TEXT NOT NULL DEFAULT '0.1.0',
    kind TEXT NOT NULL CHECK(kind IN (
        'plugin', 'mcp_server', 'skill', 'hook', 'command'
    )),
    enabled INTEGER NOT NULL DEFAULT 1,
    description TEXT,
    manifest TEXT NOT NULL DEFAULT '{}',
    health TEXT NOT NULL DEFAULT 'healthy' CHECK(health IN (
        'healthy', 'degraded', 'offline'
    )),
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS extension_permission (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    extension_id TEXT NOT NULL REFERENCES extension(id) ON DELETE CASCADE,
    permission TEXT NOT NULL,
    granted INTEGER NOT NULL DEFAULT 0,
    UNIQUE(extension_id, permission)
);

CREATE TABLE IF NOT EXISTS mcp_server_config (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    transport TEXT NOT NULL DEFAULT 'stdio' CHECK(transport IN ('stdio', 'http_sse')),
    command TEXT,
    args TEXT NOT NULL DEFAULT '[]',
    env_vars TEXT NOT NULL DEFAULT '[]',
    url TEXT,
    enabled INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS hook_registration (
    id TEXT PRIMARY KEY,
    hook_point TEXT NOT NULL CHECK(hook_point IN (
        'before_run', 'after_run', 'before_prompt', 'after_prompt',
        'before_tool_call', 'after_tool_call', 'before_permission',
        'after_permission', 'on_completion', 'on_error'
    )),
    priority INTEGER NOT NULL DEFAULT 0,
    handler TEXT NOT NULL,
    timeout_ms INTEGER NOT NULL DEFAULT 5000,
    fail_strategy TEXT NOT NULL DEFAULT 'skip' CHECK(fail_strategy IN ('fail', 'skip', 'default')),
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
";

/// Migration 005: Indexes and performance optimizations.
const MIGRATION_005: &str = "
CREATE INDEX IF NOT EXISTS idx_message_conversation ON message(conversation_id, created_at);
CREATE INDEX IF NOT EXISTS idx_run_conversation ON run(conversation_id, created_at);
CREATE INDEX IF NOT EXISTS idx_tool_call_run ON tool_call(run_id);
CREATE INDEX IF NOT EXISTS idx_permission_request_run ON permission_request(run_id);
CREATE INDEX IF NOT EXISTS idx_artifact_run ON artifact(run_id);
CREATE INDEX IF NOT EXISTS idx_artifact_conversation ON artifact(conversation_id);
CREATE INDEX IF NOT EXISTS idx_context_snapshot_run ON context_snapshot(run_id, sequence);
CREATE INDEX IF NOT EXISTS idx_provider_key_provider ON provider_key(provider_id);
CREATE INDEX IF NOT EXISTS idx_model_cache_provider ON model_cache(provider_id);
CREATE INDEX IF NOT EXISTS idx_extension_kind ON extension(kind);
CREATE INDEX IF NOT EXISTS idx_hook_registration_point ON hook_registration(hook_point);
";

/// Migration 006: Align persisted run status CHECK with Protocol v2.
const MIGRATION_006: &str = "
CREATE INDEX IF NOT EXISTS idx_run_conversation ON run(conversation_id, created_at);
CREATE INDEX IF NOT EXISTS idx_run_status ON run(status);
";

const MIGRATION_007: &str = "SELECT 1;";

const MIGRATION_008: &str = "
-- conversation.mode may already allow goal via prior rebuilds; ensure index only.
CREATE INDEX IF NOT EXISTS idx_conversation_updated ON conversation(updated_at);
";

const MIGRATION_009: &str = "
CREATE TABLE IF NOT EXISTS prompt_queue (
    id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL REFERENCES conversation(id) ON DELETE CASCADE,
    content TEXT NOT NULL,
    source TEXT NOT NULL DEFAULT 'user',
    attachments TEXT,
    position INTEGER NOT NULL DEFAULT 0,
    client_temp_id TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_prompt_queue_conversation
    ON prompt_queue(conversation_id, position);

CREATE TABLE IF NOT EXISTS interaction (
    id TEXT PRIMARY KEY,
    run_id TEXT REFERENCES run(id) ON DELETE CASCADE,
    conversation_id TEXT REFERENCES conversation(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN (
        'pending', 'resolved', 'expired', 'cancelled'
    )),
    payload TEXT NOT NULL DEFAULT '{}',
    response TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    responded_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_interaction_run ON interaction(run_id);
CREATE INDEX IF NOT EXISTS idx_interaction_status ON interaction(status);

CREATE TABLE IF NOT EXISTS task (
    id TEXT PRIMARY KEY,
    conversation_id TEXT REFERENCES conversation(id) ON DELETE CASCADE,
    parent_run_id TEXT REFERENCES run(id) ON DELETE SET NULL,
    agent_profile_id TEXT,
    status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN (
        'pending', 'running', 'completed', 'failed', 'cancelled', 'interrupted'
    )),
    title TEXT NOT NULL DEFAULT '',
    input TEXT,
    result TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    finished_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_task_conversation ON task(conversation_id, created_at);

CREATE TABLE IF NOT EXISTS checkpoint (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES run(id) ON DELETE CASCADE,
    conversation_id TEXT NOT NULL REFERENCES conversation(id) ON DELETE CASCADE,
    sequence INTEGER NOT NULL DEFAULT 0,
    label TEXT,
    snapshot_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_checkpoint_run ON checkpoint(run_id, sequence);

CREATE TABLE IF NOT EXISTS tool_grant (
    id TEXT PRIMARY KEY,
    conversation_id TEXT REFERENCES conversation(id) ON DELETE CASCADE,
    run_id TEXT REFERENCES run(id) ON DELETE CASCADE,
    tool_name TEXT NOT NULL,
    scope TEXT,
    grant_type TEXT NOT NULL DEFAULT 'once' CHECK(grant_type IN (
        'once', 'session', 'always'
    )),
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    expires_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_tool_grant_conversation
    ON tool_grant(conversation_id, tool_name);

CREATE TABLE IF NOT EXISTS _host_authority_migration (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    version INTEGER NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('completed', 'failed')),
    detail TEXT,
    applied_at TEXT NOT NULL DEFAULT (datetime('now'))
);
";

/// Migration 010: hidden subagent conversations + route policy + session registry.
/// Stores only provider/key/model *IDs* — never plaintext credentials.
const MIGRATION_010: &str = "
ALTER TABLE conversation ADD COLUMN parent_conversation_id TEXT
    REFERENCES conversation(id) ON DELETE CASCADE;
CREATE INDEX IF NOT EXISTS idx_conversation_parent
    ON conversation(parent_conversation_id);

CREATE TABLE IF NOT EXISTS subagent_route_policy (
    parent_conversation_id TEXT PRIMARY KEY
        REFERENCES conversation(id) ON DELETE CASCADE,
    mode TEXT NOT NULL DEFAULT 'default' CHECK(mode IN ('default', 'random', 'custom')),
    bindings_json TEXT NOT NULL DEFAULT '[]',
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS subagent_session (
    id TEXT PRIMARY KEY,
    parent_conversation_id TEXT NOT NULL
        REFERENCES conversation(id) ON DELETE CASCADE,
    child_conversation_id TEXT NOT NULL
        REFERENCES conversation(id) ON DELETE CASCADE,
    parent_run_id TEXT,
    task_call_id TEXT,
    name TEXT NOT NULL DEFAULT '',
    task TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'open' CHECK(status IN (
        'open', 'running', 'idle', 'closed', 'failed', 'cancelled'
    )),
    provider_id TEXT NOT NULL,
    key_id TEXT NOT NULL,
    model_id TEXT NOT NULL,
    attempted_bindings_json TEXT NOT NULL DEFAULT '[]',
    last_activity_at TEXT NOT NULL DEFAULT (datetime('now')),
    closed_at TEXT,
    error TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_subagent_session_parent
    ON subagent_session(parent_conversation_id, created_at);
CREATE INDEX IF NOT EXISTS idx_subagent_session_parent_run
    ON subagent_session(parent_run_id);
CREATE INDEX IF NOT EXISTS idx_subagent_session_activity
    ON subagent_session(status, last_activity_at);
";

/// Migration 011: subagent status vocabulary + parent heartbeat.
///
/// - Allow `completed` (success terminal; `idle` kept for legacy rows).
/// - Expand intermediate statuses used by assignment / resume.
/// - Track parent conversation heartbeat independently of child activity.
const MIGRATION_011: &str = "
PRAGMA foreign_keys=OFF;
PRAGMA legacy_alter_table=ON;

CREATE TABLE subagent_session_new (
    id TEXT PRIMARY KEY,
    parent_conversation_id TEXT NOT NULL
        REFERENCES conversation(id) ON DELETE CASCADE,
    child_conversation_id TEXT NOT NULL
        REFERENCES conversation(id) ON DELETE CASCADE,
    parent_run_id TEXT,
    task_call_id TEXT,
    name TEXT NOT NULL DEFAULT '',
    task TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'open' CHECK(status IN (
        'pending_assignment', 'open', 'queued', 'running', 'waiting',
        'completed', 'idle', 'failed', 'cancelled', 'interrupted', 'closed'
    )),
    provider_id TEXT NOT NULL,
    key_id TEXT NOT NULL,
    model_id TEXT NOT NULL,
    attempted_bindings_json TEXT NOT NULL DEFAULT '[]',
    last_activity_at TEXT NOT NULL DEFAULT (datetime('now')),
    closed_at TEXT,
    error TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
INSERT INTO subagent_session_new
    SELECT id, parent_conversation_id, child_conversation_id, parent_run_id, task_call_id,
           name, task, status, provider_id, key_id, model_id, attempted_bindings_json,
           last_activity_at, closed_at, error, created_at, updated_at
    FROM subagent_session;
DROP TABLE subagent_session;
ALTER TABLE subagent_session_new RENAME TO subagent_session;
CREATE INDEX IF NOT EXISTS idx_subagent_session_parent
    ON subagent_session(parent_conversation_id, created_at);
CREATE INDEX IF NOT EXISTS idx_subagent_session_parent_run
    ON subagent_session(parent_run_id);
CREATE INDEX IF NOT EXISTS idx_subagent_session_activity
    ON subagent_session(status, last_activity_at);

ALTER TABLE subagent_route_policy
    ADD COLUMN last_parent_heartbeat_at TEXT;

PRAGMA legacy_alter_table=OFF;
PRAGMA foreign_keys=ON;
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
const MIGRATION_022: &str = "
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
