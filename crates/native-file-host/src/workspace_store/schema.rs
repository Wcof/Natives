use rusqlite::Connection;
use serde_json::Value;
use std::path::PathBuf;

use super::types::{StoreResult, WidgetRecord, WorkspaceError};

pub const WIDGET_KEYS: &[&str] = &[
    "widget/binaryTime",
    "widget/bitcoin",
    "widget/bookmarks",
    "widget/countdown",
    "widget/css",
    "widget/currencyRates",
    "widget/customText",
    "widget/github",
    "widget/greeting",
    "widget/html",
    "widget/ipInfo",
    "widget/joke",
    "widget/leetcode",
    "widget/links",
    "widget/literatureClock",
    "widget/message",
    "widget/notes",
    "widget/palette",
    "widget/quote",
    "widget/search",
    "widget/since",
    "widget/tallyCounter",
    "widget/time",
    "widget/timeTracker",
    "widget/todo",
    "widget/topSites",
    "widget/trello",
    "widget/weather",
    "widget/workHours",
];

pub const BACKGROUND_KEYS: &[&str] = &[
    "background/apod",
    "background/bing",
    "background/colour",
    "background/giphy",
    "background/gradient",
    "background/media",
    "background/online",
    "background/unsplash",
    "background/wikimedia",
];

pub const POSITIONS: &[&str] = &[
    "topLeft",
    "topCentre",
    "topRight",
    "middleLeft",
    "middleCentre",
    "middleRight",
    "bottomLeft",
    "bottomCentre",
    "bottomRight",
    "free",
];

pub const SETTINGS_KEYS: &[&str] = &[
    "theme",
    "accent",
    "locale",
    "timeZone",
    "favicon",
    "highlightingEnabled",
    "hideSettingsIcon",
    "settingsIconPosition",
    "themePreference",
    "autoHideSettings",
];

pub const MAX_JSON_BYTES: usize = 64 * 1024;
pub const MAX_WIDGETS_PER_WORKSPACE: usize = 64;

pub fn default_db_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".natives").join("natives.db")
}

pub fn validate_widget_key(key: &str) -> StoreResult<()> {
    if WIDGET_KEYS.contains(&key) {
        Ok(())
    } else {
        Err(WorkspaceError::InvalidInput(format!(
            "unknown widget key: {key}"
        )))
    }
}

pub fn validate_background_key(key: &str) -> StoreResult<()> {
    if BACKGROUND_KEYS.contains(&key) {
        Ok(())
    } else {
        Err(WorkspaceError::InvalidInput(format!(
            "unknown background key: {key}"
        )))
    }
}

pub fn validate_position(position: &str) -> StoreResult<()> {
    if POSITIONS.contains(&position) {
        Ok(())
    } else {
        Err(WorkspaceError::InvalidInput(format!(
            "unknown position: {position}"
        )))
    }
}

pub fn validate_settings_key(key: &str) -> StoreResult<()> {
    if SETTINGS_KEYS.contains(&key) {
        Ok(())
    } else {
        Err(WorkspaceError::InvalidInput(format!(
            "disallowed settings key: {key}"
        )))
    }
}

pub fn validate_json_size(value: &Value, label: &str) -> StoreResult<()> {
    let size = serde_json::to_string(value).map(|s| s.len()).unwrap_or(0);
    if size > MAX_JSON_BYTES {
        Err(WorkspaceError::InvalidInput(format!(
            "{label} JSON exceeds {MAX_JSON_BYTES} bytes ({size})"
        )))
    } else {
        Ok(())
    }
}

pub fn validate_widget_record(widget: &WidgetRecord) -> StoreResult<()> {
    validate_widget_key(&widget.key)?;
    validate_json_size(&widget.config_json, "config")?;
    validate_json_size(&widget.display_json, "display")?;
    if let Some(position) = widget.display_json.get("position").and_then(Value::as_str) {
        validate_position(position)?;
    }
    Ok(())
}

