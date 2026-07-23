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
