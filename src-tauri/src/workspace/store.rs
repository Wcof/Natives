//! Workspace V2 — raw SQL CRUD over the seven v27 workspace tables.
//!
//! Every function takes a `&Connection`; callers (commands) hand in a pooled
//! connection from `AppState.db`. Nothing here is async — Tauri commands run on
//! the main thread and the pool provides short-lived connections.
//!
//! Host-generated ids are opaque (never derived from secrets) and unique
//! enough for local single-user operation: `<prefix>_<monotonic_nanos>_<seq>`.

use super::types::{
    normalize_layout_mode, normalize_theme, WorkspaceContextItem, WorkspaceContextItemInput,
    WorkspaceContextItemPatch, WorkspaceLayout, WorkspaceSummary, WorkspaceToolProfile,
    WorkspaceUpdateRequest, WorkspaceViewState, WorkspaceWidget, WorkspaceWidgetInput,
};
use crate::{Error, Result};
use rusqlite::{Connection, OptionalExtension};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

static ID_COUNTER: AtomicU64 = AtomicU64::new(0);

/// WS-04: maximum workspace name length (characters, after trim).
pub const MAX_WORKSPACE_NAME_CHARS: usize = 80;

pub(crate) fn new_id(prefix: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}_{nanos:x}_{seq:x}")
}

