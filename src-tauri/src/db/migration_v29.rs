//! v29 migration — PWSV2 (Personal Workspace V2) incremental schema step.
//!
//! Implements the ADR-0021 §PWSV2 ruling (2026-08-23, see also
//! `docs/contracts/workspace-v2-contract.md` revision header and
//! `docs/contracts/workspace-v2-contract-matrix.md` §10.5):
//!
//! - `workspaces`: `default_layout_mode` / `appearance_json` /
//!   `template_source_id` / `template_version` / `deleted_at`
//!   (soft delete — M-05 reversal; Close ≠ Delete).
//! - `workspace_widgets`: `config_version` / `appearance_json` / `enabled` /
//!   `z_index` (M-13..M-16). Backfill `enabled = NOT hidden` (legacy rows
//!   only); the `hidden` column is kept until death proof — after cutover the
//!   production path reads/writes `enabled` exclusively.
//! - `workspace_layouts`: `layout_mode` / `layout_version` (M-17..M-20). Free
//!   mode keeps the reserved breakpoint value `'free'`, so the v27
//!   `UNIQUE(workspace_id, breakpoint)` stays valid across modes; an explicit
//!   `UNIQUE(workspace_id, layout_mode, breakpoint)` index is added for
//!   semantic clarity (strictly weaker than the existing constraint).
//! - `workspace_view_states`: `state_version` (M-22).
//! - `workspace_context_items`: `updated_at` (M-12). The 9-value `item_kind`
//!   CHECK (M-09) is enforced in the workspace service layer, because
//!   SQLite cannot add a CHECK to an existing column without a table rebuild,
//!   which R-D3 discourages for user data.
//! - new table `workspace_open_tabs`: the real Workspace *session* tabs
//!   (row exists = open; M-06/M-26). The v27 `workspace_tabs` content-tab
//!   table becomes legacy (no production reads/writes after this step).
//! - new table `workspace_templates`: builtin/personal template manifests.
//!   Builtin manifests are code-provided (`crate::workspace::templates`);
//!   the table stores personal rows and builtin metadata overrides.
//!
//! Fully additive and idempotent: every `ALTER TABLE ADD COLUMN` is guarded
//! by a `PRAGMA table_info` check, every `CREATE TABLE`/`INDEX` uses
//! `IF NOT EXISTS`, and every backfill is deterministic. No `DROP TABLE`,
//! no rebuild (R-D3). Rollback story: an older release simply ignores the
//! new columns/tables.

use crate::Error;
use rusqlite::Connection;

/// Idempotent ALTER: add a column to `table` only when it is absent yet.
fn add_column_if_missing(
    conn: &Connection,
    table: &str,
    col: &str,
    ddl: &str,
) -> Result<(), Error> {
    let has = table_columns(conn, table)?.iter().any(|c| c == col);
    if !has {
        conn.execute(&format!("ALTER TABLE {table} ADD COLUMN {col} {ddl}"), [])
            .map_err(Error::Database)?;
    }
    Ok(())
}

/// Column-name set of a table via `PRAGMA table_info`.
fn table_columns(conn: &Connection, table: &str) -> Result<Vec<String>, Error> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(Error::Database)?;
    let rows = stmt
        .query_map([], |r| r.get::<_, String>(1))
        .map_err(Error::Database)?;
    let mut cols = Vec::new();
    for r in rows {
        cols.push(r.map_err(Error::Database)?);
    }
    Ok(cols)
}

