//! v28 migration — personal-creative → application-center (Phase B, spec 05).
//!
//! v28 is the schema step that promotes `applications` from the legacy
//! `source`/`source_id` string identity to the strong-typed
//! `kind` / `registration_origin` contract, and introduces the two
//! kind-specific spec tables. It is fully idempotent: every `ALTER TABLE
//! ADD COLUMN` is guarded by a `PRAGMA table_info` check, every table is
//! created with `IF NOT EXISTS`, and every backfill is safe to re-run.
//!
//! Order (spec 05 §迁移顺序):
//! 1. ALTER / CREATE  — applications + runtime_instances + window_instances
//!                      columns, and the two spec tables (+ indexes).
//! 2. backfill applications kind/origin.
//! 3. backfill remote non_owned → applications + web_application_specs.
//! 4. repair profile bindings.
//! 5. verify unique/FK (enforced by the schema, asserted in tests).
//! 6. `_schema_version = 28`.
//!
//! `source` / `source_id` are intentionally NOT dropped in this migration
//! (compat window, spec 05).

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

pub(super) fn migrate_v28(conn: &Connection) -> Result<(), Error> {
    // ── 1. ALTER / CREATE ─────────────────────────────────────────────────
    // applications: strong-typed kind + registration origin + sidebar + metadata.
    add_column_if_missing(conn, "applications", "kind", "TEXT")?;
    add_column_if_missing(conn, "applications", "registration_origin", "TEXT")?;
    add_column_if_missing(
        conn,
        "applications",
        "show_in_sidebar",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    add_column_if_missing(conn, "applications", "sidebar_order", "INTEGER")?;
    add_column_if_missing(conn, "applications", "metadata_json", "TEXT")?;

    // system_application_specs — one row per system app (spec 05 DDL).
    conn.execute(
        "CREATE TABLE IF NOT EXISTS system_application_specs (
            application_id TEXT PRIMARY KEY REFERENCES applications(id) ON DELETE CASCADE,
            application_path TEXT NOT NULL,
            bundle_identifier TEXT,
            platform TEXT NOT NULL,
            launch_policy TEXT NOT NULL DEFAULT 'activate_existing',
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )",
        [],
    )
    .map_err(Error::Database)?;

    // web_application_specs — one row per web app; profile lives in
    // browser_profile_bindings, never duplicated here (spec 05).
    conn.execute(
        "CREATE TABLE IF NOT EXISTS web_application_specs (
            application_id TEXT PRIMARY KEY REFERENCES applications(id) ON DELETE CASCADE,
            url TEXT NOT NULL,
            approved_origins_json TEXT NOT NULL DEFAULT '[]',
            open_behavior TEXT NOT NULL DEFAULT 'native_webview',
            keep_alive INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )",
        [],
    )
    .map_err(Error::Database)?;

    // runtime_instances: System ownership bookkeeping (spec 05).
    add_column_if_missing(conn, "runtime_instances", "ownership_mode", "TEXT")?;
    add_column_if_missing(conn, "runtime_instances", "external_identity", "TEXT")?;

    // window_instances: Web LRU / hibernation governance (spec 05).
    add_column_if_missing(conn, "window_instances", "last_active_at", "TEXT")?;
    add_column_if_missing(conn, "window_instances", "hibernated_at", "TEXT")?;

    // kind / sidebar query indexes (APP-006 step 6).
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_applications_kind
            ON applications(kind);
         CREATE INDEX IF NOT EXISTS idx_applications_sidebar
            ON applications(show_in_sidebar, sidebar_order)",
        [],
    )
    .map_err(Error::Database)?;

    // ── 2-4. backfill（kind/origin → remote non_owned → profile bindings）──
    // 幂等，整体在写版本标记之前完成（spec 05 §迁移顺序）。
    super::backfill::backfill_v28(conn)?;

    // ── 5-6. 验证 unique/FK 由上面 schema 强制；写版本标记 ────────────────
    conn.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES ('_schema_version', '28')",
        [],
    )
    .map_err(Error::Database)?;
    Ok(())
}

#[cfg(test)]
mod migration_v28_validation {
    //! A-005: v28 列/表/索引/回填/FK 校验（内存临时库，无副作用）。

