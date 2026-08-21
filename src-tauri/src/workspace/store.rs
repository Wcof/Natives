//! Workspace V2 — raw SQL CRUD over the seven v27 workspace tables.
//!
//! Every function takes a `&Connection`; callers (commands) hand in a pooled
//! connection from `AppState.db`. Nothing here is async — Tauri commands run on
//! the main thread and the pool provides short-lived connections.
//!
//! Host-generated ids are opaque (never derived from secrets) and unique
//! enough for local single-user operation: `<prefix>_<monotonic_nanos>_<seq>`.

use super::types::{
    WorkspaceContextItem, WorkspaceContextItemInput, WorkspaceLayout, WorkspaceSummary,
    WorkspaceTab, WorkspaceTabInput, WorkspaceTabUpdate, WorkspaceToolProfile, WorkspaceUpdateRequest,
    WorkspaceViewState, WorkspaceWidget, WorkspaceWidgetInput, normalize_theme,
};
use crate::{Error, Result};
use rusqlite::{Connection, OptionalExtension};
use std::sync::atomic::{AtomicU64, Ordering};

static ID_COUNTER: AtomicU64 = AtomicU64::new(0);

fn new_id(prefix: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}_{nanos:x}_{seq:x}")
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

// ──────────────────────────────────────────────
// Row mappers
// ──────────────────────────────────────────────

fn row_to_workspace(row: &rusqlite::Row) -> rusqlite::Result<WorkspaceSummary> {
    Ok(WorkspaceSummary {
        id: row.get(0)?,
        name: row.get(1)?,
        kind: row.get(2)?,
        icon: row.get(3)?,
        description: row.get(4)?,
        theme: row.get(5)?,
        is_active: row.get::<_, i64>(6)? != 0,
        position: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
    })
}

