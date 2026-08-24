//! v30 migration — PWSV2 (Personal Workspace V2) completion + schema marker.
//!
//! v29 already created every PWSV2 table/column/index (spec §1.3): the
//! `workspaces` / `workspace_widgets` / `workspace_layouts` /
//! `workspace_view_states` / `workspace_context_items` additive columns, the
//! `workspace_open_tabs` session table, and the `workspace_templates` manifest
//! table. v30 therefore does **not** invent new tables. Its genuine, additive
//! role is a *failure-recoverable completion step* (the same pattern the
//! codebase already uses for "repair path for vN tables when a database
//! carries an advanced marker"):
//!
//! - If a v29 run was interrupted **between** DDL statements and its
//!   `_schema_version = '29'` marker write (crash/panic mid-step), the marker
//!   may already be `29` while some PWSV2 objects are still missing. v30
//!   re-asserts every v29 guard idempotently (column guards are
//!   `PRAGMA table_info`-based; tables/indexes use `IF NOT EXISTS`) and
//!   re-runs the two deterministic backfills.
//! - On a healthy v29/v30 database every guard is a no-op — v30 only advances
//!   the schema marker.
//!
//! Fully additive and idempotent: no `DROP TABLE`, no rebuild (R-D3), no data
//! loss. A process that already reports `>= 30` skips the whole step. The
//! shared DDL lives in `migration_v29::complete_pws2_schema` so the two steps
//! cannot drift.

use super::migration_v29::complete_pws2_schema;
use crate::Error;
use rusqlite::Connection;

/// Migration v29 → v30: complete (idempotently) the PWSV2 schema that v29
/// started, then advance the marker to 30.
///
/// See `complete_pws2_schema` for the exact object set. This function never
/// downgrades a newer marker and never drops user data.
pub(super) fn migrate_v30(conn: &Connection) -> Result<(), Error> {
    complete_pws2_schema(conn)?;
    conn.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES ('_schema_version', '30')",
        [],
    )
    .map_err(Error::Database)?;
    Ok(())
}

#[cfg(test)]
mod migration_v30_validation {
    //! PWSV2-T02: v30 completion / idempotency / recovery fixtures
    //! (in-memory DB, no side effects).

    use crate::db::{apply_migrations, complete_pws2_schema, create_tables, SCHEMA_VERSION};
    use rusqlite::{params, Connection};

    fn columns(conn: &Connection, table: &str) -> Vec<String> {
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info({table})"))
            .unwrap();
        let rows = stmt.query_map([], |r| r.get::<_, String>(1)).unwrap();
        let list: Vec<String> = rows.filter_map(|r| r.ok()).collect();
        list
    }

    fn index_exists(conn: &Connection, table: &str, name: &str) -> bool {
        let mut stmt = conn
            .prepare(&format!("PRAGMA index_list({table})"))
            .unwrap();
        let rows = stmt.query_map([], |r| r.get::<_, String>(1)).unwrap();
        let found = rows.filter_map(|r| r.ok()).any(|n| n == name);
        found
    }

    fn schema_version(conn: &Connection) -> String {
        conn.query_row(
            "SELECT value FROM settings WHERE key = '_schema_version'",
            [],
            |r| r.get(0),
        )
        .unwrap()
    }

    fn count(conn: &Connection, table: &str) -> i64 {
        conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }

    fn seed_workspace(conn: &Connection, id: &str, active: i64) {
        conn.execute(
            "INSERT INTO workspaces (id, name, kind, theme, is_active, position, created_at, updated_at)
             VALUES (?1, ?1, 'workspace', 'dark', ?2, 0, '2026-08-24T00:00:00Z', '2026-08-24T00:00:00Z')",
            params![id, active],
        )
        .unwrap();
    }

    fn seed_widget(conn: &Connection, ws: &str, id: &str, hidden: i64) {
        conn.execute(
            "INSERT INTO workspace_widgets
                (id, workspace_id, widget_type, config_json, hidden, position, created_at, updated_at)
             VALUES (?1, ?2, 'notes', '{}', ?3, 0, '2026-08-24T00:00:00Z', '2026-08-24T00:00:00Z')",
            params![id, ws, hidden],
        )
        .unwrap();
    }

