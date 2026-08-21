//! Workspace V2 — Tauri command surface (typed IPC).
//!
//! Every command talks to the SQLite v27 workspace tables through
//! `crate::workspace::store` / `crate::workspace::snapshot`. The renderer never
//! opens the DB directly.
//!
//! Mutations emit a `db-state-changed` event with channel `workspace` so
//! renderer stores (`src/lib/workspace/*`) can invalidate/refresh.

use crate::workspace::{
    self, WorkspaceContextItem, WorkspaceContextItemInput, WorkspaceCreateRequest, WorkspaceLayout,
    WorkspaceSessionSnapshot, WorkspaceSnapshot, WorkspaceSummary, WorkspaceTab, WorkspaceTabInput,
    WorkspaceTabUpdate, WorkspaceToolProfile, WorkspaceUpdateRequest, WorkspaceViewState,
    WorkspaceWidget, WorkspaceWidgetInput,
};
use crate::{emit_db_state_changed, Error, Result};
use r2d2::PooledConnection;
use r2d2_sqlite::SqliteConnectionManager;
use tauri::{AppHandle, State};

use crate::AppState;

type PoolConn = PooledConnection<SqliteConnectionManager>;

fn conn_from(state: &State<'_, AppState>) -> Result<PoolConn> {
    state
        .db
        .get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))
}

/// Borrow a pooled connection as `&rusqlite::Connection` for a single call.
fn with_conn<T>(state: &State<'_, AppState>, f: impl FnOnce(&rusqlite::Connection) -> Result<T>) -> Result<T> {
    let pool_conn = conn_from(state)?;
    let conn: &rusqlite::Connection = &pool_conn;
    f(conn)
}

fn emit_workspace(app: &AppHandle, workspace_id: &str, event: &str) {
    emit_db_state_changed(
        app,
        "workspace",
        serde_json::json!({
            "workspaceId": workspace_id,
            "event": event,
        }),
    );
}

// ──────────────────────────────────────────────
// Workspaces
// ──────────────────────────────────────────────

#[tauri::command]
pub fn workspace_list(state: State<'_, AppState>) -> Result<Vec<WorkspaceSummary>> {
    with_conn(&state, workspace::list_workspaces)
}

#[tauri::command]
pub fn workspace_get(
    workspace_id: String,
    state: State<'_, AppState>,
) -> Result<Option<WorkspaceSnapshot>> {
    with_conn(&state, |conn| workspace::load_workspace_snapshot(conn, &workspace_id))
}

