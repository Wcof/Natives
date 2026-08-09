//! Database migrations for the Agent Daemon.
//!
//! Migrations are versioned, sequential, and idempotent.
//! Each migration is a complete SQL string that can be executed as a batch.

use rusqlite::Connection;

/// All migrations: (version, SQL) tuples, ordered by version.
///
/// The order is load-bearing, not cosmetic. `DataStore::run_migrations` reads
/// `MAX(version)` from `_daemon_schema_version` **once**, then walks this slice
/// skipping every entry with `version <= current_version`. It does not test row
/// membership. So a version that is merged in *below* a version an existing
/// database has already recorded is skipped forever, silently — see the note on
/// [`MIGRATION_021`]. Always append with a strictly larger number, and keep this
/// slice sorted ascending.
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
    // 021 is the capability library (ADR-0016), 022 the Harness control plane.
    // Two parallel workstreams; the numbers were reserved so they never collide,
    // and both must stay present and ascending.
    (21, MIGRATION_021),
    (22, MIGRATION_022),
    (23, MIGRATION_023),
    (24, MIGRATION_024),
    (25, MIGRATION_025),
    (26, MIGRATION_026),
    (27, MIGRATION_027),
    (28, MIGRATION_028),
    (29, MIGRATION_029),
    (30, MIGRATION_030),
    (31, MIGRATION_031),
    (32, MIGRATION_032),
    (33, MIGRATION_033),
    (34, MIGRATION_034),
    (35, MIGRATION_035),
    (36, MIGRATION_036),
    // 37 folds the previously untracked `ensure_run_metadata_columns` repair
    // logic into a real, versioned migration so no column repair runs without
    // ledger state (P0-023/P0-024): partial-core DBs can no longer skip it.
    (37, MIGRATION_037),
];

/// FNV-1a 64-bit checksum (self-implemented; no new crate). Used to detect
/// applied-migration drift: once a migration is recorded in `_daemon_migrations`,
/// editing its canonical SQL fails closed on the next open.
pub fn checksum_hex(canonical: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in canonical.as_bytes() {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

/// Postcondition probe: `true` when the schema objects a migration is
/// responsible for already exist. Used to (a) repair partial/crash states and
/// (b) adopt migrations that were applied by an older runner but never
/// recorded in the ledger. Conservative: probe only the *key* object of each
/// version; the canonical SQL is idempotent (`IF NOT EXISTS` / tolerant
/// ALTERs), so a `false` here followed by `apply` is safe to re-run.
fn postcondition_satisfied(conn: &Connection, version: i64) -> bool {
    let table_exists = |name: &str| -> bool {
        conn.query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name=?1",
            rusqlite::params![name],
            |row| row.get::<_, bool>(0),
        )
        .unwrap_or(false)
    };
    let index_exists = |name: &str| -> bool {
        conn.query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='index' AND name=?1",
            rusqlite::params![name],
            |row| row.get::<_, bool>(0),
        )
        .unwrap_or(false)
    };
    let trigger_exists = |name: &str| -> bool {
        conn.query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='trigger' AND name=?1",
            rusqlite::params![name],
            |row| row.get::<_, bool>(0),
        )
        .unwrap_or(false)
    };
    let column_exists = |table: &str, column: &str| -> bool {
        conn.query_row(
            "SELECT COUNT(*) > 0 FROM pragma_table_info(?1) WHERE name=?2",
            rusqlite::params![table, column],
            |row| row.get::<_, bool>(0),
        )
        .unwrap_or(false)
    };
    match version {
        1 => table_exists("conversation"),
        2 => table_exists("tool_call"),
        3 => table_exists("provider"),
        4 => table_exists("extension"),
        5 => index_exists("idx_message_conversation"),
        6 => index_exists("idx_run_status"),
        7 => true, // SELECT 1 (no-op migration)
        8 => index_exists("idx_conversation_updated"),
        9 => table_exists("prompt_queue"),
        10 => table_exists("subagent_session"),
        11 => column_exists("subagent_route_policy", "last_parent_heartbeat_at"),
        12 => table_exists("session_actor"),
        13 => column_exists("run", "revision"),
        14 => column_exists("run_event", "event_id"),
        15 => table_exists("project_identity"),
        16 => table_exists("tool_grant_v2"),
        17 => table_exists("subagent_budget_ledger"),
        18 => table_exists("side_effect_record"),
        19 => index_exists("idx_conversation_updated_id"),
        20 => table_exists("provider_route_health"),
        21 => table_exists("capability_skill"),
        22 => table_exists("harness_profile"),
        23 => table_exists("harness_source_manifest"),
        24 => column_exists("harness_draft", "source_candidate_json"),
        25 => table_exists("harness_project_identity"),
        26 => table_exists("harness_notice"),
        27 => trigger_exists("trg_harness_audit_notice"),
        28 => table_exists("turn"),
        29 => column_exists("subagent_session", "project_path"),
        30 => column_exists("side_effect_record", "ledger_sequence"),
        31 => column_exists("side_effect_record", "replay_contract"),
        32 => table_exists("projection_watermark"),
        33 => table_exists("interaction_outbox"),
        34 => index_exists("idx_side_effect_run_sequence"),
        35 => table_exists("creative_proposal_fact"),
        36 => column_exists("subagent_session", "tokens_used"),
        37 => column_exists("resume_plan", "decision"), // last ALTER of v37
        _ => true,
    }
}

