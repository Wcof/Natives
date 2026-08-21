//! Workspace V2 — read-model assembly.
//!
//! `WorkspaceSnapshot` is the complete read model of one workspace; it is what
//! `workspace.get` returns. `WorkspaceSessionSnapshot` is the runtime session
//! read model of the currently open workspace (no dedicated session table — a
//! session is the open workspace's live state, assembled from the same rows).

use super::store;
use super::types::{WorkspaceSessionSnapshot, WorkspaceSnapshot};
use crate::Result;
use rusqlite::Connection;

/// Load the complete snapshot for a workspace, or `None` when the workspace
/// does not exist.
pub fn load_workspace_snapshot(
    conn: &Connection,
    workspace_id: &str,
) -> Result<Option<WorkspaceSnapshot>> {
    let Some(workspace) = store::get_workspace(conn, workspace_id)? else {
        return Ok(None);
    };
    let tabs = store::list_tabs(conn, workspace_id)?;
    let context_items = store::list_context_items(conn, workspace_id)?;
    let widgets = store::list_widgets(conn, workspace_id)?;
    let layouts = store::list_layouts(conn, workspace_id)?;
    let view_states = store::list_view_states(conn, workspace_id)?;
    let tool_profiles = store::list_tool_profiles(conn, workspace_id)?;
    Ok(Some(WorkspaceSnapshot {
        workspace,
        tabs,
        context_items,
        widgets,
        layouts,
        view_states,
        tool_profiles,
    }))
}

/// Load the runtime session snapshot for a workspace, or `None` when the
/// workspace does not exist.
pub fn load_session_snapshot(
    conn: &Connection,
    workspace_id: &str,
) -> Result<Option<WorkspaceSessionSnapshot>> {
    let Some(workspace) = store::get_workspace(conn, workspace_id)? else {
        return Ok(None);
    };
    let tabs = store::list_tabs(conn, workspace_id)?;
    let active_tab_id = tabs.iter().find(|t| t.is_active).map(|t| t.id.clone());
    let context_items = store::list_context_items(conn, workspace_id)?;
    let widgets = store::list_widgets(conn, workspace_id)?;
    let layouts = store::list_layouts(conn, workspace_id)?;
    let view_states = store::list_view_states(conn, workspace_id)?;
    let tool_profiles = store::list_tool_profiles(conn, workspace_id)?;
    Ok(Some(WorkspaceSessionSnapshot {
        workspace_id: workspace.id,
        active_tab_id,
        tabs,
        context_items,
        widgets,
        layouts,
        view_states,
        tool_profiles,
    }))
}
