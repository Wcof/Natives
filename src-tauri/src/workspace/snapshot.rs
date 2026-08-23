//! Workspace V2 — read-model assembly.
//!
//! `WorkspaceSnapshot` is the complete read model of one workspace; it is what
//! `workspace.get` returns. `WorkspaceSessionSnapshot` is the runtime session
//! read model of the currently open workspace (no dedicated session table — a
//! session is the open workspace's live state, assembled from the same rows).
//!
//! Both snapshots carry a `revision` (A-033): a 48-bit content fingerprint
//! (FNV-1a over the serialized rows). It is stable for identical content and
//! changes when any row content changes (including reorders, which only touch
//! `position`), so the renderer can detect a stale snapshot.

use super::store;
use super::types::{
    WorkspaceContextItem, WorkspaceLayout, WorkspaceSessionSnapshot, WorkspaceSnapshot,
    WorkspaceSummary, WorkspaceTab, WorkspaceToolProfile, WorkspaceViewState, WorkspaceWidget,
};
use crate::Result;
use rusqlite::Connection;
use serde::Serialize;

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
    let revision = compute_revision(
        &workspace,
        &tabs,
        &context_items,
        &widgets,
        &layouts,
        &view_states,
        &tool_profiles,
    );
    Ok(Some(WorkspaceSnapshot {
        workspace,
        tabs,
        context_items,
        widgets,
        layouts,
        view_states,
        tool_profiles,
        revision,
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
    let revision = compute_revision(
        &workspace,
        &tabs,
        &context_items,
        &widgets,
        &layouts,
        &view_states,
        &tool_profiles,
    );
    Ok(Some(WorkspaceSessionSnapshot {
        workspace_id: workspace.id,
        active_tab_id,
        tabs,
        context_items,
        widgets,
        layouts,
        view_states,
        tool_profiles,
        revision,
    }))
}

// ──────────────────────────────────────────────
// Revision fingerprint (A-033)
// ──────────────────────────────────────────────

/// FNV-1a 64-bit offset basis.
const FNV1A_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64-bit prime.
const FNV1A_PRIME: u64 = 0x0000_0100_0000_01b3;

fn hash_bytes(state: &mut u64, bytes: &[u8]) {
    for &b in bytes {
        *state ^= u64::from(b);
        *state = (*state).wrapping_mul(FNV1A_PRIME);
    }
}

fn hash_component(state: &mut u64, value: &impl Serialize) {
    // serde_json emits struct fields in declaration order and object maps in
    // sorted-key order unless `preserve_order` is enabled; either way the bytes
    // are deterministic for identical DB content (DB read order is stable).
    let bytes = serde_json::to_string(value).unwrap_or_default();
    hash_bytes(state, bytes.as_bytes());
}

/// Compute the snapshot revision as a 48-bit content fingerprint. The mask
/// keeps the value below 2^53 so it round-trips losslessly as a JS `number`.
/// It is intentionally *not* a monotonic counter — it is an equality version
/// for stale-snapshot detection.
fn compute_revision(
    workspace: &WorkspaceSummary,
    tabs: &[WorkspaceTab],
    context_items: &[WorkspaceContextItem],
    widgets: &[WorkspaceWidget],
    layouts: &[WorkspaceLayout],
    view_states: &[WorkspaceViewState],
    tool_profiles: &[WorkspaceToolProfile],
) -> i64 {
    let mut state = FNV1A_OFFSET;
    hash_component(&mut state, workspace);
    for tab in tabs {
        hash_component(&mut state, tab);
    }
    for item in context_items {
        hash_component(&mut state, item);
    }
    for widget in widgets {
        hash_component(&mut state, widget);
    }
    for layout in layouts {
        hash_component(&mut state, layout);
    }
    for view in view_states {
        hash_component(&mut state, view);
    }
    for profile in tool_profiles {
        hash_component(&mut state, profile);
    }
    (state & 0x0000_FFFF_FFFF_FFFF) as i64
}

/// Load the read-only MCP exposure allowlist for a workspace (A-034).
pub fn get_workspace_mcp_exposure(
    conn: &Connection,
    workspace_id: &str,
) -> Result<Option<crate::workspace::WorkspaceMcpExposure>> {
    let Some(workspace) = store::get_workspace(conn, workspace_id)? else {
        return Ok(None);
    };
    let context_items = store::list_context_items(conn, workspace_id)?;
    let widgets = store::list_widgets(conn, workspace_id)?;
    let layouts = store::list_layouts(conn, workspace_id)?;
    let tool_profiles = store::list_tool_profiles(conn, workspace_id)?;

    use std::collections::BTreeSet;
    let context_item_kinds: Vec<String> = context_items
        .into_iter()
        .map(|i| i.item_kind)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let widget_types: Vec<String> = widgets
        .into_iter()
        .map(|w| w.widget_type)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let layout_breakpoints: Vec<String> = layouts
        .into_iter()
        .map(|l| l.breakpoint)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let exposed_profiles = tool_profiles
        .into_iter()
        .filter(|p| p.enabled)
        .map(|p| crate::workspace::McpToolProfileExposure {
            profile_id: p.profile_id,
            tool_key: p.tool_key,
            enabled: p.enabled,
        })
        .collect();

    Ok(Some(crate::workspace::WorkspaceMcpExposure {
        workspace_id: workspace.id,
        context_item_kinds,
        widget_types,
        tool_profiles: exposed_profiles,
        layout_breakpoints,
        generated_at: chrono::Utc::now().to_rfc3339(),
    }))
}
