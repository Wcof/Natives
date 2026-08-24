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

/// Layout mode values (PWSV2, ADR-0021 §PWSV2-1). The historical word
/// `compact` is retired — `structured` is the default 12/8/4 magnetic grid.
pub const LAYOUT_MODE_STRUCTURED: &str = "structured";
pub const LAYOUT_MODE_FREE: &str = "free";
/// Reserved breakpoint value for the Free Canvas layout row (keeps the v27
/// `UNIQUE(workspace_id, breakpoint)` valid across both modes).
#[allow(dead_code)]
pub const FREE_BREAKPOINT: &str = "free";
/// Structured responsive breakpoints (lg/md/sm = 12/8/4 columns).
#[allow(dead_code)]
pub const STRUCTURED_BREAKPOINTS: [&str; 3] = ["lg", "md", "sm"];

/// Normalize a layout-mode string to the two-value PWSV2 contract. Unknown
/// values (including the retired `compact`) fall back to `structured`.
pub fn normalize_layout_mode(mode: &str) -> &'static str {
    match mode {
        "free" => LAYOUT_MODE_FREE,
        _ => LAYOUT_MODE_STRUCTURED,
    }
}

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
    /// PWSV2: `structured` | `free` (default `structured`).
    pub default_layout_mode: String,
    /// PWSV2: workspace-level appearance (V-003 whitelist; no color tokens).
    pub appearance: serde_json::Value,
    /// PWSV2: template provenance (informational only — never auto-applied).
    pub template_source_id: Option<String>,
    pub template_version: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
}

/// PWSV2: a row in `workspace_open_tabs` — an OPEN Workspace *session* tab
/// (row exists ⇔ workspace is open). The active workspace itself is sourced
/// from `workspaces.is_active`, never from this table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceOpenTab {
    pub workspace_id: String,
    pub sort_order: f64,
    pub is_pinned: bool,
    pub opened_at: String,
    pub last_active_at: String,
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
///
/// PWSV2: visibility is `enabled` (the legacy `hidden` column was mapped by
/// migration v29 — `enabled = NOT hidden` — and is no longer read/written by
/// the production path). `appearance` is the V-003 whitelist only
/// (`surfaceVariant` / `header` / `opacity`); color tokens are rejected.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceWidget {
    pub id: String,
    pub workspace_id: String,
    pub widget_type: String,
    /// PWSV2: schema version of `config` (per widget type).
    pub config_version: i64,
    pub config: serde_json::Value,
    /// PWSV2: V-003 whitelisted appearance (instance-level only).
    pub appearance: serde_json::Value,
    pub enabled: bool,
    /// PWSV2: stacking order inside the Free Canvas (structured ignores it).
    pub z_index: i64,
    pub position: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// One responsive layout per (layout_mode, breakpoint). `layout` is the raw
/// react-grid-layout item array for structured modes (lg/md/sm = 12/8/4) and
/// the bounded Free Canvas document for mode `free` (breakpoint `'free'`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceLayout {
    pub id: String,
    pub workspace_id: String,
    /// PWSV2: `structured` | `free`.
    pub layout_mode: String,
    pub breakpoint: String,
    /// PWSV2: layout document version.
    pub layout_version: i64,
    pub layout: serde_json::Value,
    pub is_active: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// Keyed view state for a workspace (split/window/scroll/selection, and the
/// Data View widget's display mode `list|table|board|calendar` per view_key).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceViewState {
    pub id: String,
    pub workspace_id: String,
    pub view_key: String,
    /// PWSV2: state schema version.
    pub state_version: i64,
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

/// Complete read model of ONE workspace: summary + every child collection
/// (PWSV2: content tabs are gone — the workspace's children are context
/// items, widgets, layouts, view states, tool profiles).
///
/// `revision` (A-033) is a content fingerprint of the whole read model
/// (48-bit FNV-1a over the serialized rows), stable for identical content and
/// different for any changed row. It lets the renderer detect a stale snapshot
/// before/after a Host reconcile. It is deliberately masked to 48 bits so it
/// stays a lossless JS `number` (below 2^53). It round-trips as
/// `expectedRevision` on every mutation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSnapshot {
    pub workspace: WorkspaceSummary,
    pub context_items: Vec<WorkspaceContextItem>,
    pub widgets: Vec<WorkspaceWidget>,
    pub layouts: Vec<WorkspaceLayout>,
    pub view_states: Vec<WorkspaceViewState>,
    pub tool_profiles: Vec<WorkspaceToolProfile>,
    pub revision: i64,
}

/// Global lightweight session read model (PWSV2 / M-026): which workspaces
/// are OPEN, which one is on screen, and the workspace metadata — WITHOUT any
/// per-workspace widget/layout payloads. Inactive workspaces never carry
/// child collections in this shape; the renderer fetches
/// `WorkspaceSnapshot` on demand.
///
/// `revision` is a content fingerprint over the open tabs + workspace
/// metadata, so opening/closing/reordering sessions is detectable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSessionSnapshot {
    pub opened_tabs: Vec<WorkspaceOpenTab>,
    pub active_workspace_id: Option<String>,
    pub workspaces: Vec<WorkspaceSummary>,
    pub revision: i64,
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
    /// PWSV2: initial layout mode (`structured` | `free`, default
    /// `structured`).
    #[serde(default)]
    pub default_layout_mode: Option<String>,
    /// PWSV2: builtin template to instantiate on create (e.g.
    /// `classic-personal-dashboard`). Absent/`null` = blank workspace.
    #[serde(default)]
    pub template_id: Option<String>,
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
    /// PWSV2: switch layout mode (`structured` | `free`).
    pub default_layout_mode: Option<String>,
    /// PWSV2: workspace appearance (V-003 whitelist).
    pub appearance: Option<serde_json::Value>,
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