    use crate::db::{apply_migrations, backfill_v28, create_tables, SCHEMA_VERSION};
    use rusqlite::{params, Connection};

    fn columns(conn: &Connection, table: &str) -> Vec<String> {
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info({table})"))
            .unwrap();
        stmt.query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
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

    /// A-005: a FRESH database reaches v28 through the full chain and the
    /// version marker matches `SCHEMA_VERSION`.
    #[test]
    fn fresh_db_reaches_v28_and_marker_matches_const() {
        let conn = Connection::open_in_memory().unwrap();
        create_tables(&conn).unwrap();
        apply_migrations(&conn).unwrap();
        assert_eq!(schema_version(&conn), SCHEMA_VERSION);
    }

    /// APP-006/007/008/009/010: v28 alters add the exact expected columns.
    #[test]
    fn v28_altered_tables_expose_expected_columns() {
        let conn = Connection::open_in_memory().unwrap();
        create_tables(&conn).unwrap();
        apply_migrations(&conn).unwrap();

        let apps = columns(&conn, "applications");
        for col in [
            "kind",
            "registration_origin",
            "show_in_sidebar",
            "sidebar_order",
            "metadata_json",
        ] {
            assert!(
                apps.contains(&col.to_string()),
                "applications missing {col}"
            );
        }
        // Compat: legacy identity columns are preserved (spec 05 / APP-006 禁止).
        assert!(apps.contains(&"source".to_string()));
        assert!(apps.contains(&"source_id".to_string()));

        let ri = columns(&conn, "runtime_instances");
        assert!(ri.contains(&"ownership_mode".to_string()));
        assert!(ri.contains(&"external_identity".to_string()));

        let win = columns(&conn, "window_instances");
        assert!(win.contains(&"last_active_at".to_string()));
        assert!(win.contains(&"hibernated_at".to_string()));

        // New spec tables exist with their contract columns.
        let sys = columns(&conn, "system_application_specs");
        for col in [
            "application_id",
            "application_path",
            "bundle_identifier",
            "platform",
            "launch_policy",
            "created_at",
            "updated_at",
        ] {
            assert!(
                sys.contains(&col.to_string()),
                "system_application_specs missing {col}"
            );
        }
        let web = columns(&conn, "web_application_specs");
        for col in [
            "application_id",
            "url",
            "approved_origins_json",
            "open_behavior",
            "keep_alive",
            "created_at",
            "updated_at",
        ] {
            assert!(
                web.contains(&col.to_string()),
                "web_application_specs missing {col}"
            );
        }
        // Profile is NOT copied into web spec (single source of truth).
        assert!(!web.contains(&"profile_id".to_string()));
    }

    /// APP-005: applying the whole migration chain twice is a no-op that does
    /// not error (idempotent DDL).
    #[test]
    fn migration_chain_is_idempotent_on_empty_db() {
        let conn = Connection::open_in_memory().unwrap();
        create_tables(&conn).unwrap();
        apply_migrations(&conn).unwrap();
        apply_migrations(&conn).unwrap();
        apply_migrations(&conn).unwrap();
        assert_eq!(schema_version(&conn), SCHEMA_VERSION);
    }

    /// APP-011: the v28 backfill mapping (kind/origin + remote non-owned
    /// migration) is correct and idempotent.
    ///
    /// Faithfully simulates a v27→v28 upgrade: run the full chain to v28 on an
    /// empty DB, then inject the v27-era rows exactly as the v12 backfill would
    /// have produced them (`applications` with `kind=NULL`, plus `non_owned_apps`),
    /// and run `backfill_v28` — the same call `migrate_v28` makes after DDL.
    #[test]
    fn v28_backfill_mapping_is_correct_and_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        create_tables(&conn).unwrap();
        apply_migrations(&conn).unwrap();
        assert_eq!(schema_version(&conn), SCHEMA_VERSION);

        let now = "2026-01-01T00:00:00Z";

        // Simulate the v27-era identity rows the v12 backfill builds, with
        // `kind`/`registration_origin` still NULL (the pre-v28 state).
        conn.execute(
            "INSERT INTO applications (id, source, source_id, title, version, created_at, updated_at)
             VALUES ('app-internal-m1', 'internal', 'm1', 'M', '1', ?1, ?1),
                    ('app-local-loc1', 'local_project', 'loc1', 'Local', '1', ?1, ?1),
                    ('app-external-ext1', 'external_github', 'ext1', 'Ext', '1', ?1, ?1)",
            params![now],
        )
        .unwrap();
        // Remote + attached non-owned records (spec 05 migration inputs).
        conn.execute(
            "INSERT INTO non_owned_apps (id, ownership, url, approved_origins_json, title, created_at, updated_at)
             VALUES ('remote-1', 'remote', 'https://app.example.com/', '[\"app.example.com\"]', 'Remote', ?1, ?1),
                    ('attached-1', 'attached', 'http://127.0.0.1:8080/', '[]', 'Attached', ?1, ?1)",
            params![now],
        )
        .unwrap();

        backfill_v28(&conn).unwrap();

        // Kind/origin mapping for the three legacy sources.
        let mapping: Vec<(String, String, String)> = {
            let mut stmt = conn
                .prepare(
                    "SELECT source, kind, registration_origin FROM applications ORDER BY source",
                )
                .unwrap();
            stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
        };
        // (source, kind, registration_origin) mapping for the three legacy sources.
        let mapping_as_str = |src: &str| -> (&str, &str, &str) {
            let (s, k, o) = mapping
                .iter()
                .find(|(s, _, _)| s == src)
                .expect("source row present");
            (s.as_str(), k.as_str(), o.as_str())
        };
        assert_eq!(
            mapping_as_str("local_project"),
            ("local_project", "local_project", "local_scan")
        );
        assert_eq!(
            mapping_as_str("internal"),
            ("internal", "local_project", "legacy_internal")
        );
        assert_eq!(
            mapping_as_str("external_github"),
            ("external_github", "web_application", "legacy_github")
        );

        // Remote non-owned migrated into applications + web_application_specs.
        let remote_app: Option<(String, String)> = conn
            .query_row(
                "SELECT kind, registration_origin FROM applications WHERE id = 'remote-1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok();
        assert_eq!(
            remote_app.as_ref(),
            Some(&("web_application".to_string(), "migration".to_string()))
        );
        let spec: (String, String, i64) = conn
            .query_row(
                "SELECT url, open_behavior, keep_alive FROM web_application_specs WHERE application_id = 'remote-1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            (spec.0.as_str(), spec.1.as_str(), spec.2),
            ("https://app.example.com/", "native_webview", 0)
        );

        // Attached non-owned is NOT promoted to an application (APP-011 step 4).
        let attached: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM applications WHERE id = 'attached-1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(attached, 0);

        // Exactly one web spec (only the remote record).
        assert_eq!(count(&conn, "web_application_specs"), 1);

        // Idempotency: re-running the v28 backfill must not create duplicates or
        // drift (a second v27→v28 pass over the same rows is a stable no-op).
        let apps_before = count(&conn, "applications");
        backfill_v28(&conn).unwrap();
        backfill_v28(&conn).unwrap();
        assert_eq!(count(&conn, "applications"), apps_before);
        assert_eq!(count(&conn, "web_application_specs"), 1);
        // Mapped values remain stable after re-runs.
        assert_eq!(
            mapping_as_str("internal"),
            ("internal", "local_project", "legacy_internal")
        );
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM applications WHERE kind IS NULL",
                [],
                |r| r.get::<_, i64>(0),
            ),
            Ok(0),
            "every legacy source must be mapped"
        );
    }

    /// APP-007/008: spec rows cascade-delete with their application (FK).
    #[test]
    fn v28_spec_rows_cascade_delete_with_application() {
        let conn = Connection::open_in_memory().unwrap();
        create_tables(&conn).unwrap();
        apply_migrations(&conn).unwrap();
        conn.execute("PRAGMA foreign_keys = ON", []).unwrap();
        let now = "2026-01-01T00:00:00Z";

        conn.execute(
            "INSERT INTO applications (id, source, source_id, title, kind, registration_origin, version, created_at, updated_at)
             VALUES ('app-sys', 'system', 'sys1', 'Sys', 'system_application', 'system_discovery', '1', ?1, ?1)",
            [now],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO system_application_specs (application_id, application_path, bundle_identifier, platform, launch_policy, created_at, updated_at)
             VALUES ('app-sys', '/Applications/X.app', 'com.x.app', 'macos', 'activate_existing', ?1, ?1)",
            [now],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO applications (id, source, source_id, title, kind, registration_origin, version, created_at, updated_at)
             VALUES ('app-web', 'web', 'web1', 'Web', 'web_application', 'manual', '1', ?1, ?1)",
            [now],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO web_application_specs (application_id, url, approved_origins_json, open_behavior, keep_alive, created_at, updated_at)
             VALUES ('app-web', 'https://a.example.com', '[\"a.example.com\"]', 'native_webview', 1, ?1, ?1)",
            [now],
        )
        .unwrap();
        assert_eq!(count(&conn, "system_application_specs"), 1);
        assert_eq!(count(&conn, "web_application_specs"), 1);

        conn.execute("DELETE FROM applications WHERE id = 'app-sys'", [])
            .unwrap();
        conn.execute("DELETE FROM applications WHERE id = 'app-web'", [])
            .unwrap();
        assert_eq!(
            count(&conn, "system_application_specs"),
            0,
            "system spec must cascade"
        );
        assert_eq!(
            count(&conn, "web_application_specs"),
            0,
            "web spec must cascade"
        );
    }

    /// APP-011 step 5: after migrating a remote non-owned app, the
    /// `browser_profile_bindings` reference space stays consistent with the new
    /// application identity.
    ///
    /// A remote app's id doubles as its application id (the stable migration id),
    /// and a remote app that carries NO explicit binding resolves to the default
    /// profile implicitly — so migration must not fabricate a binding, and an
    /// explicitly bound web app keeps pointing at the same application id.
    #[test]
    fn v28_profile_bindings_stay_consistent_after_remote_migration() {
        let conn = Connection::open_in_memory().unwrap();
        create_tables(&conn).unwrap();
        apply_migrations(&conn).unwrap();
        let now = "2026-01-01T00:00:00Z";

        // v27-era state: two remote non-owned apps; one has an explicit binding,
        // the other relies on the implicit default profile (no binding row).
        conn.execute(
            "INSERT INTO non_owned_apps (id, ownership, url, approved_origins_json, title, created_at, updated_at)
             VALUES ('remote-9', 'remote', 'https://b.example.com/', '[\"b.example.com\"]', 'B', ?1, ?1),
                    ('remote-10', 'remote', 'https://c.example.com/', '[\"c.example.com\"]', 'C', ?1, ?1)",
            [now],
        )
        .unwrap();
        // An explicit binding is only valid once the app id is a real application
        // id; model the migrated identity directly (this is the post-migration
        // state the profile store resolves against).
        conn.execute(
            "INSERT INTO applications (id, source, source_id, title, kind, registration_origin, version, created_at, updated_at)
             VALUES ('remote-10', 'non_owned_remote', 'remote-10', 'C', 'web_application', 'migration', '1', ?1, ?1)",
            [now],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO browser_profile_bindings (application_id, profile_id, created_at, updated_at)
             VALUES ('remote-10', 'default', ?1, ?1)",
            [now],
        )
        .unwrap();

        // Run the v28 backfill: it migrates remote-9 (not remote-10, already present).
        backfill_v28(&conn).unwrap();

        // remote-9 was migrated and now owns the application id.
        let (kind, origin): (String, String) = conn
            .query_row(
                "SELECT kind, registration_origin FROM applications WHERE id = 'remote-9'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(kind, "web_application");
        assert_eq!(origin, "migration");

        // The pre-existing explicit binding is untouched and still resolves to a
        // real application (FK-consistent).
        let bound: String = conn
            .query_row(
                "SELECT b.profile_id FROM browser_profile_bindings b
                 JOIN applications a ON a.id = b.application_id
                 WHERE b.application_id = 'remote-10'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(bound, "default", "explicit binding must stay consistent");

        // remote-9 had no binding and must NOT gain a spurious one.
        let orphan_bindings: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM browser_profile_bindings WHERE application_id = 'remote-9'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            orphan_bindings, 0,
            "migration must not fabricate a profile binding"
        );

        // Idempotent: a second pass adds nothing.
        backfill_v28(&conn).unwrap();
        assert_eq!(count(&conn, "browser_profile_bindings"), 1);
    }
}
