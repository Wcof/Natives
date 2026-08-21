//! Workspace V2 — serde DTOs (frozen contract shapes).
//!
//! Field naming is part of the V2 contract: every struct uses camelCase over
//! IPC (`#[serde(rename_all = "camelCase")]`). Do not rename a field without a
//! contract change — the mirror TS types live in `src/lib/workspace/contracts.ts`.

use serde::{Deserialize, Serialize};

/// Canonical theme values (V-002). Legacy aliases (`terminal-volt`,
/// `frosted-jasmine`) are normalized at the boundary (migration + commands).
pub const THEME_DARK: &str = "dark";
pub const THEME_LIGHT: &str = "light";

/// Normalize a theme string to the two-value contract. Unknown values fall
/// back to `dark` rather than persisting noise.
pub fn normalize_theme(theme: &str) -> &'static str {
    match theme {
        "light" | "frosted-jasmine" => THEME_LIGHT,
        _ => THEME_DARK,
    }
}

/// Workspace root entity (summary shape, also embedded in snapshots).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSummary {
    pub id: String,
    pub name: String,
    /// `home` (legacy Home import) | `workspace`.
    pub kind: String,
    pub icon: Option<String>,
    pub description: Option<String>,
    /// Normalized: `dark` | `light`.
    pub theme: String,
    pub is_active: bool,
    pub position: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// A tab inside a workspace. `ref_id` is an opaque reference to the host
/// surface (runtime instance / app / tool) — never a secret.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceTab {
    pub id: String,
    pub workspace_id: String,
    pub tab_type: String,
    pub title: String,
    pub ref_id: Option<String>,
    pub url: Option<String>,
    pub position: i64,
    pub is_active: bool,
    pub pinned: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// A context item pinned to a workspace (file / folder / url / app / document).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceContextItem {
    pub id: String,
    pub workspace_id: String,
    pub item_kind: String,
    pub ref_id: String,
    pub title: String,
    pub meta: serde_json::Value,
    pub position: i64,
    pub created_at: String,
}

/// A widget instance (legacy Home widgets migrated here).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceWidget {
    pub id: String,
    pub workspace_id: String,
    pub widget_type: String,
    pub config: serde_json::Value,
    pub hidden: bool,
    pub position: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// One responsive layout per breakpoint (lg/md/sm). `layout` is the raw
/// react-grid-layout item array.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceLayout {
    pub id: String,
    pub workspace_id: String,
    pub breakpoint: String,
    pub layout: serde_json::Value,
    pub is_active: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// Keyed view state for a workspace (split/window/scroll/selection).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceViewState {
    pub id: String,
    pub workspace_id: String,
    pub view_key: String,
    pub state: serde_json::Value,
    pub updated_at: String,
}

/// A tool-profile binding scoped to a workspace.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceToolProfile {
    pub id: String,
    pub workspace_id: String,
    pub profile_id: String,
    pub tool_key: Option<String>,
    pub config: serde_json::Value,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// Complete read model of one workspace: summary + every child collection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSnapshot {
    pub workspace: WorkspaceSummary,
    pub tabs: Vec<WorkspaceTab>,
    pub context_items: Vec<WorkspaceContextItem>,
    pub widgets: Vec<WorkspaceWidget>,
    pub layouts: Vec<WorkspaceLayout>,
    pub view_states: Vec<WorkspaceViewState>,
    pub tool_profiles: Vec<WorkspaceToolProfile>,
}

/// Runtime session read model for the currently open workspace. Backend has no
/// dedicated session table — a session is the open workspace's live state, so
/// this is assembled from the same seven tables and consumed by the renderer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSessionSnapshot {
    pub workspace_id: String,
    pub active_tab_id: Option<String>,
    pub tabs: Vec<WorkspaceTab>,
    pub context_items: Vec<WorkspaceContextItem>,
    pub widgets: Vec<WorkspaceWidget>,
    pub layouts: Vec<WorkspaceLayout>,
    pub view_states: Vec<WorkspaceViewState>,
    pub tool_profiles: Vec<WorkspaceToolProfile>,
}

// ──────────────────────────────────────────────
// Command inputs (deserialized from typed IPC)
// ──────────────────────────────────────────────

/// Workspace create request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceCreateRequest {
    pub name: String,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub theme: Option<String>,
}

/// Workspace patch (only present fields are applied; empty string clears the
/// nullable column).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WorkspaceUpdateRequest {
    pub name: Option<String>,
    pub icon: Option<String>,
    pub description: Option<String>,
    pub theme: Option<String>,
    pub position: Option<i64>,
}

/// Tab create request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceTabInput {
    pub tab_type: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub ref_id: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
}

/// Tab patch (only present fields are applied; empty string clears nullable).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WorkspaceTabUpdate {
    pub title: Option<String>,
    pub ref_id: Option<String>,
    pub url: Option<String>,
    pub is_active: Option<bool>,
    pub pinned: Option<bool>,
    pub position: Option<i64>,
}

/// Context item create request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceContextItemInput {
    pub item_kind: String,
    pub ref_id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub meta: Option<serde_json::Value>,
}

/// Widget upsert request. When `id` is present the row is updated, otherwise a
/// new widget id is generated by the host.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceWidgetInput {
    #[serde(default)]
    pub id: Option<String>,
    pub widget_type: String,
    #[serde(default)]
    pub config: Option<serde_json::Value>,
    #[serde(default)]
    pub hidden: Option<bool>,
}