/// Apply migration 037 (fold of the legacy `ensure_run_metadata_columns`
/// repair) idempotently, one ALTER at a time. A single `execute_batch` would
/// abort mid-way on `conversation.parent_conversation_id` (already added by
/// v10) after the earlier `run.*` ALTERs had succeeded, leaving later columns
/// (e.g. `side_effect_record.resource`) missing while the postcondition
/// (`run.parent_run_id`) already looked satisfied. Per-statement tolerance
/// makes partial/crash re-entry safe (P0-024).
fn apply_migration_037(conn: &Connection) -> Result<(), String> {
    let alters: &[(&str, &str)] = &[
        ("run", "parent_run_id TEXT"),
        ("run", "agent_profile_id TEXT"),
        ("run", "key_id TEXT"),
        ("run", "permission_profile TEXT NOT NULL DEFAULT 'ask'"),
        ("run", "project_path TEXT"),
        ("run", "retry_count INTEGER NOT NULL DEFAULT 0"),
        ("run", "idempotency_key TEXT"),
        ("run", "retry_of_run_id TEXT"),
        ("run", "retry_of_turn_id TEXT"),
        ("run", "continued_from_run_id TEXT"),
        ("run", "branch_id TEXT"),
        ("run", "branch_parent_message_id TEXT"),
        ("run", "checkpoint_id TEXT"),
        ("run", "resume_of_run_id TEXT"),
        ("conversation", "branch_id TEXT"),
        ("conversation", "parent_conversation_id TEXT"),
        ("conversation", "branch_parent_message_id TEXT"),
        ("side_effect_record", "resource TEXT"),
        ("side_effect_record", "started_at TEXT"),
        ("side_effect_record", "completed_at TEXT"),
        (
            "resume_plan",
            "decision TEXT NOT NULL DEFAULT 'RequiresUserConfirmation'",
        ),
    ];
    for (table, ddl) in alters {
        let already = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM pragma_table_info(?1) WHERE name=?2",
                rusqlite::params![table, ddl.split_whitespace().next().unwrap_or("")],
                |row| row.get::<_, bool>(0),
            )
            .unwrap_or(false);
        if already {
            continue;
        }
        let sql = format!("ALTER TABLE {table} ADD COLUMN {ddl}");
        if let Err(e) = conn.execute_batch(&sql) {
            let msg = e.to_string();
            // Tolerate duplicate column / missing table on re-entry (legacy
            // partial state); anything else is a real failure.
            if !msg.contains("duplicate column") && !msg.contains("no such table") {
                return Err(format!("Migration 37 ({table}) failed: {e}"));
            }
        }
    }
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_run_idempotency_key ON run(idempotency_key);",
    )
    .map_err(|e| format!("Migration 37 index failed: {e}"))
}

/// Legacy checksums accepted for migrations whose canonical text was rewritten
/// during the DATA-001 remediation (R-D3: eliminate DROP TABLE / rename-rebuild
/// from active migrations).
///
/// A database that applied the *historical* text records its historical
/// checksum. Rewriting the canonical text would otherwise trip the drift check
/// and fail closed forever on every real database. Instead, the historical
/// checksum is accepted once as an already-applied marker and the ledger row is
/// re-stamped with the new canonical checksum, so drift checks stay meaningful
/// from that point on (one-way, idempotent, observable).
///
/// Versions and their historical checksums were captured before the rewrite
/// (FNV-1a 64-bit, same function as `checksum_hex`):
///
/// | version | reason for rewrite                                   | legacy checksum            |
/// |---------|------------------------------------------------------|----------------------------|
/// | 4       | removed dead `mcp_server_config` CREATE (was DROP'd by 021) | `27678788fa1a0015` |
/// | 10      | widened `subagent_session.status` CHECK to the business vocabulary | `c09a522ebe1b08b6` |
/// | 11      | removed `subagent_session` create-copy-drop-rename rebuild | `60837927902eced6` |
/// | 21      | removed `DROP TABLE IF EXISTS mcp_server_config`     | `4a3bce47882233f6`         |
fn legacy_checksum_for(version: i64) -> Option<&'static str> {
    match version {
        4 => Some("27678788fa1a0015"),
        10 => Some("c09a522ebe1b08b6"),
        11 => Some("60837927902eced6"),
        21 => Some("4a3bce47882233f6"),
        _ => None,
    }
}