    /// Assert the full PWSV2 object set is present (columns, tables, indexes).
    fn expect_pws2_objects(conn: &Connection) {
        let ws = columns(conn, "workspaces");
        for col in [
            "default_layout_mode",
            "appearance_json",
            "template_source_id",
            "template_version",
            "deleted_at",
        ] {
            assert!(
                ws.contains(&col.to_string()),
                "workspaces.{col} missing after v30"
            );
        }
        let widgets = columns(conn, "workspace_widgets");
        for col in [
            "config_version",
            "appearance_json",
            "enabled",
            "z_index",
            "hidden",
        ] {
            assert!(
                widgets.contains(&col.to_string()),
                "workspace_widgets.{col} missing after v30 (hidden must survive)"
            );
        }
        let layouts = columns(conn, "workspace_layouts");
        for col in ["layout_mode", "layout_version"] {
            assert!(
                layouts.contains(&col.to_string()),
                "workspace_layouts.{col} missing after v30"
            );
        }
        assert!(columns(conn, "workspace_view_states").contains(&"state_version".to_string()));
        assert!(columns(conn, "workspace_context_items").contains(&"updated_at".to_string()));
        assert_eq!(
            columns(conn, "workspace_open_tabs").len(),
            5,
            "workspace_open_tabs shape drifted"
        );
        assert_eq!(
            columns(conn, "workspace_templates").len(),
            10,
            "workspace_templates shape drifted"
        );
        assert!(
            index_exists(
                conn,
                "workspace_layouts",
                "uq_workspace_layouts_mode_breakpoint"
            ),
            "semantic unique index missing after v30"
        );
        assert!(
            index_exists(conn, "workspace_open_tabs", "idx_workspace_open_tabs_order"),
            "open-tabs order index missing after v30"
        );
        assert!(
            index_exists(
                conn,
                "workspace_templates",
                "idx_workspace_templates_origin"
            ),
            "templates origin index missing after v30"
        );
    }

    /// PWSV2-T02: a FRESH database reaches v30 through the full chain and the
    /// version marker matches `SCHEMA_VERSION`.
    #[test]
    fn fresh_db_reaches_v30_and_marker_matches_const() {
        let conn = Connection::open_in_memory().unwrap();
        create_tables(&conn).unwrap();
        apply_migrations(&conn).unwrap();
        assert_eq!(schema_version(&conn), SCHEMA_VERSION);
        expect_pws2_objects(&conn);
    }