pub(crate) fn now_rfc3339() -> String {
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
        default_layout_mode: row.get(8)?,
        appearance: parse_json(row.get::<_, String>(9)?),
        template_source_id: row.get(10)?,
        template_version: row.get(11)?,
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
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
        config_version: row.get(3)?,
        config: parse_json(row.get::<_, String>(4)?),
        appearance: parse_json(row.get::<_, String>(5)?),
        enabled: row.get::<_, i64>(6)? != 0,
        z_index: row.get(7)?,
        position: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

fn row_to_layout(row: &rusqlite::Row) -> rusqlite::Result<WorkspaceLayout> {
    Ok(WorkspaceLayout {
        id: row.get(0)?,
        workspace_id: row.get(1)?,
        layout_mode: row.get(2)?,
        breakpoint: row.get(3)?,
        layout_version: row.get(4)?,
        layout: parse_json(row.get::<_, String>(5)?),
        is_active: row.get::<_, i64>(6)? != 0,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

fn row_to_view_state(row: &rusqlite::Row) -> rusqlite::Result<WorkspaceViewState> {
    Ok(WorkspaceViewState {
        id: row.get(0)?,
        workspace_id: row.get(1)?,
        view_key: row.get(2)?,
        state_version: row.get(3)?,
        state: parse_json(row.get::<_, String>(4)?),
        updated_at: row.get(5)?,
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

pub(crate) fn validate_appearance(value: &serde_json::Value) -> Result<()> {
    let Some(object) = value.as_object() else {
        return Err(Error::InvalidInput("appearance must be an object".into()));
    };
    if object
        .keys()
        .any(|key| !matches!(key.as_str(), "surfaceVariant" | "header" | "opacity"))
    {
        return Err(Error::InvalidInput(
            "appearance contains unsupported fields".into(),
        ));
    }
    if object.values().any(|value| {
        value
            .as_str()
            .is_some_and(|text| text.contains('#') || text.contains("url(") || text.contains('<'))
    }) {
        return Err(Error::InvalidInput(
            "appearance cannot contain CSS, HTML, or custom colors".into(),
        ));
    }
    Ok(())
}

pub(crate) fn validate_widget_config(value: &serde_json::Value) -> Result<()> {
    fn contains_forbidden(value: &serde_json::Value) -> bool {
        match value {
            serde_json::Value::Object(object) => object.iter().any(|(key, value)| {
                matches!(
                    key.to_ascii_lowercase().as_str(),
                    "secret" | "password" | "token" | "apikey" | "api_key" | "credential"
                ) || contains_forbidden(value)
            }),
            serde_json::Value::Array(values) => values.iter().any(contains_forbidden),
            serde_json::Value::String(text) => {
                text.contains("<script") || text.contains("javascript:")
            }
            _ => false,
        }
    }
    if contains_forbidden(value) {
        return Err(Error::InvalidInput(
            "widget config cannot contain secrets, scripts, or credentials".into(),
        ));
    }
    Ok(())
}

const WORKSPACE_COLS: &str =
    "id, name, kind, icon, description, theme, is_active, position, default_layout_mode, appearance_json, template_source_id, template_version, created_at, updated_at";
const CONTEXT_COLS: &str =
    "id, workspace_id, item_kind, ref_id, title, meta_json, position, created_at";
const WIDGET_COLS: &str =
    "id, workspace_id, widget_type, config_version, config_json, appearance_json, enabled, z_index, position, created_at, updated_at";
const LAYOUT_COLS: &str =
    "id, workspace_id, layout_mode, breakpoint, layout_version, layout_json, is_active, created_at, updated_at";
const VIEW_STATE_COLS: &str = "id, workspace_id, view_key, state_version, state_json, updated_at";
const TOOL_PROFILE_COLS: &str =
    "id, workspace_id, profile_id, tool_key, config_json, enabled, created_at, updated_at";

// ──────────────────────────────────────────────
// Workspaces
// ──────────────────────────────────────────────

pub fn list_workspaces(conn: &Connection) -> Result<Vec<WorkspaceSummary>> {
    // Deterministic order: position first, then creation time, then id as a
    // tiebreak (created_at is second-resolution, so equal positions within the
    // same second are disambiguated by the nanosecond-bearing id).
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {WORKSPACE_COLS} FROM workspaces WHERE deleted_at IS NULL ORDER BY position ASC, created_at ASC, id ASC"
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
        &format!("SELECT {WORKSPACE_COLS} FROM workspaces WHERE id = ?1 AND deleted_at IS NULL"),
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
    default_layout_mode: &str,
) -> Result<WorkspaceSummary> {
    let id = new_id("ws");
    let now = now_rfc3339();
    let theme = normalize_theme(theme).to_string();
    let position = {
        // MAX() on an empty table is a single NULL row — bind as Option<i64>.
        let max_pos: Option<i64> = conn
            .query_row("SELECT MAX(position) FROM workspaces", [], |r| {
                r.get::<_, Option<i64>>(0)
            })
            .map_err(Error::Database)?;
        max_pos.map(|p| p + 1).unwrap_or(0)
    };
    conn.execute(
        "INSERT INTO workspaces
            (id, name, kind, icon, description, theme, is_active, position, default_layout_mode, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7, ?8, ?9, ?9)",
        rusqlite::params![id, name, kind, icon, description, theme, position, normalize_layout_mode(default_layout_mode), now],
    )
    .map_err(Error::Database)?;
    get_workspace(conn, &id)?
        .ok_or_else(|| Error::Internal(format!("workspace {id} was created but not readable")))
}

pub fn update_workspace(
    conn: &Connection,
    id: &str,
    patch: &WorkspaceUpdateRequest,
) -> Result<Option<WorkspaceSummary>> {
    let Some(existing) = get_workspace(conn, id)? else {
        return Ok(None);
    };
    // WS-04: workspace name validation when patched — trim, non-empty, ≤80 chars.
    // Empty/whitespace-only and over-length names are rejected before any write.
    // Duplicate names remain allowed (revision/conflict handling is the caller's
    // responsibility via check_revision).
    let owned_name: Option<String> = patch.name.as_deref().map(str::trim).map(str::to_string);
    if let Some(ref trimmed) = owned_name {
        if trimmed.is_empty() {
            return Err(Error::InvalidInput("workspace name cannot be empty".into()));
        }
        if trimmed.chars().count() > MAX_WORKSPACE_NAME_CHARS {
            return Err(Error::InvalidInput(format!(
                "workspace name must be {MAX_WORKSPACE_NAME_CHARS} characters or fewer"
            )));
        }
    }
    let name = owned_name.as_deref().unwrap_or(&existing.name);
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
    let layout_mode = patch
        .default_layout_mode
        .as_deref()
        .map(normalize_layout_mode)
        .unwrap_or(&existing.default_layout_mode);
    let appearance = patch.appearance.as_ref().unwrap_or(&existing.appearance);
    validate_appearance(appearance)?;
    let now = now_rfc3339();
    conn.execute(
        "UPDATE workspaces
            SET name = ?1, icon = ?2, description = ?3, theme = ?4, position = ?5,
                default_layout_mode = ?6, appearance_json = ?7, updated_at = ?8
          WHERE id = ?9 AND deleted_at IS NULL",
        rusqlite::params![
            name,
            icon,
            description,
            theme,
            position,
            layout_mode,
            appearance.to_string(),
            now,
            id
        ],
    )
    .map_err(Error::Database)?;
    get_workspace(conn, id)
}

pub fn delete_workspace(conn: &Connection, id: &str) -> Result<bool> {
    let now = now_rfc3339();
    let changed = conn
        .execute(
            "UPDATE workspaces SET deleted_at = ?1, is_active = 0, updated_at = ?1 WHERE id = ?2 AND deleted_at IS NULL",
            rusqlite::params![now, id],
        )
        .map_err(Error::Database)?;
    Ok(changed > 0)
}

/// Set `id` as the single active (on-screen) workspace, deactivating all
/// others. Runs in one transaction so the active flag never has two owners.
///
/// PWSV2 session semantics: if the workspace also has an OPEN session tab
/// (`workspace_open_tabs` row), its `last_active_at` is persisted in the same
/// transaction so the session read model reflects the activation. A
/// non-existent workspace returns `Ok(None)` (the caller surfaces
/// not-found); a present workspace always yields a summary.
pub fn set_active_workspace(conn: &Connection, id: &str) -> Result<Option<WorkspaceSummary>> {
    if get_workspace(conn, id)?.is_none() {
        return Ok(None);
    }
    let now = now_rfc3339();
    let tx = conn.unchecked_transaction().map_err(Error::Database)?;
    tx.execute(
        "UPDATE workspaces SET is_active = 0, updated_at = ?1",
        [&now],
    )
    .map_err(Error::Database)?;
    tx.execute(
        "UPDATE workspaces SET is_active = 1, updated_at = ?1 WHERE id = ?2 AND deleted_at IS NULL",
        rusqlite::params![&now, id],
    )
    .map_err(Error::Database)?;
    // Persist the open session tab's activity (no-op when not open).
    tx.execute(
        "UPDATE workspace_open_tabs SET last_active_at = ?1 WHERE workspace_id = ?2",
        rusqlite::params![&now, id],
    )
    .map_err(Error::Database)?;
    tx.commit().map_err(Error::Database)?;
    get_workspace(conn, id)
}

#[allow(unused_imports)]
pub use super::store_tabs::{
    activate_workspace_session, close_workspace_tab, list_open_tabs, open_workspace_tab,
    reorder_workspace_tabs,
};

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
    const KINDS: [&str; 9] = [
        "file",
        "folder",
        "url",
        "app",
        "document",
        "project",
        "conversation",
        "tool",
        "resource",
    ];
    if !KINDS.contains(&input.item_kind.as_str()) {
        return Err(Error::InvalidInput(format!(
            "unsupported workspace context kind: {}",
            input.item_kind
        )));
    }
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
                |r| r.get::<_, Option<i64>>(0),
            )
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

/// A-032: reorder context items by writing `position = idx` for each id.
/// Missing ids are ignored (they may belong to another workspace). Runs in one
/// transaction so a large drag/reorder commit is atomic.
pub fn reorder_context_items(
    conn: &Connection,
    workspace_id: &str,
    ordered_ids: &[String],
) -> Result<Vec<WorkspaceContextItem>> {
    if get_workspace(conn, workspace_id)?.is_none() {
        return Ok(Vec::new());
    }
    let tx = conn.unchecked_transaction().map_err(Error::Database)?;
    for (idx, item_id) in ordered_ids.iter().enumerate() {
        tx.execute(
            "UPDATE workspace_context_items SET position = ?1
              WHERE id = ?2 AND workspace_id = ?3",
            rusqlite::params![idx as i64, item_id, workspace_id],
        )
        .map_err(Error::Database)?;
    }
    tx.commit().map_err(Error::Database)?;
    list_context_items(conn, workspace_id)
}

/// A-032: apply a batch of context-item patches to one workspace in a single
/// transaction. Only present patch fields are written; rows not present in the
/// batch (or not belonging to the workspace) are left untouched.
pub fn batch_update_context_items(
    conn: &Connection,
    workspace_id: &str,
    patches: &[WorkspaceContextItemPatch],
) -> Result<Vec<WorkspaceContextItem>> {
    if get_workspace(conn, workspace_id)?.is_none() {
        return Ok(Vec::new());
    }
    if patches.is_empty() {
        return list_context_items(conn, workspace_id);
    }
    let existing = list_context_items(conn, workspace_id)?;
    let by_id: HashMap<&str, &WorkspaceContextItem> = existing
        .iter()
        .map(|item| (item.id.as_str(), item))
        .collect();
    let tx = conn.unchecked_transaction().map_err(Error::Database)?;
    for patch in patches {
        let Some(item) = by_id.get(patch.id.as_str()) else {
            continue;
        };
        let title = patch.title.clone().unwrap_or_else(|| item.title.clone());
        let meta = patch
            .meta
            .clone()
            .unwrap_or_else(|| item.meta.clone())
            .to_string();
        let position = patch.position.unwrap_or(item.position);
        tx.execute(
            "UPDATE workspace_context_items
                SET title = ?1, meta_json = ?2, position = ?3
              WHERE id = ?4 AND workspace_id = ?5",
            rusqlite::params![title, meta, position, patch.id, workspace_id],
        )
        .map_err(Error::Database)?;
    }
    tx.commit().map_err(Error::Database)?;
    list_context_items(conn, workspace_id)
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
    let appearance_value = input
        .appearance
        .clone()
        .unwrap_or_else(|| serde_json::json!({}));
    validate_appearance(&appearance_value)?;
    if let Some(config) = input.config.as_ref() {
        validate_widget_config(config)?;
    }
    match input.id.as_deref() {
        Some(widget_id) => {
            let Some(existing) = get_widget(conn, widget_id)? else {
                return Ok(None);
            };
            let config = input.config.clone().unwrap_or(existing.config).to_string();
            let appearance = input
                .appearance
                .clone()
                .unwrap_or(existing.appearance)
                .to_string();
            conn.execute(
                "UPDATE workspace_widgets
                    SET widget_type = ?1, config_version = ?2, config_json = ?3,
                        appearance_json = ?4, enabled = ?5, z_index = ?6, updated_at = ?7
                  WHERE id = ?8 AND workspace_id = ?9",
                rusqlite::params![
                    input.widget_type.clone(),
                    input.config_version.unwrap_or(existing.config_version),
                    config,
                    appearance,
                    i64::from(input.enabled.unwrap_or(existing.enabled)),
                    input.z_index.unwrap_or(existing.z_index),
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
            let appearance = appearance_value.to_string();
            let position = {
                let max_pos: Option<i64> = conn
                    .query_row(
                        "SELECT MAX(position) FROM workspace_widgets WHERE workspace_id = ?1",
                        [workspace_id],
                        |r| r.get::<_, Option<i64>>(0),
                    )
                    .map_err(Error::Database)?;
                max_pos.map(|p| p + 1).unwrap_or(0)
            };
            conn.execute(
                "INSERT INTO workspace_widgets
                    (id, workspace_id, widget_type, config_version, config_json, appearance_json,
                     enabled, z_index, position, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
                rusqlite::params![
                    id,
                    workspace_id,
                    input.widget_type.clone(),
                    input.config_version.unwrap_or(1),
                    config,
                    appearance,
                    i64::from(input.enabled.unwrap_or(true)),
                    input.z_index.unwrap_or(0),
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
    let tx = conn.unchecked_transaction().map_err(Error::Database)?;
    let changed = tx
        .execute(
            "DELETE FROM workspace_widgets WHERE id = ?1 AND workspace_id = ?2",
            rusqlite::params![widget_id, workspace_id],
        )
        .map_err(Error::Database)?;
    if changed == 0 {
        return Ok(false);
    }

    let now = now_rfc3339();
    let mut stmt = tx
        .prepare(
            "SELECT id, layout_mode, layout_json FROM workspace_layouts WHERE workspace_id = ?1",
        )
        .map_err(Error::Database)?;
    let rows = stmt
        .query_map([workspace_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(Error::Database)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Error::Database)?;
    drop(stmt);

    for (layout_row_id, layout_mode, layout_json_str) in rows {
        let mut modified = false;
        if let Ok(mut parsed) = serde_json::from_str::<serde_json::Value>(&layout_json_str) {
            if layout_mode == "structured" {
                if let Some(items) = parsed.as_array_mut() {
                    let prev_len = items.len();
                    items.retain(|item| {
                        item.get("i")
                            .and_then(serde_json::Value::as_str)
                            .map(|i| i != widget_id)
                            .unwrap_or(true)
                    });
                    if items.len() != prev_len {
                        modified = true;
                    }
                }
            } else if layout_mode == "free" {
                if let Some(nodes) = parsed
                    .get_mut("nodes")
                    .and_then(serde_json::Value::as_array_mut)
                {
                    let prev_len = nodes.len();
                    nodes.retain(|node| {
                        node.get("id")
                            .and_then(serde_json::Value::as_str)
                            .map(|id| id != widget_id)
                            .unwrap_or(true)
                    });
                    if nodes.len() != prev_len {
                        modified = true;
                    }
                } else if let Some(items) = parsed.as_array_mut() {
                    let prev_len = items.len();
                    items.retain(|node| {
                        node.get("id")
                            .and_then(serde_json::Value::as_str)
                            .map(|id| id != widget_id)
                            .unwrap_or(true)
                    });
                    if items.len() != prev_len {
                        modified = true;
                    }
                }
            }

            if modified {
                tx.execute(
                    "UPDATE workspace_layouts SET layout_json = ?1, updated_at = ?2 WHERE id = ?3",
                    rusqlite::params![parsed.to_string(), now, layout_row_id],
                )
                .map_err(Error::Database)?;
            }
        }
    }

    tx.commit().map_err(Error::Database)?;
    Ok(true)
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
    layout_mode: &str,
    breakpoint: &str,
    layout_version: i64,
    layout_json: &str,
) -> Result<Option<WorkspaceLayout>> {
    if get_workspace(conn, workspace_id)?.is_none() {
        return Ok(None);
    }
    let now = now_rfc3339();
    let layout_mode = normalize_layout_mode(layout_mode);
    let breakpoint = if layout_mode == "free" {
        "free"
    } else {
        breakpoint
    };
    conn.execute(
        "INSERT INTO workspace_layouts
            (id, workspace_id, layout_mode, breakpoint, layout_version, layout_json, is_active, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, ?7)
         ON CONFLICT(workspace_id, breakpoint) DO UPDATE SET
            layout_mode = excluded.layout_mode,
            layout_version = excluded.layout_version,
            layout_json = excluded.layout_json,
            updated_at = excluded.updated_at",
        rusqlite::params![new_id("lay"), workspace_id, layout_mode, breakpoint, layout_version, layout_json, now],
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

pub fn list_view_states(conn: &Connection, workspace_id: &str) -> Result<Vec<WorkspaceViewState>> {
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
    state_version: i64,
    state_json: &str,
) -> Result<Option<WorkspaceViewState>> {
    if get_workspace(conn, workspace_id)?.is_none() {
        return Ok(None);
    }
    let now = now_rfc3339();
    conn.execute(
        "INSERT INTO workspace_view_states
            (id, workspace_id, view_key, state_version, state_json, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(workspace_id, view_key) DO UPDATE SET
            state_version = excluded.state_version,
            state_json = excluded.state_json,
            updated_at = excluded.updated_at",
        rusqlite::params![
            new_id("vs"),
            workspace_id,
            view_key,
            state_version,
            state_json,
            now
        ],
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
        rusqlite::params![
            new_id("tpf"),
            workspace_id,
            profile_id,
            tool_key,
            config_json,
            now
        ],
    )
    .map_err(Error::Database)?;
    get_tool_profile(conn, workspace_id, profile_id)
}

pub fn unbind_tool_profile(
    conn: &Connection,
    workspace_id: &str,
    profile_id: &str,
) -> Result<bool> {
    let changed = conn
        .execute(
            "DELETE FROM workspace_tool_profiles WHERE workspace_id = ?1 AND profile_id = ?2",
            rusqlite::params![workspace_id, profile_id],
        )
        .map_err(Error::Database)?;
    Ok(changed > 0)
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod store_tests;
