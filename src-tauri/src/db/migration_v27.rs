use crate::Error;
use rusqlite::{Connection, OptionalExtension};

pub(super) fn migrate_v27(conn: &Connection) -> Result<(), Error> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS workspaces (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            kind TEXT NOT NULL DEFAULT 'workspace',
            icon TEXT,
            description TEXT,
            theme TEXT NOT NULL DEFAULT 'dark',
            is_active INTEGER NOT NULL DEFAULT 0,
            position INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_workspaces_active
            ON workspaces(is_active, position);

        CREATE TABLE IF NOT EXISTS workspace_tabs (
            id TEXT PRIMARY KEY,
            workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
            tab_type TEXT NOT NULL,
            title TEXT NOT NULL DEFAULT '',
            ref_id TEXT,
            url TEXT,
            position INTEGER NOT NULL DEFAULT 0,
            is_active INTEGER NOT NULL DEFAULT 0,
            pinned INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_workspace_tabs_workspace
            ON workspace_tabs(workspace_id, position);

        CREATE TABLE IF NOT EXISTS workspace_context_items (
            id TEXT PRIMARY KEY,
            workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
            item_kind TEXT NOT NULL,
            ref_id TEXT NOT NULL,
            title TEXT NOT NULL DEFAULT '',
            meta_json TEXT NOT NULL DEFAULT '{}',
            position INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_workspace_context_workspace
            ON workspace_context_items(workspace_id, position);

        CREATE TABLE IF NOT EXISTS workspace_widgets (
            id TEXT PRIMARY KEY,
            workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
            widget_type TEXT NOT NULL,
            config_json TEXT NOT NULL DEFAULT '{}',
            hidden INTEGER NOT NULL DEFAULT 0,
            position INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_workspace_widgets_workspace
            ON workspace_widgets(workspace_id, position);

        CREATE TABLE IF NOT EXISTS workspace_layouts (
            id TEXT PRIMARY KEY,
            workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
            breakpoint TEXT NOT NULL,
            layout_json TEXT NOT NULL DEFAULT '[]',
            is_active INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            UNIQUE(workspace_id, breakpoint)
        );
        CREATE INDEX IF NOT EXISTS idx_workspace_layouts_workspace
            ON workspace_layouts(workspace_id);

        CREATE TABLE IF NOT EXISTS workspace_view_states (
            id TEXT PRIMARY KEY,
            workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
            view_key TEXT NOT NULL,
            state_json TEXT NOT NULL DEFAULT '{}',
            updated_at TEXT NOT NULL,
            UNIQUE(workspace_id, view_key)
        );
        CREATE INDEX IF NOT EXISTS idx_workspace_view_states_workspace
            ON workspace_view_states(workspace_id);

        CREATE TABLE IF NOT EXISTS workspace_tool_profiles (
            id TEXT PRIMARY KEY,
            workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
            profile_id TEXT NOT NULL,
            tool_key TEXT,
            config_json TEXT NOT NULL DEFAULT '{}',
            enabled INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            UNIQUE(workspace_id, profile_id)
        );
        CREATE INDEX IF NOT EXISTS idx_workspace_tool_profiles_workspace
            ON workspace_tool_profiles(workspace_id);

        INSERT OR REPLACE INTO settings (key, value) VALUES ('_schema_version', '27');
        ",
    )
    .map_err(Error::Database)?;

    import_legacy_home_workspace(conn)?;
    normalize_legacy_theme(conn)?;
    Ok(())
}

/// One-shot, idempotent import of the legacy `settings:home_workspace` JSON
/// document (schemaVersion 1) into the v27 workspace tables.
///
/// The legacy document shape is:
/// `{ schemaVersion: 1, hidden: string[], instances: [{id, widget_type, config}],
///    layouts: { lg: LayoutItem[], md: LayoutItem[], sm: LayoutItem[] } }`.
///
/// Imported as a fixed `home` workspace (kind `home`, active when no other
/// workspace is active). Widgets keep their original ids; layout items are
/// stored as JSON per breakpoint. On success the legacy key is deleted.
fn import_legacy_home_workspace(conn: &Connection) -> Result<(), Error> {
    let Some(raw) = super::get_setting(conn, "settings:home_workspace")? else {
        return Ok(());
    };
    let value: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(_) => {
            // Corrupt legacy document: drop the key and move on, never fail startup.
            let _ = super::delete_setting(conn, "settings:home_workspace");
            return Ok(());
        }
    };

    let now = chrono::Utc::now().to_rfc3339();
    let ws_id = "home";

    let already_imported = conn
        .query_row(
            "SELECT 1 FROM workspaces WHERE id = ?1",
            [ws_id],
            |_| Ok(()),
        )
        .optional()
        .map_err(Error::Database)?;
    if already_imported.is_some() {
        // Import already done for this id — just drop the stale key.
        super::delete_setting(conn, "settings:home_workspace")?;
        return Ok(());
    }

    let name = value.get("name").and_then(|v| v.as_str()).unwrap_or("Home");
    let hidden: std::collections::HashSet<String> = value
        .get("hidden")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    conn.execute(
        "INSERT INTO workspaces
            (id, name, kind, icon, description, theme, is_active, position, created_at, updated_at)
         VALUES (?1, ?2, 'home', NULL, NULL, 'dark', 1, 0, ?3, ?3)",
        rusqlite::params![ws_id, name, now],
    )
    .map_err(Error::Database)?;

    if let Some(instances) = value.get("instances").and_then(|v| v.as_array()) {
        for (i, inst) in instances.iter().enumerate() {
            let Some(id) = inst.get("id").and_then(|v| v.as_str()) else {
                continue;
            };
            let widget_type = inst
                .get("widget_type")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let config = inst
                .get("config")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));
            let is_hidden = hidden.contains(id);
            conn.execute(
                "INSERT INTO workspace_widgets
                    (id, workspace_id, widget_type, config_json, hidden, position, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
                 ON CONFLICT(id) DO NOTHING",
                rusqlite::params![
                    id,
                    ws_id,
                    widget_type,
                    config.to_string(),
                    if is_hidden { 1 } else { 0 },
                    i as i64,
                    now
                ],
            )
            .map_err(Error::Database)?;
        }
    }

    if let Some(layouts) = value.get("layouts").and_then(|v| v.as_object()) {
        for (bp, arr) in layouts {
            let layout_json = match arr.as_array() {
                Some(arr) => serde_json::to_string(arr).unwrap_or_else(|_| "[]".into()),
                None => "[]".into(),
            };
            let layout_id = format!("home-{bp}");
            conn.execute(
                "INSERT INTO workspace_layouts
                    (id, workspace_id, breakpoint, layout_json, is_active, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, 1, ?5, ?5)
                 ON CONFLICT(workspace_id, breakpoint) DO UPDATE SET
                    layout_json = excluded.layout_json,
                    updated_at = excluded.updated_at",
                rusqlite::params![layout_id, ws_id, bp, layout_json, now],
            )
            .map_err(Error::Database)?;
        }
    }

    // The legacy K/V document is no longer authoritative.
    super::delete_setting(conn, "settings:home_workspace")?;
    Ok(())
}