/// Run pending migrations one-by-one with an idempotent, reentrant protocol.
///
/// - Progress is per-version ledger membership (`_daemon_migrations`), never a
///   `MAX(version)` cut (P0-004): a migration merged below an already-applied
///   version still runs.
/// - No bootstrap heuristic that stamps v1..=v10 from the presence of four
///   core tables (P0-023): each version is adopted only when its own
///   postcondition is satisfied.
/// - ALTER-heavy migrations tolerate partial DDL: `apply` runs the canonical
///   SQL; duplicate-column / no-such-table are treated as already-applied and
///   verified, not fatal (crash reentry, P0-024).
/// - Active migrations never use `DROP TABLE` rebuilds (R-D3). The last table
///   rebuild (v11) was replaced by an incremental ALTER during the DATA-001
///   remediation; see `legacy_checksum_for` for how already-applied databases
///   are re-stamped.
/// - Migration failure fails closed: the process startup errors instead of
///   entering a healthy-but-half-migrated state.
pub fn run_pending(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS _daemon_migrations (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL DEFAULT '',
            checksum TEXT NOT NULL,
            applied_at TEXT NOT NULL DEFAULT (datetime('now'))
        );",
    )
    .map_err(|e| format!("Failed to ensure _daemon_migrations: {e}"))?;

    let is_recorded = |version: i64| -> Result<bool, String> {
        conn.query_row(
            "SELECT COUNT(*) > 0 FROM _daemon_migrations WHERE id=?1",
            rusqlite::params![version],
            |row| row.get::<_, bool>(0),
        )
        .map_err(|e| format!("Failed to query migration ledger: {e}"))
    };
    let record = |version: i64, canonical: &str| -> Result<(), String> {
        conn.execute(
            "INSERT OR IGNORE INTO _daemon_migrations (id, name, checksum) VALUES (?1, ?2, ?3)",
            rusqlite::params![
                version,
                format!("migration_{version:03}"),
                checksum_hex(canonical)
            ],
        )
        .map_err(|e| format!("Failed to record migration {version}: {e}"))?;
        Ok(())
    };
    let restamp = |version: i64, canonical: &str| -> Result<(), String> {
        conn.execute(
            "UPDATE _daemon_migrations SET checksum = ?2 WHERE id = ?1",
            rusqlite::params![version, checksum_hex(canonical)],
        )
        .map_err(|e| format!("Failed to re-stamp migration {version} checksum: {e}"))?;
        Ok(())
    };

    for (version, canonical) in ALL {
        // Already applied: verify checksum drift (fail closed), accepting the
        // one-time legacy re-stamp from the DATA-001 remediation.
        if is_recorded(*version)? {
            let stored: String = conn
                .query_row(
                    "SELECT checksum FROM _daemon_migrations WHERE id=?1",
                    rusqlite::params![*version],
                    |row| row.get(0),
                )
                .map_err(|e| format!("Failed to read migration checksum: {e}"))?;
            let current = checksum_hex(canonical);
            if stored != current {
                if let Some(legacy) = legacy_checksum_for(*version) {
                    if stored == legacy {
                        restamp(*version, canonical)?;
                        continue;
                    }
                }
                return Err(format!(
                    "Migration {version} checksum drift (stored {stored}, current {current}) — \
                     editing an applied migration fails closed; add a new migration instead"
                ));
            }
            continue;
        }
        // Not recorded: adopt if postcondition already satisfied (partial /
        // crash / legacy runner state), else apply then verify then record.
        if postcondition_satisfied(conn, *version) {
            record(*version, canonical)?;
            continue;
        }
        // Run the canonical SQL. Migrations rely on idempotent SQL + tolerant
        // postcondition re-entry; v37 uses a per-statement idempotent apply
        // (see `apply_migration_037`).
        let apply_result: Result<(), String> = if *version == 37 {
            apply_migration_037(conn)
        } else {
            conn.execute_batch(canonical)
                .map_err(|e| format!("Migration {version} failed: {e}"))
        };
        match apply_result {
            Ok(()) => {
                if !postcondition_satisfied(conn, *version) {
                    return Err(format!(
                        "Migration {version} verify failed: postcondition not satisfied after apply"
                    ));
                }
                record(*version, canonical)?;
            }
            Err(e) => {
                // Tolerate already-applied ALTERs on re-entry (crash mid-way).
                if postcondition_satisfied(conn, *version) {
                    record(*version, canonical)?;
                } else {
                    return Err(e);
                }
            }
        }
    }

    // Fail closed on any FK violations left by migrations.
    let fk_violations: i64 = conn
        .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })
        .map_err(|e| format!("foreign_key_check failed: {e}"))?;
    if fk_violations > 0 {
        return Err(format!(
            "Migration run left {fk_violations} foreign key violations — fail closed"
        ));
    }
    Ok(())
}

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

