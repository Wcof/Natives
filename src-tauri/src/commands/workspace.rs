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
    WorkspaceSessionSnapshot, WorkspaceSnapshot, WorkspaceSummary, WorkspaceTemplate,
    WorkspaceTemplateSaveRequest, WorkspaceToolProfile, WorkspaceUpdateRequest, WorkspaceViewState,
    WorkspaceWidget, WorkspaceWidgetConfigPatch, WorkspaceWidgetInput,
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
fn with_conn<T>(
    state: &State<'_, AppState>,
    f: impl FnOnce(&rusqlite::Connection) -> Result<T>,
) -> Result<T> {
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
    with_conn(&state, |conn| {
        workspace::load_workspace_snapshot(conn, &workspace_id)
    })
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
            req.default_layout_mode.as_deref().unwrap_or("structured"),
        )
    })?;
    let template_id = req
        .template_id
        .as_deref()
        .unwrap_or(workspace::templates::classic_template_id());
    with_conn(&state, |conn| {
        workspace::templates::apply_template(conn, &summary.id, template_id)
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
    expected_revision: Option<i64>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<WorkspaceSnapshot>> {
    with_conn(&state, |conn| {
        crate::workspace::service::check_revision(conn, &workspace_id, expected_revision)
    })?;
    let updated = with_conn(&state, |conn| {
        workspace::update_workspace(conn, &workspace_id, &patch)
    })?;
    if updated.is_none() {
        return Ok(None);
    }
    emit_workspace(&app, &workspace_id, "updated");
    with_conn(&state, |conn| {
        workspace::load_workspace_snapshot(conn, &workspace_id)
    })
}

#[tauri::command]
pub fn workspace_delete(
    workspace_id: String,
    expected_revision: Option<i64>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<bool> {
    with_conn(&state, |conn| {
        crate::workspace::service::check_revision(conn, &workspace_id, expected_revision)
    })?;
    let removed = with_conn(&state, |conn| {
        workspace::delete_workspace(conn, &workspace_id)
    })?;
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
    let summary = with_conn(&state, |conn| {
        workspace::set_active_workspace(conn, &workspace_id)
    })?;
    if summary.is_none() {
        return Ok(None);
    }
    emit_workspace(&app, &workspace_id, "activeChanged");
    with_conn(&state, |conn| {
        workspace::load_workspace_snapshot(conn, &workspace_id)
    })
}

/// A-022: duplicate a workspace (copy workspace + tabs/context/widgets/
/// layouts/view states/tool profiles under fresh ids, one transaction).
#[tauri::command]
pub fn workspace_duplicate(
    workspace_id: String,
    name: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<WorkspaceSummary>> {
    let duplicated = with_conn(&state, |conn| {
        workspace::duplicate_workspace(conn, &workspace_id, &name)
    })?;
    if duplicated.is_some() {
        emit_workspace(&app, &workspace_id, "created");
    }
    Ok(duplicated)
}

#[tauri::command]
pub fn workspace_snapshot(
    workspace_id: String,
    state: State<'_, AppState>,
) -> Result<Option<WorkspaceSnapshot>> {
    with_conn(&state, |conn| {
        workspace::load_workspace_snapshot(conn, &workspace_id)
    })
}

// ──────────────────────────────────────────────
// Context items
// ──────────────────────────────────────────────

#[tauri::command]
pub fn workspace_context_add(
    workspace_id: String,
    input: WorkspaceContextItemInput,
    expected_revision: Option<i64>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<WorkspaceContextItem>> {
    with_conn(&state, |conn| {
        crate::workspace::service::check_revision(conn, &workspace_id, expected_revision)
    })?;
    let created = with_conn(&state, |conn| {
        workspace::add_context_item(conn, &workspace_id, &input)
    })?;
    if created.is_some() {
        emit_workspace(&app, &workspace_id, "contextChanged");
    }
    Ok(created)
}

#[tauri::command]
pub fn workspace_context_batch_update(
    workspace_id: String,
    patches: Vec<workspace::WorkspaceContextItemPatch>,
    expected_revision: Option<i64>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<WorkspaceContextItem>> {
    with_conn(&state, |conn| {
        crate::workspace::service::check_revision(conn, &workspace_id, expected_revision)
    })?;
    let updated = with_conn(&state, |conn| {
        workspace::batch_update_context_items(conn, &workspace_id, &patches)
    })?;
    emit_workspace(&app, &workspace_id, "contextChanged");
    Ok(updated)
}

#[tauri::command]
pub fn workspace_context_reorder(
    workspace_id: String,
    ordered_ids: Vec<String>,
    expected_revision: Option<i64>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<WorkspaceContextItem>> {
    with_conn(&state, |conn| {
        crate::workspace::service::check_revision(conn, &workspace_id, expected_revision)
    })?;
    let updated = with_conn(&state, |conn| {
        workspace::reorder_context_items(conn, &workspace_id, &ordered_ids)
    })?;
    emit_workspace(&app, &workspace_id, "contextChanged");
    Ok(updated)
}

#[tauri::command]
pub fn workspace_mcp_exposure(
    workspace_id: String,
    state: State<'_, AppState>,
) -> Result<Option<workspace::WorkspaceMcpExposure>> {
    with_conn(&state, |conn| {
        workspace::get_workspace_mcp_exposure(conn, &workspace_id)
    })
}

#[tauri::command]
pub fn workspace_context_remove(
    workspace_id: String,
    item_id: String,
    expected_revision: Option<i64>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<bool> {
    with_conn(&state, |conn| {
        crate::workspace::service::check_revision(conn, &workspace_id, expected_revision)
    })?;
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
    expected_revision: Option<i64>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<WorkspaceWidget>> {
    with_conn(&state, |conn| {
        crate::workspace::service::check_revision(conn, &workspace_id, expected_revision)
    })?;
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
    expected_revision: Option<i64>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<bool> {
    with_conn(&state, |conn| {
        crate::workspace::service::check_revision(conn, &workspace_id, expected_revision)
    })?;
    let removed = with_conn(&state, |conn| {
        workspace::remove_widget(conn, &workspace_id, &widget_id)
    })?;
    if removed {
        emit_workspace(&app, &workspace_id, "widgetChanged");
    }
    Ok(removed)
}

/// Contract `batch_update_widget_configs`: apply config patches to multiple
/// widgets in one transaction (A-015 transactional batch updates).
#[tauri::command]
pub fn workspace_widget_batch_update(
    workspace_id: String,
    updates: Vec<WorkspaceWidgetConfigPatch>,
    expected_revision: Option<i64>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<usize> {
    with_conn(&state, |conn| {
        crate::workspace::service::check_revision(conn, &workspace_id, expected_revision)
    })?;
    let changed = with_conn(&state, |conn| {
        workspace::batch_update_widget_configs(conn, &workspace_id, &updates)
    })?;
    if changed > 0 {
        emit_workspace(&app, &workspace_id, "widgetChanged");
    }
    Ok(changed)
}

// ──────────────────────────────────────────────
// Layouts
// ──────────────────────────────────────────────

#[tauri::command]
pub fn workspace_layout_save(
    workspace_id: String,
    layout_mode: String,
    breakpoint: String,
    layout_version: Option<i64>,
    layout_json: String,
    expected_revision: Option<i64>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<WorkspaceLayout>> {
    with_conn(&state, |conn| {
        crate::workspace::service::check_revision(conn, &workspace_id, expected_revision)
    })?;
    let saved = with_conn(&state, |conn| {
        workspace::save_layout(
            conn,
            &workspace_id,
            &layout_mode,
            &breakpoint,
            layout_version.unwrap_or(1),
            &layout_json,
        )
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
    state_version: Option<i64>,
    state_json: String,
    expected_revision: Option<i64>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<WorkspaceViewState>> {
    with_conn(&state, |conn| {
        crate::workspace::service::check_revision(conn, &workspace_id, expected_revision)
    })?;
    let saved = with_conn(&state, |conn| {
        workspace::save_view_state(
            conn,
            &workspace_id,
            &view_key,
            state_version.unwrap_or(1),
            &state_json,
        )
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
    expected_revision: Option<i64>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<WorkspaceToolProfile>> {
    with_conn(&state, |conn| {
        crate::workspace::service::check_revision(conn, &workspace_id, expected_revision)
    })?;
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
    expected_revision: Option<i64>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<bool> {
    with_conn(&state, |conn| {
        crate::workspace::service::check_revision(conn, &workspace_id, expected_revision)
    })?;
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

/// Open a Workspace session tab and make it active.
#[tauri::command]
pub fn workspace_session_open(
    workspace_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<WorkspaceSessionSnapshot>> {
    let opened = with_conn(&state, |conn| {
        workspace::open_workspace_tab(conn, &workspace_id)?;
        workspace::set_active_workspace(conn, &workspace_id)
    })?;
    if opened.is_none() {
        return Ok(None);
    }
    emit_workspace(&app, &workspace_id, "sessionOpened");
    with_conn(&state, workspace::load_session_snapshot).map(Some)
}

#[tauri::command]
pub fn workspace_session_close(
    workspace_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<WorkspaceSessionSnapshot> {
    with_conn(&state, |conn| {
        workspace::close_workspace_tab(conn, &workspace_id)
    })?;
    emit_workspace(&app, &workspace_id, "sessionClosed");
    with_conn(&state, workspace::load_session_snapshot)
}

#[tauri::command]
pub fn workspace_session_snapshot(state: State<'_, AppState>) -> Result<WorkspaceSessionSnapshot> {
    with_conn(&state, workspace::load_session_snapshot)
}

#[tauri::command]
pub fn workspace_session_reorder(
    ordered_workspace_ids: Vec<String>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<WorkspaceSessionSnapshot> {
    with_conn(&state, |conn| {
        workspace::reorder_workspace_tabs(conn, &ordered_workspace_ids)
    })?;
    emit_db_state_changed(
        &app,
        "workspace",
        serde_json::json!({"event":"sessionReordered"}),
    );
    with_conn(&state, workspace::load_session_snapshot)
}

#[tauri::command]
pub fn workspace_template_list(state: State<'_, AppState>) -> Result<Vec<WorkspaceTemplate>> {
    with_conn(&state, workspace::templates::list_templates)
}

#[tauri::command]
pub fn workspace_template_save(
    workspace_id: String,
    req: WorkspaceTemplateSaveRequest,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<WorkspaceTemplate>> {
    let saved = with_conn(&state, |conn| {
        workspace::templates::capture_workspace(conn, &workspace_id, &req)
    })?;
    if saved.is_some() {
        emit_workspace(&app, &workspace_id, "templateSaved");
    }
    Ok(saved)
}

#[tauri::command]
pub fn workspace_template_delete(
    template_id: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<bool> {
    let deleted = with_conn(&state, |conn| {
        workspace::templates::delete_personal_template(conn, &template_id)
    })?;
    if deleted {
        emit_db_state_changed(
            &app,
            "workspace",
            serde_json::json!({"event":"templateDeleted"}),
        );
    }
    Ok(deleted)
}

#[tauri::command]
pub fn workspace_restore_template(
    workspace_id: String,
    template_id: String,
    expected_revision: Option<i64>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<WorkspaceSnapshot>> {
    with_conn(&state, |conn| {
        workspace::service::check_revision(conn, &workspace_id, expected_revision)
    })?;
    if !with_conn(&state, |conn| {
        workspace::templates::apply_template(conn, &workspace_id, &template_id)
    })? {
        return Ok(None);
    }
    emit_workspace(&app, &workspace_id, "templateRestored");
    with_conn(&state, |conn| {
        workspace::load_workspace_snapshot(conn, &workspace_id)
    })
}

#[tauri::command]
pub fn workspace_widget_reset(
    workspace_id: String,
    widget_id: String,
    template_id: Option<String>,
    expected_revision: Option<i64>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<WorkspaceSnapshot>> {
    with_conn(&state, |conn| {
        workspace::service::check_revision(conn, &workspace_id, expected_revision)
    })?;
    if !with_conn(&state, |conn| {
        workspace::templates::reset_widget(conn, &workspace_id, &widget_id, template_id.as_deref())
    })? {
        return Ok(None);
    }
    emit_workspace(&app, &workspace_id, "widgetReset");
    with_conn(&state, |conn| {
        workspace::load_workspace_snapshot(conn, &workspace_id)
    })
}