#[tauri::command]
pub fn workspace_create(
    req: WorkspaceCreateRequest,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<WorkspaceSnapshot> {
    let kind = req.kind.as_deref().unwrap_or("workspace");
    let summary = with_conn(&state, |conn| {
        workspace::create_workspace(
            conn,
            &req.name,
            kind,
            req.icon.as_deref(),
            req.description.as_deref(),
            req.theme.as_deref().unwrap_or("dark"),
        )
    })?;
    emit_workspace(&app, &summary.id, "created");
    with_conn(&state, |conn| {
        workspace::load_workspace_snapshot(conn, &summary.id)
    })?
    .ok_or_else(|| Error::Internal("created workspace snapshot missing".into()))
}

#[tauri::command]
pub fn workspace_update(
    workspace_id: String,
    patch: WorkspaceUpdateRequest,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<WorkspaceSnapshot>> {
    let updated = with_conn(&state, |conn| {
        workspace::update_workspace(conn, &workspace_id, &patch)
    })?;
    if updated.is_none() {
        return Ok(None);
    }
    emit_workspace(&app, &workspace_id, "updated");
    with_conn(&state, |conn| workspace::load_workspace_snapshot(conn, &workspace_id))
}

#[tauri::command]
pub fn workspace_delete(
    workspace_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<bool> {
    let removed = with_conn(&state, |conn| workspace::delete_workspace(conn, &workspace_id))?;
    if removed {
        emit_workspace(&app, &workspace_id, "deleted");
    }
    Ok(removed)
}

#[tauri::command]
pub fn workspace_set_active(
    workspace_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<WorkspaceSnapshot>> {
    let summary = with_conn(&state, |conn| workspace::set_active_workspace(conn, &workspace_id))?;
    if summary.is_none() {
        return Ok(None);
    }
    emit_workspace(&app, &workspace_id, "activeChanged");
    with_conn(&state, |conn| workspace::load_workspace_snapshot(conn, &workspace_id))
}

#[tauri::command]
pub fn workspace_snapshot(
    workspace_id: String,
    state: State<'_, AppState>,
) -> Result<Option<WorkspaceSnapshot>> {
    with_conn(&state, |conn| workspace::load_workspace_snapshot(conn, &workspace_id))
}

// ──────────────────────────────────────────────
// Tabs
// ──────────────────────────────────────────────

#[tauri::command]
pub fn workspace_tab_create(
    workspace_id: String,
    input: WorkspaceTabInput,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<WorkspaceTab>> {
    let created = with_conn(&state, |conn| workspace::create_tab(conn, &workspace_id, &input))?;
    if created.is_some() {
        emit_workspace(&app, &workspace_id, "tabChanged");
    }
    Ok(created)
}

#[tauri::command]
pub fn workspace_tab_update(
    tab_id: String,
    patch: WorkspaceTabUpdate,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<WorkspaceTab>> {
    let updated = with_conn(&state, |conn| workspace::update_tab(conn, &tab_id, &patch))?;
    if let Some(tab) = updated.as_ref() {
        emit_workspace(&app, &tab.workspace_id, "tabChanged");
    }
    Ok(updated)
}

#[tauri::command]
pub fn workspace_tab_close(
    tab_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<bool> {
    let workspace_id = with_conn(&state, |conn| workspace::get_tab(conn, &tab_id))?
        .map(|t| t.workspace_id)
        .unwrap_or_default();
    let removed = with_conn(&state, |conn| workspace::close_tab(conn, &tab_id))?;
    if removed && !workspace_id.is_empty() {
        emit_workspace(&app, &workspace_id, "tabChanged");
    }
    Ok(removed)
}

#[tauri::command]
pub fn workspace_tab_reorder(
    workspace_id: String,
    ordered_ids: Vec<String>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<WorkspaceTab>> {
    let tabs = with_conn(&state, |conn| {
        workspace::reorder_tabs(conn, &workspace_id, &ordered_ids)
    })?;
    emit_workspace(&app, &workspace_id, "tabChanged");
    Ok(tabs)
}

// ──────────────────────────────────────────────
// Context items
// ──────────────────────────────────────────────

#[tauri::command]
pub fn workspace_context_add(
    workspace_id: String,
    input: WorkspaceContextItemInput,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<WorkspaceContextItem>> {
    let created = with_conn(&state, |conn| {
        workspace::add_context_item(conn, &workspace_id, &input)
    })?;
    if created.is_some() {
        emit_workspace(&app, &workspace_id, "contextChanged");
    }
    Ok(created)
}

#[tauri::command]
pub fn workspace_context_remove(
    workspace_id: String,
    item_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<bool> {
    let removed = with_conn(&state, |conn| {
        workspace::remove_context_item(conn, &workspace_id, &item_id)
    })?;
    if removed {
        emit_workspace(&app, &workspace_id, "contextChanged");
    }
    Ok(removed)
}

// ──────────────────────────────────────────────
// Widgets
// ──────────────────────────────────────────────

#[tauri::command]
pub fn workspace_widget_upsert(
    workspace_id: String,
    input: WorkspaceWidgetInput,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<WorkspaceWidget>> {
    let saved = with_conn(&state, |conn| {
        workspace::upsert_widget(conn, &workspace_id, &input)
    })?;
    if saved.is_some() {
        emit_workspace(&app, &workspace_id, "widgetChanged");
    }
    Ok(saved)
}

#[tauri::command]
pub fn workspace_widget_remove(
    workspace_id: String,
    widget_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<bool> {
    let removed = with_conn(&state, |conn| {
        workspace::remove_widget(conn, &workspace_id, &widget_id)
    })?;
    if removed {
        emit_workspace(&app, &workspace_id, "widgetChanged");
    }
    Ok(removed)
}

// ──────────────────────────────────────────────
// Layouts
// ──────────────────────────────────────────────

#[tauri::command]
pub fn workspace_layout_save(
    workspace_id: String,
    breakpoint: String,
    layout_json: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<WorkspaceLayout>> {
    let saved = with_conn(&state, |conn| {
        workspace::save_layout(conn, &workspace_id, &breakpoint, &layout_json)
    })?;
    if saved.is_some() {
        emit_workspace(&app, &workspace_id, "layoutChanged");
    }
    Ok(saved)
}

// ──────────────────────────────────────────────
// View states
// ──────────────────────────────────────────────

#[tauri::command]
pub fn workspace_view_state_save(
    workspace_id: String,
    view_key: String,
    state_json: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<WorkspaceViewState>> {
    let saved = with_conn(&state, |conn| {
        workspace::save_view_state(conn, &workspace_id, &view_key, &state_json)
    })?;
    if saved.is_some() {
        emit_workspace(&app, &workspace_id, "viewStateChanged");
    }
    Ok(saved)
}

// ──────────────────────────────────────────────
// Tool profiles
// ──────────────────────────────────────────────

#[tauri::command]
pub fn workspace_tool_profile_bind(
    workspace_id: String,
    profile_id: String,
    tool_key: Option<String>,
    config_json: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<WorkspaceToolProfile>> {
    let bound = with_conn(&state, |conn| {
        workspace::bind_tool_profile(
            conn,
            &workspace_id,
            &profile_id,
            tool_key.as_deref(),
            &config_json,
        )
    })?;
    if bound.is_some() {
        emit_workspace(&app, &workspace_id, "toolProfileChanged");
    }
    Ok(bound)
}

#[tauri::command]
pub fn workspace_tool_profile_unbind(
    workspace_id: String,
    profile_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<bool> {
    let removed = with_conn(&state, |conn| {
        workspace::unbind_tool_profile(conn, &workspace_id, &profile_id)
    })?;
    if removed {
        emit_workspace(&app, &workspace_id, "toolProfileChanged");
    }
    Ok(removed)
}

// ──────────────────────────────────────────────
// Sessions
// ──────────────────────────────────────────────

/// Open (activate + assemble) a workspace session. Backend keeps no dedicated
/// session table — the session is the open workspace's live state assembled
/// from the seven tables; "open" also marks the workspace active.
#[tauri::command]
pub fn workspace_session_open(
    workspace_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<WorkspaceSessionSnapshot>> {
    let summary = with_conn(&state, |conn| workspace::set_active_workspace(conn, &workspace_id))?;
    if summary.is_none() {
        return Ok(None);
    }
    emit_workspace(&app, &workspace_id, "sessionOpened");
    with_conn(&state, |conn| workspace::load_session_snapshot(conn, &workspace_id))
}

/// Close the current workspace session (renderer clears its store; the
/// workspace rows are preserved).
#[tauri::command]
pub fn workspace_session_close(app: AppHandle) -> Result<()> {
    emit_db_state_changed(&app, "workspace", serde_json::json!({ "event": "sessionClosed" }));
    Ok(())
}

#[tauri::command]
pub fn workspace_session_snapshot(
    workspace_id: String,
    state: State<'_, AppState>,
) -> Result<Option<WorkspaceSessionSnapshot>> {
    with_conn(&state, |conn| workspace::load_session_snapshot(conn, &workspace_id))
}