fn row_to_tab(row: &rusqlite::Row) -> rusqlite::Result<WorkspaceTab> {
    Ok(WorkspaceTab {
        id: row.get(0)?,
        workspace_id: row.get(1)?,
        tab_type: row.get(2)?,
        title: row.get(3)?,
        ref_id: row.get(4)?,
        url: row.get(5)?,
        position: row.get(6)?,
        is_active: row.get::<_, i64>(7)? != 0,
        pinned: row.get::<_, i64>(8)? != 0,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

fn row_to_context_item(row: &rusqlite::Row) -> rusqlite::Result<WorkspaceContextItem> {
    Ok(WorkspaceContextItem {
        id: row.get(0)?,
        workspace_id: row.get(1)?,
        item_kind: row.get(2)?,
        ref_id: row.get(3)?,
        title: row.get(4)?,
        meta: parse_json(row.get::<_, String>(5)?),
        position: row.get(6)?,
        created_at: row.get(7)?,
    })
}

fn row_to_widget(row: &rusqlite::Row) -> rusqlite::Result<WorkspaceWidget> {
    Ok(WorkspaceWidget {
        id: row.get(0)?,
        workspace_id: row.get(1)?,
        widget_type: row.get(2)?,
        config: parse_json(row.get::<_, String>(3)?),
        hidden: row.get::<_, i64>(4)? != 0,
        position: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

fn row_to_layout(row: &rusqlite::Row) -> rusqlite::Result<WorkspaceLayout> {
    Ok(WorkspaceLayout {
        id: row.get(0)?,
        workspace_id: row.get(1)?,
        breakpoint: row.get(2)?,
        layout: parse_json(row.get::<_, String>(3)?),
        is_active: row.get::<_, i64>(4)? != 0,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

fn row_to_view_state(row: &rusqlite::Row) -> rusqlite::Result<WorkspaceViewState> {
    Ok(WorkspaceViewState {
        id: row.get(0)?,
        workspace_id: row.get(1)?,
        view_key: row.get(2)?,
        state: parse_json(row.get::<_, String>(3)?),
        updated_at: row.get(4)?,
    })
}

fn row_to_tool_profile(row: &rusqlite::Row) -> rusqlite::Result<WorkspaceToolProfile> {
    Ok(WorkspaceToolProfile {
        id: row.get(0)?,
        workspace_id: row.get(1)?,
        profile_id: row.get(2)?,
        tool_key: row.get(3)?,
        config: parse_json(row.get::<_, String>(4)?),
        enabled: row.get::<_, i64>(5)? != 0,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

/// Best-effort JSON column parse; corrupt cells degrade to `{}` / `[]` rather
/// than failing the whole read (defensive, keeps rows recoverable).
fn parse_json(raw: String) -> serde_json::Value {
    serde_json::from_str(&raw).unwrap_or_else(|_| serde_json::json!({}))
}

const WORKSPACE_COLS: &str =
    "id, name, kind, icon, description, theme, is_active, position, created_at, updated_at";
const TAB_COLS: &str =
    "id, workspace_id, tab_type, title, ref_id, url, position, is_active, pinned, created_at, updated_at";
const CONTEXT_COLS: &str =
    "id, workspace_id, item_kind, ref_id, title, meta_json, position, created_at";
const WIDGET_COLS: &str =
    "id, workspace_id, widget_type, config_json, hidden, position, created_at, updated_at";
const LAYOUT_COLS: &str =
    "id, workspace_id, breakpoint, layout_json, is_active, created_at, updated_at";
const VIEW_STATE_COLS: &str = "id, workspace_id, view_key, state_json, updated_at";
const TOOL_PROFILE_COLS: &str =
    "id, workspace_id, profile_id, tool_key, config_json, enabled, created_at, updated_at";

// ──────────────────────────────────────────────
// Workspaces
// ──────────────────────────────────────────────

pub fn list_workspaces(conn: &Connection) -> Result<Vec<WorkspaceSummary>> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {WORKSPACE_COLS} FROM workspaces ORDER BY position ASC, created_at ASC"
        ))
        .map_err(Error::Database)?;
    let rows = stmt
        .query_map([], row_to_workspace)
        .map_err(Error::Database)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Error::Database)
}

pub fn get_workspace(conn: &Connection, id: &str) -> Result<Option<WorkspaceSummary>> {
    conn.query_row(
        &format!("SELECT {WORKSPACE_COLS} FROM workspaces WHERE id = ?1"),
        [id],
        row_to_workspace,
    )
    .optional()
    .map_err(Error::Database)
}

pub fn create_workspace(
    conn: &Connection,
    name: &str,
    kind: &str,
    icon: Option<&str>,
    description: Option<&str>,
    theme: &str,
) -> Result<WorkspaceSummary> {
    let id = new_id("ws");
    let now = now_rfc3339();
    let theme = normalize_theme(theme).to_string();
    let position = {
        let max_pos: Option<i64> = conn
            .query_row("SELECT MAX(position) FROM workspaces", [], |r| r.get(0))
            .optional()
            .map_err(Error::Database)?;
        max_pos.map(|p| p + 1).unwrap_or(0)
    };
    conn.execute(
        "INSERT INTO workspaces
            (id, name, kind, icon, description, theme, is_active, position, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7, ?8, ?8)",
        rusqlite::params![id, name, kind, icon, description, theme, position, now],
    )
    .map_err(Error::Database)?;
    get_workspace(conn, &id)?.ok_or_else(|| {
        Error::Internal(format!("workspace {id} was created but not readable"))
    })
}

pub fn update_workspace(
    conn: &Connection,
    id: &str,
    patch: &WorkspaceUpdateRequest,
) -> Result<Option<WorkspaceSummary>> {
    let Some(existing) = get_workspace(conn, id)? else {
        return Ok(None);
    };
    let name = patch.name.as_deref().unwrap_or(&existing.name);
    // Empty string clears nullable columns.
    let icon = match patch.icon.as_deref() {
        Some("") => None,
        Some(v) => Some(v.to_string()),
        None => existing.icon,
    };
    let description = match patch.description.as_deref() {
        Some("") => None,
        Some(v) => Some(v.to_string()),
        None => existing.description,
    };
    let theme = patch
        .theme
        .as_deref()
        .map(normalize_theme)
        .unwrap_or(&existing.theme)
        .to_string();
    let position = patch.position.unwrap_or(existing.position);
    let now = now_rfc3339();
    conn.execute(
        "UPDATE workspaces
            SET name = ?1, icon = ?2, description = ?3, theme = ?4, position = ?5, updated_at = ?6
          WHERE id = ?7",
        rusqlite::params![name, icon, description, theme, position, now, id],
    )
    .map_err(Error::Database)?;
    get_workspace(conn, id)
}

pub fn delete_workspace(conn: &Connection, id: &str) -> Result<bool> {
    let changed = conn
        .execute("DELETE FROM workspaces WHERE id = ?1", [id])
        .map_err(Error::Database)?;
    Ok(changed > 0)
}

pub fn set_active_workspace(conn: &Connection, id: &str) -> Result<Option<WorkspaceSummary>> {
    if get_workspace(conn, id)?.is_none() {
        return Ok(None);
    }
    let now = now_rfc3339();
    conn.execute(
        "UPDATE workspaces SET is_active = 0, updated_at = ?1",
        [&now],
    )
    .map_err(Error::Database)?;
    conn.execute(
        "UPDATE workspaces SET is_active = 1, updated_at = ?1 WHERE id = ?2",
        rusqlite::params![&now, id],
    )
    .map_err(Error::Database)?;
    get_workspace(conn, id)
}

// ──────────────────────────────────────────────
// Tabs
// ──────────────────────────────────────────────

pub fn list_tabs(conn: &Connection, workspace_id: &str) -> Result<Vec<WorkspaceTab>> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {TAB_COLS} FROM workspace_tabs WHERE workspace_id = ?1 ORDER BY position ASC, created_at ASC"
        ))
        .map_err(Error::Database)?;
    let rows = stmt
        .query_map([workspace_id], row_to_tab)
        .map_err(Error::Database)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Error::Database)
}

pub fn get_tab(conn: &Connection, id: &str) -> Result<Option<WorkspaceTab>> {
    conn.query_row(
        &format!("SELECT {TAB_COLS} FROM workspace_tabs WHERE id = ?1"),
        [id],
        row_to_tab,
    )
    .optional()
    .map_err(Error::Database)
}

pub fn create_tab(
    conn: &Connection,
    workspace_id: &str,
    input: &WorkspaceTabInput,
) -> Result<Option<WorkspaceTab>> {
    if get_workspace(conn, workspace_id)?.is_none() {
        return Ok(None);
    }
    let id = new_id("tab");
    let now = now_rfc3339();
    let position = {
        let max_pos: Option<i64> = conn
            .query_row(
                "SELECT MAX(position) FROM workspace_tabs WHERE workspace_id = ?1",
                [workspace_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(Error::Database)?;
        max_pos.map(|p| p + 1).unwrap_or(0)
    };
    let title = input.title.clone().unwrap_or_default();
    let tab_type = input.tab_type.clone();
    conn.execute(
        "INSERT INTO workspace_tabs
            (id, workspace_id, tab_type, title, ref_id, url, position, is_active, pinned, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, 0, ?8, ?8)",
        rusqlite::params![
            id,
            workspace_id,
            tab_type,
            title,
            input.ref_id.clone(),
            input.url.clone(),
            position,
            now
        ],
    )
    .map_err(Error::Database)?;
    get_tab(conn, &id)
}

pub fn update_tab(
    conn: &Connection,
    id: &str,
    patch: &WorkspaceTabUpdate,
) -> Result<Option<WorkspaceTab>> {
    let Some(existing) = get_tab(conn, id)? else {
        return Ok(None);
    };
    let title = match patch.title.as_deref() {
        Some(v) if !v.is_empty() => v.to_string(),
        _ => existing.title,
    };
    // Empty string clears nullable columns.
    let ref_id = match patch.ref_id.as_deref() {
        Some("") => None,
        Some(v) => Some(v.to_string()),
        None => existing.ref_id,
    };
    let url = match patch.url.as_deref() {
        Some("") => None,
        Some(v) => Some(v.to_string()),
        None => existing.url,
    };
    let is_active = patch.is_active.unwrap_or(existing.is_active);
    let pinned = patch.pinned.unwrap_or(existing.pinned);
    let position = patch.position.unwrap_or(existing.position);
    let now = now_rfc3339();
    conn.execute(
        "UPDATE workspace_tabs
            SET title = ?1, ref_id = ?2, url = ?3, is_active = ?4, pinned = ?5,
                position = ?6, updated_at = ?7
          WHERE id = ?8",
        rusqlite::params![
            title,
            ref_id,
            url,
            i64::from(is_active),
            i64::from(pinned),
            position,
            now,
            id
        ],
    )
    .map_err(Error::Database)?;
    get_tab(conn, id)
}

pub fn close_tab(conn: &Connection, id: &str) -> Result<bool> {
    let changed = conn
        .execute("DELETE FROM workspace_tabs WHERE id = ?1", [id])
        .map_err(Error::Database)?;
    Ok(changed > 0)
}

/// Reorder tabs in a workspace by writing `position = idx` for each id.
/// Missing ids are ignored (they may belong to another workspace).
pub fn reorder_tabs(
    conn: &Connection,
    workspace_id: &str,
    ordered_ids: &[String],
) -> Result<Vec<WorkspaceTab>> {
    for (idx, tab_id) in ordered_ids.iter().enumerate() {
        conn.execute(
            "UPDATE workspace_tabs SET position = ?1, updated_at = ?2
              WHERE id = ?3 AND workspace_id = ?4",
            rusqlite::params![
                idx as i64,
                now_rfc3339(),
                tab_id,
                workspace_id
            ],
        )
        .map_err(Error::Database)?;
    }
    list_tabs(conn, workspace_id)
}

// ──────────────────────────────────────────────
// Context items
// ──────────────────────────────────────────────

pub fn list_context_items(
    conn: &Connection,
    workspace_id: &str,
) -> Result<Vec<WorkspaceContextItem>> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {CONTEXT_COLS} FROM workspace_context_items WHERE workspace_id = ?1 ORDER BY position ASC, created_at ASC"
        ))
        .map_err(Error::Database)?;
    let rows = stmt
        .query_map([workspace_id], row_to_context_item)
        .map_err(Error::Database)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Error::Database)
}

pub fn add_context_item(
    conn: &Connection,
    workspace_id: &str,
    input: &WorkspaceContextItemInput,
) -> Result<Option<WorkspaceContextItem>> {
    if get_workspace(conn, workspace_id)?.is_none() {
        return Ok(None);
    }
    let id = new_id("ctx");
    let now = now_rfc3339();
    let position = {
        let max_pos: Option<i64> = conn
            .query_row(
                "SELECT MAX(position) FROM workspace_context_items WHERE workspace_id = ?1",
                [workspace_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(Error::Database)?;
        max_pos.map(|p| p + 1).unwrap_or(0)
    };
    let title = input.title.clone().unwrap_or_default();
    let meta = input
        .meta
        .clone()
        .unwrap_or_else(|| serde_json::json!({}))
        .to_string();
    conn.execute(
        "INSERT INTO workspace_context_items
            (id, workspace_id, item_kind, ref_id, title, meta_json, position, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![
            id,
            workspace_id,
            input.item_kind.clone(),
            input.ref_id.clone(),
            title,
            meta,
            position,
            now
        ],
    )
    .map_err(Error::Database)?;
    get_context_item(conn, &id)
}

pub fn get_context_item(conn: &Connection, id: &str) -> Result<Option<WorkspaceContextItem>> {
    conn.query_row(
        &format!("SELECT {CONTEXT_COLS} FROM workspace_context_items WHERE id = ?1"),
        [id],
        row_to_context_item,
    )
    .optional()
    .map_err(Error::Database)
}

pub fn remove_context_item(conn: &Connection, workspace_id: &str, item_id: &str) -> Result<bool> {
    let changed = conn
        .execute(
            "DELETE FROM workspace_context_items WHERE id = ?1 AND workspace_id = ?2",
            rusqlite::params![item_id, workspace_id],
        )
        .map_err(Error::Database)?;
    Ok(changed > 0)
}

// ──────────────────────────────────────────────
// Widgets
// ──────────────────────────────────────────────

pub fn list_widgets(conn: &Connection, workspace_id: &str) -> Result<Vec<WorkspaceWidget>> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {WIDGET_COLS} FROM workspace_widgets WHERE workspace_id = ?1 ORDER BY position ASC, created_at ASC"
        ))
        .map_err(Error::Database)?;
    let rows = stmt
        .query_map([workspace_id], row_to_widget)
        .map_err(Error::Database)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Error::Database)
}

pub fn get_widget(conn: &Connection, id: &str) -> Result<Option<WorkspaceWidget>> {
    conn.query_row(
        &format!("SELECT {WIDGET_COLS} FROM workspace_widgets WHERE id = ?1"),
        [id],
        row_to_widget,
    )
    .optional()
    .map_err(Error::Database)
}

/// Insert (when `id` is absent) or update (when `id` is present) a widget.
pub fn upsert_widget(
    conn: &Connection,
    workspace_id: &str,
    input: &WorkspaceWidgetInput,
) -> Result<Option<WorkspaceWidget>> {
    if get_workspace(conn, workspace_id)?.is_none() {
        return Ok(None);
    }
    let now = now_rfc3339();
    match input.id.as_deref() {
        Some(widget_id) => {
            if get_widget(conn, widget_id)?.is_none() {
                return Ok(None);
            }
            let config = input
                .config
                .clone()
                .unwrap_or_else(|| serde_json::json!({}))
                .to_string();
            let hidden = i64::from(input.hidden.unwrap_or(false));
            conn.execute(
                "UPDATE workspace_widgets
                    SET widget_type = ?1, config_json = ?2, hidden = ?3, updated_at = ?4
                  WHERE id = ?5 AND workspace_id = ?6",
                rusqlite::params![
                    input.widget_type.clone(),
                    config,
                    hidden,
                    now,
                    widget_id,
                    workspace_id
                ],
            )
            .map_err(Error::Database)?;
            get_widget(conn, widget_id)
        }
        None => {
            let id = new_id("wgt");
            let config = input
                .config
                .clone()
                .unwrap_or_else(|| serde_json::json!({}))
                .to_string();
            let hidden = i64::from(input.hidden.unwrap_or(false));
            let position = {
                let max_pos: Option<i64> = conn
                    .query_row(
                        "SELECT MAX(position) FROM workspace_widgets WHERE workspace_id = ?1",
                        [workspace_id],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(Error::Database)?;
                max_pos.map(|p| p + 1).unwrap_or(0)
            };
            conn.execute(
                "INSERT INTO workspace_widgets
                    (id, workspace_id, widget_type, config_json, hidden, position, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
                rusqlite::params![
                    id,
                    workspace_id,
                    input.widget_type.clone(),
                    config,
                    hidden,
                    position,
                    now
                ],
            )
            .map_err(Error::Database)?;
            get_widget(conn, &id)
        }
    }
}

pub fn remove_widget(conn: &Connection, workspace_id: &str, widget_id: &str) -> Result<bool> {
    let changed = conn
        .execute(
            "DELETE FROM workspace_widgets WHERE id = ?1 AND workspace_id = ?2",
            rusqlite::params![widget_id, workspace_id],
        )
        .map_err(Error::Database)?;
    Ok(changed > 0)
}

// ──────────────────────────────────────────────
// Layouts
// ──────────────────────────────────────────────

pub fn list_layouts(conn: &Connection, workspace_id: &str) -> Result<Vec<WorkspaceLayout>> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {LAYOUT_COLS} FROM workspace_layouts WHERE workspace_id = ?1 ORDER BY breakpoint ASC"
        ))
        .map_err(Error::Database)?;
    let rows = stmt
        .query_map([workspace_id], row_to_layout)
        .map_err(Error::Database)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Error::Database)
}