    /// PWSV2-T02: re-running the whole chain repeatedly is idempotent — the
    /// marker stays at current SCHEMA_VERSION, no duplicate objects, and backfills do not create
    /// spurious session tabs for a workspace-less DB.
    #[test]
    fn v30_full_chain_rerun_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        create_tables(&conn).unwrap();
        apply_migrations(&conn).unwrap();
        apply_migrations(&conn).unwrap();
        apply_migrations(&conn).unwrap();
        assert_eq!(schema_version(&conn), SCHEMA_VERSION);
        expect_pws2_objects(&conn);
        assert_eq!(count(&conn, "workspace_open_tabs"), 0);
        assert_eq!(count(&conn, "workspace_templates"), 0);
    }

    /// PWSV2-T02: `complete_pws2_schema` is a safe no-op on a healthy database
    /// (every guard is idempotent, column/table counts are unchanged).
    #[test]
    fn v30_completion_is_noop_on_healthy_db() {
        let conn = Connection::open_in_memory().unwrap();
        create_tables(&conn).unwrap();
        apply_migrations(&conn).unwrap();
        let before_ws = columns(&conn, "workspaces").len();
        let before_tabs = columns(&conn, "workspace_open_tabs").len();
        complete_pws2_schema(&conn).unwrap();
        complete_pws2_schema(&conn).unwrap();
        assert_eq!(columns(&conn, "workspaces").len(), before_ws);
        assert_eq!(columns(&conn, "workspace_open_tabs").len(), before_tabs);
        assert_eq!(schema_version(&conn), SCHEMA_VERSION);
        expect_pws2_objects(&conn);
    }

    /// PWSV2-T02: when the marker is rolled back to 29, the registry re-runs
    /// v30 (29 < 30) and the marker advances back to SCHEMA_VERSION without duplicate DDL.
    #[test]
    fn v30_registry_runs_when_marker_is_29() {
        let conn = Connection::open_in_memory().unwrap();
        create_tables(&conn).unwrap();
        apply_migrations(&conn).unwrap();
        assert_eq!(schema_version(&conn), SCHEMA_VERSION);
        // Simulate an install that reported 29 (e.g. a rolled-back app).
        conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES ('_schema_version', '29')",
            [],
        )
        .unwrap();
        apply_migrations(&conn).unwrap();
        assert_eq!(schema_version(&conn), SCHEMA_VERSION);
        expect_pws2_objects(&conn);
    }

    /// PWSV2-T02: a v29 run interrupted AFTER creating the v27 base tables but
    /// BEFORE the PWSV2 objects — marker claims 29, objects are missing — is
    /// completed by v30. The test-only DROP below simulates the interruption;
    /// the production path never drops (R-D3).
    #[test]
    fn v30_repairs_interrupted_v29_marker_29_with_missing_objects() {
        let conn = Connection::open_in_memory().unwrap();
        create_tables(&conn).unwrap();
        apply_migrations(&conn).unwrap();
        // Tear down the PWSV2 objects to simulate a crashed mid-v29 state.
        drop_pws2_objects(&conn);
        conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES ('_schema_version', '29')",
            [],
        )
        .unwrap();
        assert!(!columns(&conn, "workspaces").contains(&"deleted_at".to_string()));
        assert!(!index_exists(
            &conn,
            "workspace_open_tabs",
            "idx_workspace_open_tabs_order"
        ));

        super::migrate_v30(&conn).unwrap();
        assert_eq!(schema_version(&conn), "30");
        expect_pws2_objects(&conn);
    }

    /// PWSV2-T02: a database at marker 30 that is missing the PWSV2 tables
    /// (simulated advanced-marker failure) is repaired by a direct completion
    /// call, and the marker is preserved (no downgrade).
    #[test]
    fn v30_completion_repairs_advanced_marker_missing_tables() {
        let conn = Connection::open_in_memory().unwrap();
        create_tables(&conn).unwrap();
        apply_migrations(&conn).unwrap();
        drop_pws2_objects(&conn);
        assert_eq!(schema_version(&conn), SCHEMA_VERSION);

        complete_pws2_schema(&conn).unwrap();
        assert_eq!(
            schema_version(&conn),
            SCHEMA_VERSION,
            "marker must not be downgraded"
        );
        expect_pws2_objects(&conn);
    }

    /// PWSV2-T02: the `enabled` backfill re-asserted by the completion step
    /// maps legacy `hidden` rows and never clobbers a production `enabled`
    /// value on a re-run.
    #[test]
    fn v30_enabled_backfill_is_idempotent_and_safe() {
        let conn = Connection::open_in_memory().unwrap();
        create_tables(&conn).unwrap();
        apply_migrations(&conn).unwrap();
        seed_workspace(&conn, "ws-a", 1);
        seed_widget(&conn, "ws-a", "w-hidden", 1);
        seed_widget(&conn, "ws-a", "w-shown", 0);

        // Production cutover flips one row's `enabled` directly (hidden=1).
        conn.execute(
            "UPDATE workspace_widgets SET enabled = 0, hidden = 1 WHERE id = 'w-shown'",
            [],
        )
        .unwrap();
        // Re-running the completion backfill must not flip `enabled` back.
        complete_pws2_schema(&conn).unwrap();
        let enabled: Vec<i64> = {
            let mut stmt = conn
                .prepare("SELECT enabled FROM workspace_widgets ORDER BY id")
                .unwrap();
            stmt.query_map([], |r| r.get::<_, i64>(0))
                .unwrap()
                .map(|r| r.unwrap())
                .collect()
        };
        assert_eq!(enabled, vec![0, 0]);
    }

    /// PWSV2-T02: the open-tab backfill re-asserted by the completion step is
    /// idempotent and skips soft-deleted workspaces.
    #[test]
    fn v30_open_tabs_backfill_idempotent_and_skips_soft_deleted() {
        let conn = Connection::open_in_memory().unwrap();
        create_tables(&conn).unwrap();
        apply_migrations(&conn).unwrap();
        seed_workspace(&conn, "ws-active", 1);
        seed_workspace(&conn, "ws-closed", 0);
        conn.execute(
            "UPDATE workspaces SET deleted_at = '2026-08-24T00:00:00Z' WHERE id = 'ws-closed'",
            [],
        )
        .unwrap();
        complete_pws2_schema(&conn).unwrap();
        complete_pws2_schema(&conn).unwrap();
        assert_eq!(count(&conn, "workspace_open_tabs"), 1);
        let opened: String = conn
            .query_row("SELECT workspace_id FROM workspace_open_tabs", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(opened, "ws-active");
    }

    /// Test-only: tear down the PWSV2 objects to simulate an interrupted v29.
    /// Never used by the production path (R-D3).
    fn drop_pws2_objects(conn: &Connection) {
        conn.execute("DROP TABLE IF EXISTS workspace_open_tabs", [])
            .unwrap();
        conn.execute("DROP TABLE IF EXISTS workspace_templates", [])
            .unwrap();
        let _ = conn.execute(
            "DROP INDEX IF EXISTS uq_workspace_layouts_mode_breakpoint",
            [],
        );
        for (table, col) in [
            ("workspaces", "default_layout_mode"),
            ("workspaces", "appearance_json"),
            ("workspaces", "template_source_id"),
            ("workspaces", "template_version"),
            ("workspaces", "deleted_at"),
            ("workspace_widgets", "config_version"),
            ("workspace_widgets", "appearance_json"),
            ("workspace_widgets", "enabled"),
            ("workspace_widgets", "z_index"),
            ("workspace_layouts", "layout_mode"),
            ("workspace_layouts", "layout_version"),
            ("workspace_view_states", "state_version"),
            ("workspace_context_items", "updated_at"),
        ] {
            // SQLite 3.35+ supports DROP COLUMN; the bundled SQLite does.
            conn.execute(&format!("ALTER TABLE {table} DROP COLUMN {col}"), [])
                .unwrap();
        }
    }
}