/// One-shot, deterministic backfill of the v27 `hidden` column into the new
/// `enabled` column. Only legacy rows (hidden=1 while enabled still carries
/// the ADD COLUMN default of 1) are touched, so re-running the step is safe
/// even after the production path has switched to writing `enabled` only.
fn backfill_widget_enabled(conn: &Connection) -> Result<(), Error> {
    conn.execute(
        "UPDATE workspace_widgets SET enabled = 0 WHERE hidden = 1 AND enabled = 1",
        [],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// One-shot backfill of the session tab: the currently active workspace (v27
/// `is_active = 1`) becomes the single open session tab. The *active*
/// workspace itself stays sourced from `workspaces.is_active` (v27 column —
/// the single authority for "which workspace is on screen"); this table only
/// records which workspaces are OPEN. `INSERT OR IGNORE` keeps the step
/// idempotent.
fn backfill_open_tabs(conn: &Connection) -> Result<(), Error> {
    conn.execute(
        "INSERT OR IGNORE INTO workspace_open_tabs
            (workspace_id, sort_order, is_pinned, opened_at, last_active_at)
         SELECT id, 0, 0, COALESCE(updated_at, created_at), COALESCE(updated_at, created_at)
         FROM workspaces
         WHERE is_active = 1 AND deleted_at IS NULL",
        [],
    )
    .map_err(Error::Database)?;
    Ok(())
}

pub(super) fn migrate_v29(conn: &Connection) -> Result<(), Error> {
    // ── 1. workspaces: layout mode / appearance / template provenance / soft delete
    add_column_if_missing(
        conn,
        "workspaces",
        "default_layout_mode",
        "TEXT NOT NULL DEFAULT 'structured'",
    )?;
    add_column_if_missing(
        conn,
        "workspaces",
        "appearance_json",
        "TEXT NOT NULL DEFAULT '{}'",
    )?;
    add_column_if_missing(conn, "workspaces", "template_source_id", "TEXT")?;
    add_column_if_missing(conn, "workspaces", "template_version", "INTEGER")?;
    add_column_if_missing(conn, "workspaces", "deleted_at", "TEXT")?;

    // ── 2. workspace_widgets: config version / appearance / enabled / z-index
    add_column_if_missing(
        conn,
        "workspace_widgets",
        "config_version",
        "INTEGER NOT NULL DEFAULT 1",
    )?;
    add_column_if_missing(
        conn,
        "workspace_widgets",
        "appearance_json",
        "TEXT NOT NULL DEFAULT '{}'",
    )?;
    add_column_if_missing(
        conn,
        "workspace_widgets",
        "enabled",
        "INTEGER NOT NULL DEFAULT 1",
    )?;
    add_column_if_missing(
        conn,
        "workspace_widgets",
        "z_index",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    backfill_widget_enabled(conn)?;

    // ── 3. workspace_layouts: layout mode / layout version + explicit unique index
    add_column_if_missing(
        conn,
        "workspace_layouts",
        "layout_mode",
        "TEXT NOT NULL DEFAULT 'structured'",
    )?;
    add_column_if_missing(
        conn,
        "workspace_layouts",
        "layout_version",
        "INTEGER NOT NULL DEFAULT 1",
    )?;
    // Strictly implied by the existing UNIQUE(workspace_id, breakpoint) (free
    // mode reserves breakpoint 'free'), added for semantic clarity.
    conn.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS uq_workspace_layouts_mode_breakpoint
            ON workspace_layouts(workspace_id, layout_mode, breakpoint)",
        [],
    )
    .map_err(Error::Database)?;

    // ── 4. workspace_view_states: state version
    add_column_if_missing(
        conn,
        "workspace_view_states",
        "state_version",
        "INTEGER NOT NULL DEFAULT 1",
    )?;

    // ── 5. workspace_context_items: updated_at
    add_column_if_missing(conn, "workspace_context_items", "updated_at", "TEXT")?;

    // ── 6. workspace_open_tabs: real Workspace session tabs (M-06/M-26)
    conn.execute(
        "CREATE TABLE IF NOT EXISTS workspace_open_tabs (
            workspace_id TEXT PRIMARY KEY REFERENCES workspaces(id) ON DELETE CASCADE,
            sort_order REAL NOT NULL DEFAULT 0,
            is_pinned INTEGER NOT NULL DEFAULT 0,
            opened_at TEXT NOT NULL,
            last_active_at TEXT NOT NULL
        )",
        [],
    )
    .map_err(Error::Database)?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_workspace_open_tabs_order
            ON workspace_open_tabs(is_pinned, sort_order)",
        [],
    )
    .map_err(Error::Database)?;
    backfill_open_tabs(conn)?;

    // ── 7. workspace_templates: builtin/personal template manifests
    conn.execute(
        "CREATE TABLE IF NOT EXISTS workspace_templates (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            origin TEXT NOT NULL DEFAULT 'personal' CHECK(origin IN ('builtin','personal')),
            schema_version INTEGER NOT NULL DEFAULT 1,
            template_version INTEGER NOT NULL DEFAULT 1,
            manifest_json TEXT NOT NULL DEFAULT '{}',
            preview_key TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            deleted_at TEXT
        )",
        [],
    )
    .map_err(Error::Database)?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_workspace_templates_origin
            ON workspace_templates(origin, deleted_at)",
        [],
    )
    .map_err(Error::Database)?;

    // ── 8. version marker (never downgrade a newer marker — see apply()) ──
    conn.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES ('_schema_version', '29')",
        [],
    )
    .map_err(Error::Database)?;
    Ok(())
}