/// 按 id 读取单条 layout（保留：Wave2 A-032 snapshot revision 全量读取使用）。
#[allow(dead_code)]
pub fn get_layout(conn: &Connection, id: &str) -> Result<Option<WorkspaceLayout>> {
    conn.query_row(
        &format!("SELECT {LAYOUT_COLS} FROM workspace_layouts WHERE id = ?1"),
        [id],
        row_to_layout,
    )
    .optional()
    .map_err(Error::Database)
}

/// Upsert one breakpoint layout for a workspace (UNIQUE(workspace_id, breakpoint)).
pub fn save_layout(
    conn: &Connection,
    workspace_id: &str,
    breakpoint: &str,
    layout_json: &str,
) -> Result<Option<WorkspaceLayout>> {
    if get_workspace(conn, workspace_id)?.is_none() {
        return Ok(None);
    }
    let now = now_rfc3339();
    conn.execute(
        "INSERT INTO workspace_layouts
            (id, workspace_id, breakpoint, layout_json, is_active, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, 1, ?5, ?5)
         ON CONFLICT(workspace_id, breakpoint) DO UPDATE SET
            layout_json = excluded.layout_json,
            updated_at = excluded.updated_at",
        rusqlite::params![new_id("lay"), workspace_id, breakpoint, layout_json, now],
    )
    .map_err(Error::Database)?;
    conn.query_row(
        &format!("SELECT {LAYOUT_COLS} FROM workspace_layouts WHERE workspace_id = ?1 AND breakpoint = ?2"),
        rusqlite::params![workspace_id, breakpoint],
        row_to_layout,
    )
    .optional()
    .map_err(Error::Database)
}