-- mcp_server_config used to be created here and dropped by migration 021.
-- During the DATA-001 remediation both were removed: the table never shipped a
-- reader or writer, and R-D3 forbids destructive table rebuilds in active
-- migrations. Fresh databases simply never create the dead table; databases
-- that applied the old migrations already had it dropped by 021.

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
///
/// `subagent_session.status` carries the full business vocabulary here so the
/// historical migration 011 table rebuild (which widened the CHECK via
/// create-copy-drop-rename) is not needed — R-D3 forbids that rebuild, and
/// future enum additions are validated at the business layer.
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
CREATE INDEX IF NOT EXISTS idx_subagent_session_parent
    ON subagent_session(parent_conversation_id, created_at);
CREATE INDEX IF NOT EXISTS idx_subagent_session_parent_run
    ON subagent_session(parent_run_id);
CREATE INDEX IF NOT EXISTS idx_subagent_session_activity
    ON subagent_session(status, last_activity_at);
";

/// Migration 011: subagent status vocabulary + parent heartbeat.
///
/// Rewritten during the DATA-001 remediation. The original implementation
/// rebuilt `subagent_session` (create-copy-drop-rename) purely to widen its
/// status CHECK, and R-D3 forbids DROP TABLE rebuilds. SQLite cannot modify a
/// CHECK constraint in place without a rebuild, so the wider status vocabulary
/// is now enforced at the business layer (`subagent_store`) and fresh databases
/// receive the full vocabulary directly from MIGRATION_010's CREATE TABLE. The
/// only schema delta that actually needs SQL here is the parent heartbeat
/// column; the `ADD COLUMN` is idempotent and the runner tolerates re-entry.
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
const MIGRATION_021: &str = "
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

const MIGRATION_023: &str = "
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
const MIGRATION_024: &str = "
ALTER TABLE harness_draft ADD COLUMN source_candidate_json TEXT;
";

/// Migration 025: stable ProjectIdentity registration table.
const MIGRATION_025: &str = "
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
const MIGRATION_026: &str = "
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
const MIGRATION_027: &str = "
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
const MIGRATION_028: &str = "
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

const MIGRATION_029: &str = "
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
const MIGRATION_030: &str = "
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
const MIGRATION_031: &str = "
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
const MIGRATION_032: &str = "
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
const MIGRATION_033: &str = "
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
const MIGRATION_034: &str = "
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
const MIGRATION_035: &str = "
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
const MIGRATION_036: &str = "
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
const MIGRATION_037: &str = "
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

#[cfg(test)]
mod tests {
    use super::{checksum_hex, legacy_checksum_for, run_pending, ALL};
    use rusqlite::Connection;

    /// `run_migrations` skips every entry with `version <= MAX(applied)`, and it
    /// computes that maximum once. A duplicate or out-of-order version therefore
    /// does not fail loudly — it silently never runs. This is the guard for the
    /// exact way two parallel branches lose a migration when they are merged.
    #[test]
    fn migration_versions_are_unique_and_strictly_ascending() {
        let mut previous = 0i64;
        for (version, _) in ALL {
            assert!(
                *version > previous,
                "migration {version} is not greater than the preceding {previous} — \
                 the runner would skip it forever without erroring"
            );
            previous = *version;
        }
    }

    /// Both parallel workstreams must survive the merge. Named explicitly so
    /// dropping either one is a deliberate edit rather than a lost hunk.
    #[test]
    fn the_capability_and_harness_migrations_are_both_present() {
        let versions: Vec<i64> = ALL.iter().map(|(v, _)| *v).collect();
        assert!(
            versions.contains(&21),
            "capability library migration 021 is missing"
        );
        assert!(
            versions.contains(&22),
            "harness control plane migration 022 is missing"
        );
        assert!(
            ALL.iter()
                .any(|(v, sql)| *v == 21 && sql.contains("capability_skill")),
            "021 is no longer the capability library migration"
        );
        assert!(
            ALL.iter()
                .any(|(v, sql)| *v == 22 && sql.contains("harness_profile")),
            "022 is no longer the harness control plane migration"
        );
    }