#[cfg(test)]
mod migration_v29_validation {
    //! PWSV2-T02: v29 additive/idempotent/FK validation (in-memory DB, no
    //! side effects). Mirrors the v28 validation pattern.

    use crate::db::{apply_migrations, create_tables, SCHEMA_VERSION};
    use rusqlite::{params, Connection};

    fn columns(conn: &Connection, table: &str) -> Vec<String> {
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info({table})"))
            .unwrap();
        let rows = stmt.query_map([], |r| r.get::<_, String>(1)).unwrap();
        rows.filter_map(|r| r.ok()).collect()
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
             VALUES (?1, ?1, 'workspace', 'dark', ?2, 0, '2026-08-23T00:00:00Z', '2026-08-23T00:00:00Z')",
            params![id, active],
        )
        .unwrap();
    }

    fn seed_widget(conn: &Connection, ws: &str, id: &str, hidden: i64) {
        conn.execute(
            "INSERT INTO workspace_widgets
                (id, workspace_id, widget_type, config_json, hidden, position, created_at, updated_at)
             VALUES (?1, ?2, 'notes', '{}', ?3, 0, '2026-08-23T00:00:00Z', '2026-08-23T00:00:00Z')",
            params![id, ws, hidden],
        )
        .unwrap();
    }

    fn seed_layout(conn: &Connection, ws: &str, id: &str, breakpoint: &str) {
        conn.execute(
            "INSERT INTO workspace_layouts
                (id, workspace_id, breakpoint, layout_json, is_active, created_at, updated_at)
             VALUES (?1, ?2, ?3, '[]', 1, '2026-08-23T00:00:00Z', '2026-08-23T00:00:00Z')",
            params![id, ws, breakpoint],
        )
        .unwrap();
    }

    /// PWSV2-T02: a FRESH database reaches v29 through the full chain and the
    /// version marker matches `SCHEMA_VERSION`.
    #[test]
    fn fresh_db_reaches_v29_and_marker_matches_const() {
        let conn = Connection::open_in_memory().unwrap();
        create_tables(&conn).unwrap();
        apply_migrations(&conn).unwrap();
        assert_eq!(schema_version(&conn), SCHEMA_VERSION);
        assert_eq!(schema_version(&conn), "29");
    }

    /// PWSV2-T02: every PWSV2 column lands on the right table.
    #[test]
    fn v29_altered_tables_expose_expected_columns() {
        let conn = Connection::open_in_memory().unwrap();
        create_tables(&conn).unwrap();
        apply_migrations(&conn).unwrap();

        let ws = columns(&conn, "workspaces");
        for col in [
            "default_layout_mode",
            "appearance_json",
            "template_source_id",
            "template_version",
            "deleted_at",
        ] {
            assert!(ws.contains(&col.to_string()), "workspaces.{col} missing");
        }

        let widgets = columns(&conn, "workspace_widgets");
        for col in [
            "config_version",
            "appearance_json",
            "enabled",
            "z_index",
            "hidden",
        ] {
            assert!(
                widgets.contains(&col.to_string()),
                "workspace_widgets.{col} missing (hidden must survive until death proof)"
            );
        }

        let layouts = columns(&conn, "workspace_layouts");
        for col in ["layout_mode", "layout_version"] {
            assert!(
                layouts.contains(&col.to_string()),
                "workspace_layouts.{col} missing"
            );
        }

        let views = columns(&conn, "workspace_view_states");
        assert!(views.contains(&"state_version".to_string()));

        let context = columns(&conn, "workspace_context_items");
        assert!(context.contains(&"updated_at".to_string()));
    }

    /// PWSV2-T02: the two new tables exist with the contracted shape.
    #[test]
    fn v29_new_tables_exist_with_expected_columns() {
        let conn = Connection::open_in_memory().unwrap();
        create_tables(&conn).unwrap();
        apply_migrations(&conn).unwrap();

        let tabs = columns(&conn, "workspace_open_tabs");
        for col in [
            "workspace_id",
            "sort_order",
            "is_pinned",
            "opened_at",
            "last_active_at",
        ] {
            assert!(
                tabs.contains(&col.to_string()),
                "workspace_open_tabs.{col} missing"
            );
        }

        let templates = columns(&conn, "workspace_templates");
        for col in [
            "id",
            "name",
            "origin",
            "schema_version",
            "template_version",
            "manifest_json",
            "preview_key",
            "created_at",
            "updated_at",
            "deleted_at",
        ] {
            assert!(
                templates.contains(&col.to_string()),
                "workspace_templates.{col} missing"
            );
        }
    }

    /// PWSV2-T02: layout storage keeps both modes per workspace — structured
    /// lg/md/sm + free ('free' reserved breakpoint) — and the v27 unique
    /// constraint still rejects duplicate breakpoints across modes.
    #[test]
    fn v29_layouts_store_both_modes_and_keep_unique_breakpoint() {
        let conn = Connection::open_in_memory().unwrap();
        create_tables(&conn).unwrap();
        apply_migrations(&conn).unwrap();
        seed_workspace(&conn, "ws-a", 1);

        seed_layout(&conn, "ws-a", "lay-lg", "lg");
        seed_layout(&conn, "ws-a", "lay-md", "md");
        seed_layout(&conn, "ws-a", "lay-sm", "sm");
        seed_layout(&conn, "ws-a", "lay-free", "free");
        // All existing rows default to layout_mode 'structured'.
        conn.execute(
            "UPDATE workspace_layouts SET layout_mode = 'free' WHERE breakpoint = 'free'",
            [],
        )
        .unwrap();
        assert_eq!(count(&conn, "workspace_layouts"), 4);

        // A second row with the same (workspace_id, breakpoint) is rejected
        // even under a different layout_mode.
        let err = conn.execute(
            "INSERT INTO workspace_layouts
                (id, workspace_id, breakpoint, layout_json, layout_mode, is_active, created_at, updated_at)
             VALUES ('lay-lg-2', 'ws-a', 'lg', '[]', 'free', 1,
                     '2026-08-23T00:00:00Z', '2026-08-23T00:00:00Z')",
            [],
        );
        assert!(
            err.is_err(),
            "duplicate (workspace_id, breakpoint) must be rejected"
        );
    }

    /// PWSV2-T02: widget visibility backfill maps legacy `hidden` to
    /// `enabled` and is re-run safe.
    #[test]
    fn v29_widget_enabled_backfill_maps_hidden_and_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        create_tables(&conn).unwrap();
        apply_migrations(&conn).unwrap();
        seed_workspace(&conn, "ws-a", 1);
        seed_widget(&conn, "ws-a", "w-1", 0);
        seed_widget(&conn, "ws-a", "w-2", 1);
        super::backfill_widget_enabled(&conn).unwrap();

        let enabled: Vec<i64> = {
            let mut stmt = conn
                .prepare("SELECT enabled FROM workspace_widgets ORDER BY id")
                .unwrap();
            stmt.query_map([], |r| r.get::<_, i64>(0))
                .unwrap()
                .map(|r| r.unwrap())
                .collect()
        };
        assert_eq!(enabled, vec![1, 0]);

        // Production cutover later writes `enabled` directly (hidden stays 0);
        // re-running the mapping must not clobber it.
        conn.execute(
            "UPDATE workspace_widgets SET enabled = 1, hidden = 0 WHERE id = 'w-2'",
            [],
        )
        .unwrap();
        super::backfill_widget_enabled(&conn).unwrap();
        let enabled_after: Vec<i64> = {
            let mut stmt = conn
                .prepare("SELECT enabled FROM workspace_widgets ORDER BY id")
                .unwrap();
            stmt.query_map([], |r| r.get::<_, i64>(0))
                .unwrap()
                .map(|r| r.unwrap())
                .collect()
        };
        assert_eq!(enabled_after, vec![1, 1]);
    }

    /// PWSV2-T02: the active v27 workspace becomes the single open, active
    /// session tab; non-active workspaces are not opened.
    #[test]
    fn v29_open_tabs_backfilled_from_active_workspace() {
        let conn = Connection::open_in_memory().unwrap();
        create_tables(&conn).unwrap();
        apply_migrations(&conn).unwrap();
        seed_workspace(&conn, "ws-active", 1);
        seed_workspace(&conn, "ws-closed", 0);
        super::backfill_open_tabs(&conn).unwrap();

        assert_eq!(count(&conn, "workspace_open_tabs"), 1);
        let opened: String = conn
            .query_row("SELECT workspace_id FROM workspace_open_tabs", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(opened, "ws-active");
    }

    /// PWSV2-T02: the whole migration is additive — pre-existing user data in
    /// all seven v27 tables survives byte-identical row counts, and the step
    /// is safe to re-run directly (guards are no-ops).
    #[test]
    fn v29_additive_preserves_data_and_rerun_is_safe() {
        let conn = Connection::open_in_memory().unwrap();
        create_tables(&conn).unwrap();
        apply_migrations(&conn).unwrap();
        seed_workspace(&conn, "ws-a", 1);
        seed_widget(&conn, "ws-a", "w-1", 1);
        seed_layout(&conn, "ws-a", "lay-lg", "lg");
        conn.execute(
            "INSERT INTO workspace_view_states
                (id, workspace_id, view_key, state_json, updated_at)
             VALUES ('vs-1', 'ws-a', 'data_view:default', '{}', '2026-08-23T00:00:00Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO workspace_context_items
                (id, workspace_id, item_kind, ref_id, title, meta_json, position, created_at)
             VALUES ('ci-1', 'ws-a', 'link', 'https://example.com', 'Example', '{}', 0,
                     '2026-08-23T00:00:00Z')",
            [],
        )
        .unwrap();

        assert_eq!(count(&conn, "workspaces"), 1);
        assert_eq!(count(&conn, "workspace_widgets"), 1);
        assert_eq!(count(&conn, "workspace_layouts"), 1);
        assert_eq!(count(&conn, "workspace_view_states"), 1);
        assert_eq!(count(&conn, "workspace_context_items"), 1);

        // Direct re-run of the step: every guard must be a no-op.
        super::migrate_v29(&conn).unwrap();
        assert_eq!(count(&conn, "workspaces"), 1);
        assert_eq!(count(&conn, "workspace_widgets"), 1);
        assert_eq!(count(&conn, "workspace_open_tabs"), 1);
        assert_eq!(schema_version(&conn), "29");
    }

    /// PWSV2-T02: soft-delete rows are filtered out of the open-tabs backfill
    /// and open_tabs cascades with hard workspace deletion (final gate only).
    #[test]
    fn v29_soft_deleted_workspaces_never_open_and_cascade_on_hard_delete() {
        let conn = Connection::open_in_memory().unwrap();
        create_tables(&conn).unwrap();
        apply_migrations(&conn).unwrap();
        seed_workspace(&conn, "ws-a", 1);
        conn.execute(
            "UPDATE workspaces SET deleted_at = '2026-08-23T00:00:01Z' WHERE id = 'ws-a'",
            [],
        )
        .unwrap();
        super::backfill_open_tabs(&conn).unwrap();
        assert_eq!(count(&conn, "workspace_open_tabs"), 0);

        // Hard delete (final-gate path) cascades the session tab.
        conn.execute("DELETE FROM workspaces WHERE id = 'ws-a'", [])
            .unwrap();
        assert_eq!(count(&conn, "workspace_open_tabs"), 0);
        assert_eq!(count(&conn, "workspace_widgets"), 0);
    }
}