// ──────────────────────────────────────────────
// View states
// ──────────────────────────────────────────────

pub fn list_view_states(
    conn: &Connection,
    workspace_id: &str,
) -> Result<Vec<WorkspaceViewState>> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {VIEW_STATE_COLS} FROM workspace_view_states WHERE workspace_id = ?1 ORDER BY view_key ASC"
        ))
        .map_err(Error::Database)?;
    let rows = stmt
        .query_map([workspace_id], row_to_view_state)
        .map_err(Error::Database)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Error::Database)
}

pub fn get_view_state(
    conn: &Connection,
    workspace_id: &str,
    view_key: &str,
) -> Result<Option<WorkspaceViewState>> {
    conn.query_row(
        &format!(
            "SELECT {VIEW_STATE_COLS} FROM workspace_view_states WHERE workspace_id = ?1 AND view_key = ?2"
        ),
        rusqlite::params![workspace_id, view_key],
        row_to_view_state,
    )
    .optional()
    .map_err(Error::Database)
}

/// Upsert one keyed view state (UNIQUE(workspace_id, view_key)).
pub fn save_view_state(
    conn: &Connection,
    workspace_id: &str,
    view_key: &str,
    state_json: &str,
) -> Result<Option<WorkspaceViewState>> {
    if get_workspace(conn, workspace_id)?.is_none() {
        return Ok(None);
    }
    let now = now_rfc3339();
    conn.execute(
        "INSERT INTO workspace_view_states
            (id, workspace_id, view_key, state_json, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(workspace_id, view_key) DO UPDATE SET
            state_json = excluded.state_json,
            updated_at = excluded.updated_at",
        rusqlite::params![new_id("vs"), workspace_id, view_key, state_json, now],
    )
    .map_err(Error::Database)?;
    get_view_state(conn, workspace_id, view_key)
}

// ──────────────────────────────────────────────
// Tool profiles
// ──────────────────────────────────────────────

pub fn list_tool_profiles(
    conn: &Connection,
    workspace_id: &str,
) -> Result<Vec<WorkspaceToolProfile>> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {TOOL_PROFILE_COLS} FROM workspace_tool_profiles WHERE workspace_id = ?1 ORDER BY profile_id ASC"
        ))
        .map_err(Error::Database)?;
    let rows = stmt
        .query_map([workspace_id], row_to_tool_profile)
        .map_err(Error::Database)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Error::Database)
}