pub(crate) fn table_has_column(conn: &Connection, table: &str, column: &str) -> StoreResult<bool> {
    let mut statement = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let names = statement.query_map([], |row| row.get::<_, String>(1))?;
    for name in names {
        if name? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

fn ensure_column(
    conn: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> StoreResult<bool> {
    if !table_has_column(conn, table, column)? {
        conn.execute_batch(&format!(
            "ALTER TABLE \"{table}\" ADD COLUMN \"{column}\" {definition};"
        ))?;
        return Ok(true);
    }
    Ok(false)
}

pub fn migrate_db(conn: &Connection) -> StoreResult<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute_batch(
        r#"
            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
            );
            CREATE TABLE IF NOT EXISTS workspaces (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                sort_order INTEGER NOT NULL DEFAULT 0,
                pinned INTEGER NOT NULL DEFAULT 0,
                background_json TEXT NOT NULL DEFAULT '{}',
                template_source TEXT,
                deleted_at TEXT,
                revision INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
                updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
            );
            CREATE TABLE IF NOT EXISTS workspace_open_tabs (
                workspace_id TEXT PRIMARY KEY REFERENCES workspaces(id) ON DELETE CASCADE,
                sort_order INTEGER NOT NULL DEFAULT 0,
                is_pinned INTEGER NOT NULL DEFAULT 0,
                opened_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
                last_active_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
            );
            CREATE TABLE IF NOT EXISTS workspace_widgets (
                id TEXT PRIMARY KEY,
                workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
                key TEXT NOT NULL,
                "order" INTEGER NOT NULL DEFAULT 0,
                enabled INTEGER NOT NULL DEFAULT 1,
                config_json TEXT NOT NULL DEFAULT '{}',
                display_json TEXT NOT NULL DEFAULT '{}',
                config_version INTEGER NOT NULL DEFAULT 1
            );
            CREATE TABLE IF NOT EXISTS workspace_templates (
                id TEXT PRIMARY KEY,
                origin TEXT NOT NULL CHECK (origin IN ('builtin','personal')),
                name TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
                updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
                deleted_at TEXT
            );
            CREATE TABLE IF NOT EXISTS workspace_meta (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                revision INTEGER NOT NULL DEFAULT 1,
                active_workspace_id TEXT
            );
            INSERT OR IGNORE INTO workspace_meta (id, revision) VALUES (1, 1);
            "#,
    )?;

    // Older builds could create this index before the quoted `order` column
    // existed. Drop the rebuildable index before SQLite reparses the schema.
    tx.execute_batch("DROP INDEX IF EXISTS idx_widgets_workspace;")?;

    // `PRAGMA user_version` belongs to the shared historical database and cannot
    // identify this store's schema. Upgrade by capability instead.
    let added_workspace_sort_order = ensure_column(
        &tx,
        "workspaces",
        "sort_order",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(&tx, "workspaces", "pinned", "INTEGER NOT NULL DEFAULT 0")?;
    ensure_column(
        &tx,
        "workspaces",
        "background_json",
        "TEXT NOT NULL DEFAULT '{}'",
    )?;
    ensure_column(&tx, "workspaces", "template_source", "TEXT")?;
    ensure_column(&tx, "workspaces", "revision", "INTEGER NOT NULL DEFAULT 1")?;
    let added_widget_key =
        ensure_column(&tx, "workspace_widgets", "key", "TEXT NOT NULL DEFAULT ''")?;
    let added_widget_order = ensure_column(
        &tx,
        "workspace_widgets",
        "order",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    let added_widget_display = ensure_column(
        &tx,
        "workspace_widgets",
        "display_json",
        "TEXT NOT NULL DEFAULT '{}'",
    )?;
    ensure_column(
        &tx,
        "workspace_templates",
        "payload_json",
        "TEXT NOT NULL DEFAULT '{}'",
    )?;

    if added_workspace_sort_order && table_has_column(&tx, "workspaces", "position")? {
        tx.execute(
            "UPDATE workspaces SET sort_order = position WHERE sort_order = 0",
            [],
        )?;
    }
    if table_has_column(&tx, "workspace_widgets", "widget_type")? {
        if added_widget_key {
            tx.execute_batch(
                r#"
            UPDATE workspace_widgets SET
              key = CASE widget_type
                WHEN 'greeting' THEN 'widget/greeting'
                WHEN 'notes' THEN 'widget/notes'
                WHEN 'quick_links' THEN 'widget/links'
                WHEN 'work_time' THEN 'widget/workHours'
                ELSE 'legacy/' || widget_type
              END
            WHERE key = '';
            "#,
            )?;
        }
        if added_widget_order {
            tx.execute("UPDATE workspace_widgets SET \"order\" = position", [])?;
        }
        if added_widget_display {
            tx.execute(
                "UPDATE workspace_widgets SET display_json = appearance_json",
                [],
            )?;
        }
    }

    let workspace_count: i64 = tx.query_row(
        "SELECT COUNT(*) FROM workspaces WHERE deleted_at IS NULL",
        [],
        |row| row.get(0),
    )?;
    if workspace_count == 0 {
        let id = uuid_v4();
        tx.execute(
            "INSERT INTO workspaces (id, name, sort_order, background_json, template_source, created_at, updated_at)
             VALUES (?1, 'Personal Space', 0, '{\"key\":\"background/colour\",\"display\":{\"colour\":\"#101010\"}}', 'blank', strftime('%Y-%m-%dT%H:%M:%fZ','now'), strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
            [&id],
        )?;
        tx.execute(
            "INSERT INTO workspace_open_tabs (workspace_id, sort_order, opened_at, last_active_at)
             VALUES (?1, 0, strftime('%Y-%m-%dT%H:%M:%fZ','now'), strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
            [&id],
        )?;
        tx.execute(
            "UPDATE workspace_meta SET active_workspace_id = ?1 WHERE id = 1",
            [&id],
        )?;
    } else {
        let open_count: i64 = tx.query_row(
            "SELECT COUNT(*) FROM workspace_open_tabs t JOIN workspaces w ON w.id = t.workspace_id WHERE w.deleted_at IS NULL",
            [],
            |row| row.get(0),
        )?;
        if open_count == 0 {
            tx.execute(
                "INSERT INTO workspace_open_tabs (workspace_id, sort_order, opened_at, last_active_at)
                 SELECT id, 0, strftime('%Y-%m-%dT%H:%M:%fZ','now'), strftime('%Y-%m-%dT%H:%M:%fZ','now')
                 FROM workspaces WHERE deleted_at IS NULL ORDER BY sort_order, name LIMIT 1",
                [],
            )?;
        }
        tx.execute(
            "UPDATE workspace_meta SET active_workspace_id = COALESCE(
               (SELECT active_workspace_id FROM workspace_meta WHERE id = 1 AND active_workspace_id IN (SELECT id FROM workspaces WHERE deleted_at IS NULL)),
               (SELECT workspace_id FROM workspace_open_tabs ORDER BY is_pinned DESC, sort_order ASC LIMIT 1)
             ) WHERE id = 1",
            [],
        )?;
    }
    tx.execute_batch(
        "DROP INDEX IF EXISTS idx_widgets_workspace;
         CREATE INDEX idx_widgets_workspace ON workspace_widgets(workspace_id, \"order\");",
    )?;
    tx.commit()?;
    Ok(())
}

pub fn uuid_v4() -> String {
    let bytes = random_bytes_16();
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-4{:01x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5],
        bytes[6] & 0x0f, bytes[7],
        (bytes[8] & 0x3f) | 0x80, bytes[9],
        bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    )
}

fn random_bytes_16() -> [u8; 16] {
    use std::fs::File;
    use std::io::Read;
    let mut bytes = [0u8; 16];
    if let Ok(mut file) = File::open("/dev/urandom") {
        if file.read_exact(&mut bytes).is_ok() {
            return bytes;
        }
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = ((nanos >> (index * 8)) & 0xff) as u8;
    }
    bytes
}