    /// DATA-001 regression: no active migration may contain a `DROP TABLE`
    /// (or the create-copy-drop-rename rebuild pattern), per R-D3 MUST.
    #[test]
    fn no_active_migration_drops_tables() {
        for (version, sql) in ALL {
            let upper = sql.to_ascii_uppercase();
            assert!(
                !upper.contains("DROP TABLE"),
                "migration {version} must not contain DROP TABLE (R-D3)"
            );
            assert!(
                !upper.contains("RENAME TO"),
                "migration {version} must not rename-rebuild tables (R-D3)"
            );
        }
    }

    /// DATA-001 remediation: a database that applied the *historical* migration
    /// texts records their historical checksums. Rewriting the canonical text
    /// must not fail closed forever on those databases — the legacy checksum is
    /// accepted once and the ledger row re-stamped to the new checksum.
    #[test]
    fn legacy_checksum_restamp_allows_rewritten_migrations() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        // Apply everything with the new canonical texts first, then simulate a
        // pre-remediation database by overwriting the four rewritten versions'
        // ledger rows with their historical checksums.
        run_pending(&conn).unwrap();
        for (version, legacy) in [
            (4, "27678788fa1a0015"),
            (10, "c09a522ebe1b08b6"),
            (11, "60837927902eced6"),
            (21, "4a3bce47882233f6"),
        ] {
            conn.execute(
                "UPDATE _daemon_migrations SET checksum = ?2 WHERE id = ?1",
                rusqlite::params![version, legacy],
            )
            .unwrap();
        }

        // Re-running must not fail closed on drift; it re-stamps the ledger.
        run_pending(&conn).unwrap();
        for (version, sql) in ALL {
            if legacy_checksum_for(*version).is_some() {
                let stored: String = conn
                    .query_row(
                        "SELECT checksum FROM _daemon_migrations WHERE id=?1",
                        rusqlite::params![*version],
                        |r| r.get(0),
                    )
                    .unwrap();
                assert_eq!(
                    stored,
                    checksum_hex(sql),
                    "version {version} must be re-stamped with the new canonical checksum"
                );
            }
        }
        // And a third run is still clean (idempotent, no drift).
        run_pending(&conn).unwrap();
    }

    /// Unknown checksum drift still fails closed: only the documented legacy
    /// checksums are accepted, never an arbitrary edit of an applied migration.
    #[test]
    fn unknown_checksum_drift_still_fails_closed() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        run_pending(&conn).unwrap();
        conn.execute(
            "UPDATE _daemon_migrations SET checksum = 'deadbeefdeadbeef' WHERE id = 1",
            [],
        )
        .unwrap();
        let err = run_pending(&conn).unwrap_err();
        assert!(
            err.contains("checksum drift"),
            "expected fail-closed drift error, got: {err}"
        );
    }

    /// DATA-001: the rewritten v10/v11 incremental path is idempotent — the
    /// widened status CHECK comes from CREATE TABLE (v10) and the only v11
    /// schema delta is a tolerant `ALTER TABLE ... ADD COLUMN`.
    #[test]
    fn rewritten_v11_is_incremental_and_reentrant() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        run_pending(&conn).unwrap();

        // v11 postcondition: subagent_route_policy.last_parent_heartbeat_at.
        let has_heartbeat: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM pragma_table_info('subagent_route_policy')
                 WHERE name = 'last_parent_heartbeat_at'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(has_heartbeat, "v11 must add the parent heartbeat column");

        // Fresh DBs get the full business status vocabulary from v10's CREATE
        // TABLE — inserting a status that the old v10 CHECK rejected works.
        conn.execute(
            "INSERT INTO conversation (id, mode, provider_id, model_id)
             VALUES ('c1', 'agent', 'openai', 'gpt-4o'), ('c2', 'agent', 'openai', 'gpt-4o')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO subagent_session (
                id, parent_conversation_id, child_conversation_id,
                status, provider_id, key_id, model_id, task
             ) VALUES ('s1', 'c1', 'c2', 'completed', 'openai', 'k1', 'gpt-4o', 't')",
            [],
        )
        .unwrap_or_else(|e| panic!("wide status CHECK must accept 'completed': {e}"));

        // Re-entry (simulated crash between v10 and v11) stays clean.
        run_pending(&conn).unwrap();
    }
}