pub fn get_tool_profile(
    conn: &Connection,
    workspace_id: &str,
    profile_id: &str,
) -> Result<Option<WorkspaceToolProfile>> {
    conn.query_row(
        &format!(
            "SELECT {TOOL_PROFILE_COLS} FROM workspace_tool_profiles WHERE workspace_id = ?1 AND profile_id = ?2"
        ),
        rusqlite::params![workspace_id, profile_id],
        row_to_tool_profile,
    )
    .optional()
    .map_err(Error::Database)
}

/// Bind (or update) a tool profile to a workspace (UNIQUE(workspace_id, profile_id)).
pub fn bind_tool_profile(
    conn: &Connection,
    workspace_id: &str,
    profile_id: &str,
    tool_key: Option<&str>,
    config_json: &str,
) -> Result<Option<WorkspaceToolProfile>> {
    if get_workspace(conn, workspace_id)?.is_none() {
        return Ok(None);
    }
    let now = now_rfc3339();
    conn.execute(
        "INSERT INTO workspace_tool_profiles
            (id, workspace_id, profile_id, tool_key, config_json, enabled, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6, ?6)
         ON CONFLICT(workspace_id, profile_id) DO UPDATE SET
            tool_key = excluded.tool_key,
            config_json = excluded.config_json,
            updated_at = excluded.updated_at",
        rusqlite::params![new_id("tpf"), workspace_id, profile_id, tool_key, config_json, now],
    )
    .map_err(Error::Database)?;
    get_tool_profile(conn, workspace_id, profile_id)
}

pub fn unbind_tool_profile(conn: &Connection, workspace_id: &str, profile_id: &str) -> Result<bool> {
    let changed = conn
        .execute(
            "DELETE FROM workspace_tool_profiles WHERE workspace_id = ?1 AND profile_id = ?2",
            rusqlite::params![workspace_id, profile_id],
        )
        .map_err(Error::Database)?;
    Ok(changed > 0)
}
