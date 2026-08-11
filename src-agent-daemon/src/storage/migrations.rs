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
    // 38 moves creative draft metadata under the Daemon-authoritative
    // assistant.db (W3 P0-2); the capability gateway no longer opens natives.db.
    (38, MIGRATION_038),
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
// W9: versioned SQL constants moved to migrations_early / migrations_mid /
// migrations_late; `ALL` above remains the single ordered registry.
#[path = "migrations_early.rs"]
mod migrations_early;
#[path = "migrations_late.rs"]
mod migrations_late;
#[path = "migrations_mid.rs"]
mod migrations_mid;
use migrations_early::{
    MIGRATION_001, MIGRATION_002, MIGRATION_003, MIGRATION_004, MIGRATION_005, MIGRATION_006,
    MIGRATION_007, MIGRATION_008, MIGRATION_009, MIGRATION_010,
};
use migrations_late::{
    MIGRATION_021, MIGRATION_022, MIGRATION_023, MIGRATION_024, MIGRATION_025, MIGRATION_026,
    MIGRATION_027, MIGRATION_028, MIGRATION_029, MIGRATION_030, MIGRATION_031, MIGRATION_032,
    MIGRATION_033, MIGRATION_034, MIGRATION_035, MIGRATION_036, MIGRATION_037, MIGRATION_038,
};
use migrations_mid::{
    MIGRATION_011, MIGRATION_012, MIGRATION_013, MIGRATION_014, MIGRATION_015, MIGRATION_016,
    MIGRATION_017, MIGRATION_018, MIGRATION_019, MIGRATION_020,
};

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