/// V-002/V-004: normalize `settings:theme` to the two-value contract
/// (`dark` | `light`). Legacy aliases (`terminal-volt`, `frosted-jasmine`)
/// and any unknown values are rewritten to the canonical form.
fn normalize_legacy_theme(conn: &Connection) -> Result<(), Error> {
    let Some(theme) = super::get_setting(conn, "settings:theme")? else {
        return Ok(());
    };
    let normalized = match theme.as_str() {
        "dark" | "light" => theme.as_str(),
        "terminal-volt" => "dark",
        "frosted-jasmine" => "light",
        // Unknown values are not persisted noise — fall back to the default.
        _ => "dark",
    };
    if normalized != theme {
        super::set_setting(conn, "settings:theme", normalized)?;
    }
    Ok(())
}

#[cfg(test)]
mod migration_v27_validation {
    //! A-001: v27 workspace 表/索引/FK/UNIQUE 校验（内存临时库，无副作用）。

    use crate::db::{apply_migrations, create_tables};
    use rusqlite::Connection;

    /// 7 张表的期望列集（与 DDL 逐列核对，集合相等，顺序无关）。
    const EXPECTED_COLUMNS: [(&str, &[&str]); 7] = [
        (
            "workspaces",
            &[
                "id",
                "name",
                "kind",
                "icon",
                "description",
                "theme",
                "is_active",
                "position",
                "created_at",
                "updated_at",
            ],
        ),
        (
            "workspace_tabs",
            &[
                "id",
                "workspace_id",
                "tab_type",
                "title",
                "ref_id",
                "url",
                "position",
                "is_active",
                "pinned",
                "created_at",
                "updated_at",
            ],
        ),
        (
            "workspace_context_items",
            &[
                "id",
                "workspace_id",
                "item_kind",
                "ref_id",
                "title",
                "meta_json",
                "position",
                "created_at",
            ],
        ),
        (
            "workspace_widgets",
            &[
                "id",
                "workspace_id",
                "widget_type",
                "config_json",
                "hidden",
                "position",
                "created_at",
                "updated_at",
            ],
        ),
        (
            "workspace_layouts",
            &[
                "id",
                "workspace_id",
                "breakpoint",
                "layout_json",
                "is_active",
                "created_at",
                "updated_at",
            ],
        ),
        (
            "workspace_view_states",
            &["id", "workspace_id", "view_key", "state_json", "updated_at"],
        ),
        (
            "workspace_tool_profiles",
            &[
                "id",
                "workspace_id",
                "profile_id",
                "tool_key",
                "config_json",
                "enabled",
                "created_at",
                "updated_at",
            ],
        ),
    ];

