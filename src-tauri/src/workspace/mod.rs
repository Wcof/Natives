//! Workspace V2 — Host-side workspace domain.
//!
//! Single data authority: SQLite v27 (seven tables) + typed IPC. The renderer
//! never touches the DB directly — every read/write flows through
//! `crate::commands::workspace`.
//!
//! Layering:
//! - `types`     — serde DTOs (camelCase, frozen contract shapes).
//! - `store`     — raw SQL CRUD over the seven workspace tables.
//! - `snapshot`  — assembly of `WorkspaceSnapshot` / `WorkspaceSessionSnapshot`.
//!
//! Schema ownership: the seven tables are created by `migrate_v27`
//! (`crate::db::migrations_steps`); the legacy `settings:home_workspace`
//! document is imported there too.

pub mod service;
pub mod snapshot;
pub mod store;
pub mod types;

pub use service::{batch_update_widget_configs, duplicate_workspace};
pub use snapshot::{get_workspace_mcp_exposure, load_session_snapshot, load_workspace_snapshot};
pub use store::{
    add_context_item, batch_update_context_items, bind_tool_profile, close_tab, create_tab,
    create_workspace, delete_workspace, get_tab, list_workspaces, remove_context_item,
    remove_widget, reorder_context_items, reorder_tabs, save_layout, save_view_state,
    set_active_workspace, unbind_tool_profile, update_tab, update_workspace, upsert_widget,
};
pub use types::{
    McpToolProfileExposure, WorkspaceContextItem, WorkspaceContextItemInput,
    WorkspaceContextItemPatch, WorkspaceCreateRequest, WorkspaceLayout, WorkspaceMcpExposure,
    WorkspaceSessionSnapshot, WorkspaceSnapshot, WorkspaceSummary, WorkspaceTab, WorkspaceTabInput,
    WorkspaceTabUpdate, WorkspaceToolProfile, WorkspaceUpdateRequest, WorkspaceViewState,
    WorkspaceWidget, WorkspaceWidgetConfigPatch, WorkspaceWidgetInput,
};