/// A-032: single patch inside `batch_update_context_items`. Only present fields
/// are applied; the host keeps the row's current value for the rest. `meta`
/// replaces the whole payload object when present.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceContextItemPatch {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub meta: Option<serde_json::Value>,
    #[serde(default)]
    pub position: Option<i64>,
}

/// Widget upsert request. When `id` is present the row is updated, otherwise a
/// new widget id is generated by the host.
///
/// PWSV2: visibility is `enabled`; `config` is stored with an explicit
/// `config_version`; `appearance` is V-003-whitelisted (surfaceVariant /
/// header / opacity) and validated host-side — arbitrary hex/CSS is rejected.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceWidgetInput {
    #[serde(default)]
    pub id: Option<String>,
    pub widget_type: String,
    #[serde(default)]
    pub config: Option<serde_json::Value>,
    /// PWSV2: config schema version (defaults to 1).
    #[serde(default)]
    pub config_version: Option<i64>,
    /// PWSV2: V-003 whitelisted appearance (instance-level only).
    #[serde(default)]
    pub appearance: Option<serde_json::Value>,
    #[serde(default)]
    pub enabled: Option<bool>,
    /// PWSV2: Free Canvas stacking order.
    #[serde(default)]
    pub z_index: Option<i64>,
}

/// Single config patch inside `batch_update_widget_configs` (contract
/// `batch_update_widget_configs`). `config` replaces the whole row config;
/// missing widgets are ignored (they may have been removed concurrently).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceWidgetConfigPatch {
    pub id: String,
    pub config: serde_json::Value,
}

// ──────────────────────────────────────────────
// A-034 MCP exposure allowlist (read-only DTO)
// ──────────────────────────────────────────────

/// One enabled tool-profile reference eligible for MCP exposure. Carries only
/// the non-sensitive binding (profile id + optional tool key) — never config
/// secrets, never credential plaintext.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolProfileExposure {
    pub profile_id: String,
    pub tool_key: Option<String>,
    pub enabled: bool,
}

// ──────────────────────────────────────────────
// PWSV2 templates (WorkspaceTemplateManifestV1)
// ──────────────────────────────────────────────

/// PWSV2: a single widget entry inside a template manifest. `key` is the
/// template-scoped identity used for placement + single-widget reset; it is
/// rewritten to a fresh host id at instantiation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateWidgetSpec {
    pub key: String,
    pub widget_type: String,
    pub config_version: i64,
    pub config: serde_json::Value,
    /// V-003 whitelisted appearance (validated at load/save).
    pub appearance: serde_json::Value,
}

/// PWSV2: template manifest document (frozen shape
/// `WorkspaceTemplateManifestV1`). Structured layouts are per-breakpoint grid
/// item arrays keyed by the template widget key; the free layout is the
/// bounded canvas document. Manifests must NOT contain secrets, user absolute
/// paths, historical data, or live metrics — validated host-side.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceTemplateManifestV1 {
    /// Manifest schema version (currently 1).
    pub schema_version: i64,
    /// Human-facing template version (bumped on content edits).
    pub template_version: i64,
    /// i18n key for the display name (builtin templates); personal templates
    /// use `name` directly.
    #[serde(default)]
    pub name_key: Option<String>,
    pub appearance: serde_json::Value,
    pub default_layout_mode: String,
    pub widgets: Vec<TemplateWidgetSpec>,
    /// Structured breakpoint grids (`lg`/`md`/`sm`) and/or the free canvas
    /// document, each item placement keyed by `TemplateWidgetSpec.key`.
    pub layouts: serde_json::Value,
}

/// PWSV2: template read model (builtin metadata + personal rows, unified).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceTemplate {
    pub id: String,
    pub name: String,
    /// `builtin` | `personal`.
    pub origin: String,
    pub schema_version: i64,
    pub template_version: i64,
    /// i18n key for builtin display names (personal: None).
    pub name_key: Option<String>,
    pub preview_key: Option<String>,
    pub manifest: WorkspaceTemplateManifestV1,
    pub created_at: String,
    pub updated_at: String,
}

/// PWSV2: personal-template save request (captured from a workspace).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceTemplateSaveRequest {
    pub name: String,
    /// Personal template to overwrite (absent = create new).
    #[serde(default)]
    pub id: Option<String>,
}

// ──────────────────────────────────────────────
// A-034 MCP exposure allowlist (read-only DTO)
// ──────────────────────────────────────────────

/// A-034: read-only MCP exposure allowlist for a workspace.
///
/// This is the contract the *future* MCP server will consume to know which
/// non-sensitive workspace references may be exposed. It is assembled purely
/// from the workspace domain reads (no direct DB access, no Agent runtime).
/// It intentionally contains only references/ids — never secrets or payload
/// content — so the allowlist itself is safe to expose by construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceMcpExposure {
    pub workspace_id: String,
    /// Distinct `item_kind`s currently pinned, sorted (BTreeSet).
    pub context_item_kinds: Vec<String>,
    /// Distinct widget types currently placed, sorted (BTreeSet).
    pub widget_types: Vec<String>,
    /// Enabled tool-profile bindings (references only).
    pub tool_profiles: Vec<McpToolProfileExposure>,
    /// Distinct layout breakpoints present (`lg`/`md`/`sm`/free), sorted.
    pub layout_breakpoints: Vec<String>,
    /// Wall-clock of the read (informational only, not a revision).
    pub generated_at: String,
}