    const INDEXES: [&str; 7] = [
        "idx_workspaces_active",
        "idx_workspace_tabs_workspace",
        "idx_workspace_context_workspace",
        "idx_workspace_widgets_workspace",
        "idx_workspace_layouts_workspace",
        "idx_workspace_view_states_workspace",
        "idx_workspace_tool_profiles_workspace",
    ];

    fn migrated() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        create_tables(&conn).unwrap();
        apply_migrations(&conn).unwrap();
        conn
    }

    fn columns(conn: &Connection, table: &str) -> Vec<String> {
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info({table})"))
            .unwrap();
        stmt.query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    }

    #[test]
    fn v27_tables_columns_indexes_match_contract() {
        let conn = migrated();

        for (table, expected) in &EXPECTED_COLUMNS {
            let mut have = columns(&conn, table);
            have.sort();
            let mut want: Vec<String> = expected.iter().map(|s| (*s).to_string()).collect();
            want.sort();
            assert_eq!(have, want, "table {table} column set mismatch");
        }

        let index_names: Vec<String> = {
            let mut stmt = conn
                .prepare(
                    "SELECT name FROM sqlite_master
                     WHERE type = 'index' AND name LIKE 'idx_workspace%'
                     ORDER BY name",
                )
                .unwrap();
            stmt.query_map([], |r| r.get::<_, String>(0))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        };
        for want in &INDEXES {
            assert!(
                index_names.iter().any(|n| n == want),
                "missing index {want}"
            );
        }
        assert_eq!(index_names.len(), 7, "unexpected extra workspace indexes");
    }

    fn seed_workspace(conn: &Connection, id: &str, now: &str) {
        conn.execute(
            "INSERT INTO workspaces (id, name, kind, theme, is_active, position, created_at, updated_at)
             VALUES (?1, ?1, 'workspace', 'dark', 0, 0, ?2, ?2)",
            rusqlite::params![id, now],
        )
        .unwrap();
    }

    /// 在 6 张子表各插一行（id 形如 `{ws}_t` 等），供级联删除断言。
    fn seed_child_rows(conn: &Connection, ws: &str, now: &str) {
        conn.execute(
            "INSERT INTO workspace_tabs (id, workspace_id, tab_type, title, position, is_active, pinned, created_at, updated_at)
             VALUES (?1, ?2, 'home', 'Home', 0, 1, 0, ?3, ?3)",
            rusqlite::params![format!("{ws}_t"), ws, now],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO workspace_context_items (id, workspace_id, item_kind, ref_id, title, meta_json, position, created_at)
             VALUES (?1, ?2, 'note', 'ref:1', 'n', '{}', 0, ?3)",
            rusqlite::params![format!("{ws}_c"), ws, now],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO workspace_widgets (id, workspace_id, widget_type, config_json, hidden, position, created_at, updated_at)
             VALUES (?1, ?2, 'clock', '{}', 0, 0, ?3, ?3)",
            rusqlite::params![format!("{ws}_w"), ws, now],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO workspace_layouts (id, workspace_id, breakpoint, layout_json, is_active, created_at, updated_at)
             VALUES (?1, ?2, 'lg', '[]', 1, ?3, ?3)",
            rusqlite::params![format!("{ws}_l"), ws, now],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO workspace_view_states (id, workspace_id, view_key, state_json, updated_at)
             VALUES (?1, ?2, 'dv', '{}', ?3)",
            rusqlite::params![format!("{ws}_v"), ws, now],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO workspace_tool_profiles (id, workspace_id, profile_id, config_json, enabled, created_at, updated_at)
             VALUES (?1, ?2, 'prof_1', '{}', 1, ?3, ?3)",
            rusqlite::params![format!("{ws}_p"), ws, now],
        )
        .unwrap();
    }

    #[test]
    fn v27_unique_constraints_reject_duplicates() {
        let conn = migrated();
        let now = "2026-01-01T00:00:00Z";
        seed_workspace(&conn, "ws_a", now);

        // UNIQUE(workspace_id, breakpoint)
        conn.execute(
            "INSERT INTO workspace_layouts (id, workspace_id, breakpoint, layout_json, is_active, created_at, updated_at)
             VALUES ('lay_1', 'ws_a', 'lg', '[]', 1, ?1, ?1)",
            [now],
        )
        .unwrap();
        assert!(
            conn.execute(
                "INSERT INTO workspace_layouts (id, workspace_id, breakpoint, layout_json, is_active, created_at, updated_at)
                 VALUES ('lay_2', 'ws_a', 'lg', '[]', 1, ?1, ?1)",
                [now],
            )
            .is_err(),
            "duplicate (workspace_id, breakpoint) must be rejected"
        );

        // UNIQUE(workspace_id, view_key)
        conn.execute(
            "INSERT INTO workspace_view_states (id, workspace_id, view_key, state_json, updated_at)
             VALUES ('vs_1', 'ws_a', 'dv', '{}', ?1)",
            [now],
        )
        .unwrap();
        assert!(
            conn.execute(
                "INSERT INTO workspace_view_states (id, workspace_id, view_key, state_json, updated_at)
                 VALUES ('vs_2', 'ws_a', 'dv', '{}', ?1)",
                [now],
            )
            .is_err(),
            "duplicate (workspace_id, view_key) must be rejected"
        );

        // UNIQUE(workspace_id, profile_id)
        conn.execute(
            "INSERT INTO workspace_tool_profiles (id, workspace_id, profile_id, config_json, enabled, created_at, updated_at)
             VALUES ('tp_1', 'ws_a', 'prof_1', '{}', 1, ?1, ?1)",
            [now],
        )
        .unwrap();
        assert!(
            conn.execute(
                "INSERT INTO workspace_tool_profiles (id, workspace_id, profile_id, config_json, enabled, created_at, updated_at)
                 VALUES ('tp_2', 'ws_a', 'prof_1', '{}', 1, ?1, ?1)",
                [now],
            )
            .is_err(),
            "duplicate (workspace_id, profile_id) must be rejected"
        );

        // 同 breakpoint 属于不同 workspace 时不冲突（UNIQUE 作用域正确）。
        seed_workspace(&conn, "ws_b", now);
        conn.execute(
            "INSERT INTO workspace_layouts (id, workspace_id, breakpoint, layout_json, is_active, created_at, updated_at)
             VALUES ('lay_b', 'ws_b', 'lg', '[]', 1, ?1, ?1)",
            [now],
        )
        .unwrap();
    }

    #[test]
    fn v27_fk_cascade_deletes_all_children() {
        let conn = migrated();
        conn.execute("PRAGMA foreign_keys = ON", []).unwrap();
        let now = "2026-01-01T00:00:00Z";
        seed_workspace(&conn, "ws_a", now);
        seed_child_rows(&conn, "ws_a", now);

        for (table, id) in [
            ("workspace_tabs", "ws_a_t"),
            ("workspace_context_items", "ws_a_c"),
            ("workspace_widgets", "ws_a_w"),
            ("workspace_layouts", "ws_a_l"),
            ("workspace_view_states", "ws_a_v"),
            ("workspace_tool_profiles", "ws_a_p"),
        ] {
            let n: i64 = conn
                .query_row(
                    &format!("SELECT COUNT(*) FROM {table} WHERE id = ?1"),
                    [id],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 1, "{table} seed row missing");
        }

        conn.execute("DELETE FROM workspaces WHERE id = 'ws_a'", [])
            .unwrap();

        for (table, id) in [
            ("workspace_tabs", "ws_a_t"),
            ("workspace_context_items", "ws_a_c"),
            ("workspace_widgets", "ws_a_w"),
            ("workspace_layouts", "ws_a_l"),
            ("workspace_view_states", "ws_a_v"),
            ("workspace_tool_profiles", "ws_a_p"),
        ] {
            let n: i64 = conn
                .query_row(
                    &format!("SELECT COUNT(*) FROM {table} WHERE id = ?1"),
                    [id],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 0, "{table} row {id} must be cascade-deleted");
        }
    }

    #[test]
    fn v27_legacy_seed_import_and_idempotent() {
        // 先植入 legacy K/V（空文档即可：name 缺省 "Home"，widgets/layouts 缺省空）。
        let conn = Connection::open_in_memory().unwrap();
        create_tables(&conn).unwrap();
        let now = "2026-01-01T00:00:00Z";
        conn.execute(
            "INSERT INTO settings (key, value, updated_at) VALUES ('settings:home_workspace', ?1, ?2)",
            rusqlite::params!["{}", now],
        )
        .unwrap();

        apply_migrations(&conn).unwrap();

        // 旧 K/V 已删除。
        let stale: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM settings WHERE key = 'settings:home_workspace'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            stale, 0,
            "settings:home_workspace must be deleted after import"
        );

        // 种子行：id='home', name='Home', kind='home', theme='dark', is_active=1, position=0。
        let (id, name, kind, theme, active, pos): (String, String, String, String, i64, i64) = conn
            .query_row(
                "SELECT id, name, kind, theme, is_active, position FROM workspaces",
                [],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(
            (id.as_str(), name.as_str(), kind.as_str()),
            ("home", "Home", "home")
        );
        assert_eq!(theme, "dark");
        assert_eq!(active, 1);
        assert_eq!(pos, 0);

        // 恰好 1 行。
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM workspaces", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1, "seed must create exactly one workspace");

        // 幂等：重复执行整条迁移链，行数不变、不报错、K/V 仍已删除。
        apply_migrations(&conn).unwrap();
        let count2: i64 = conn
            .query_row("SELECT COUNT(*) FROM workspaces", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count2, 1, "migration must be idempotent");
        let stale2: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM settings WHERE key = 'settings:home_workspace'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stale2, 0);
    }
}
