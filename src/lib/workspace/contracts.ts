'use client';

/**
 * Workspace V2 — frozen TS contract.
 *
 * Mirror of the Rust serde DTOs in `src-tauri/src/workspace/types.rs`
 * (`#[serde(rename_all = "camelCase")]`). Field names are part of the contract;
 * do not rename without a contract change.
 *
 * Data authority: SQLite v27 via typed IPC only. The renderer never talks to
 * the DB directly.
 */

export type WorkspaceKind = 'home' | 'workspace';
export type WorkspaceTheme = 'dark' | 'light';
export type WorkspaceBreakpoint = 'lg' | 'md' | 'sm' | 'free';
export type WorkspaceLayoutMode = 'structured' | 'free';

/** Workspace root entity (summary shape, embedded in snapshots). */
export interface WorkspaceSummary {
  id: string;
  name: string;
  kind: WorkspaceKind;
  icon: string | null;
  description: string | null;
  /** Normalized: 'dark' | 'light' (V-002). */
  theme: WorkspaceTheme;
  isActive: boolean;
  position: number;
  defaultLayoutMode: WorkspaceLayoutMode;
  appearance: Record<string, unknown>;
  templateSourceId: string | null;
  templateVersion: number | null;
  createdAt: string;
  updatedAt: string;
}

/** A context item pinned to a workspace (file / folder / url / app / document). */
export interface WorkspaceContextItem {
  id: string;
  workspaceId: string;
  itemKind: string;
  refId: string;
  title: string;
  meta: Record<string, unknown>;
  position: number;
  createdAt: string;
}

/** A widget instance (legacy Home widgets migrated here). */
export interface WorkspaceWidget {
  id: string;
  workspaceId: string;
  widgetType: string;
  configVersion: number;
  config: Record<string, unknown>;
  appearance: Record<string, unknown>;
  enabled: boolean;
  zIndex: number;
  position: number;
  createdAt: string;
  updatedAt: string;
}

/** Single config patch inside `batchUpdateWidgetConfigs`. */
export interface WorkspaceWidgetConfigPatch {
  id: string;
  config: Record<string, unknown>;
}

/** One responsive layout per breakpoint; `layout` is the react-grid-layout item array. */
export interface WorkspaceLayout {
  id: string;
  workspaceId: string;
  layoutMode: WorkspaceLayoutMode;
  breakpoint: WorkspaceBreakpoint;
  layoutVersion: number;
  layout: unknown;
  isActive: boolean;
  createdAt: string;
  updatedAt: string;
}

/** Keyed view state for a workspace (split / window / scroll / selection). */
export interface WorkspaceViewState {
  id: string;
  workspaceId: string;
  viewKey: string;
  stateVersion: number;
  state: Record<string, unknown>;
  updatedAt: string;
}

/** A tool-profile binding scoped to a workspace. */
export interface WorkspaceToolProfile {
  id: string;
  workspaceId: string;
  profileId: string;
  toolKey: string | null;
  config: Record<string, unknown>;
  enabled: boolean;
  createdAt: string;
  updatedAt: string;
}

/** Complete read model of one workspace. */
export interface WorkspaceSnapshot {
  workspace: WorkspaceSummary;
  contextItems: WorkspaceContextItem[];
  widgets: WorkspaceWidget[];
  layouts: WorkspaceLayout[];
  viewStates: WorkspaceViewState[];
  toolProfiles: WorkspaceToolProfile[];
  /** A-033: 48-bit content fingerprint; pass back as `expectedRevision` (G-008). */
  revision: number;
}

/**
 * Runtime session read model of the currently open workspace. Backend has no
 * dedicated session table — a session is the open workspace's live state.
 */
export interface WorkspaceOpenTab {
  workspaceId: string;
  sortOrder: number;
  isPinned: boolean;
  openedAt: string;
  lastActiveAt: string;
}

export interface WorkspaceSessionSnapshot {
  openedTabs: WorkspaceOpenTab[];
  activeWorkspaceId: string | null;
  workspaces: WorkspaceSummary[];
  /** A-033: 48-bit content fingerprint; pass back as `expectedRevision` (G-008). */
  revision: number;
}

// ──────────────────────────────────────────────
// Command inputs (typed IPC)
// ──────────────────────────────────────────────

/**
 * G-008: every workspace content-mutation command also accepts an optional
 * `expectedRevision: number` wire arg (the A-033 snapshot `revision` the
 * client last saw). `undefined`/omitted = no check (backward-compatible); a
 * stale value makes the command fail with `Conflict` before writing.
 * Commands: update / delete / tab create·update·close·reorder / context
 * add·remove·batch update·reorder / widget upsert·remove·batch update /
 * layout save / view state save / tool profile bind·unbind.
 */
export type ExpectedRevision = number;

export interface WorkspaceCreateInput {
  name: string;
  kind?: WorkspaceKind;
  icon?: string | null;
  description?: string | null;
  theme?: WorkspaceTheme;
  defaultLayoutMode?: WorkspaceLayoutMode;
  templateId?: string | null;
}

/** Workspace patch; empty string clears the nullable column. */
export interface WorkspaceUpdateInput {
  name?: string;
  icon?: string;
  description?: string;
  theme?: WorkspaceTheme;
  position?: number;
  defaultLayoutMode?: WorkspaceLayoutMode;
  appearance?: Record<string, unknown>;
}

export interface WorkspaceContextItemInput {
  itemKind: string;
  refId: string;
  title?: string;
  meta?: Record<string, unknown>;
}

/** Widget upsert; when `id` is present the row is updated, otherwise created. */
export interface WorkspaceWidgetInput {
  id?: string;
  widgetType: string;
  config?: Record<string, unknown>;
  configVersion?: number;
  appearance?: Record<string, unknown>;
  enabled?: boolean;
  zIndex?: number;
}

/** A-032: single patch inside batchUpdateContextItems. */
export interface WorkspaceContextItemPatch {
  id: string;
  title?: string;
  meta?: Record<string, unknown>;
  position?: number;
}

/** One enabled tool-profile reference eligible for MCP exposure (A-034). */
export interface McpToolProfileExposure {
  profileId: string;
  toolKey: string | null;
  enabled: boolean;
}

/** A-034: read-only MCP exposure allowlist for a workspace. */
export interface WorkspaceMcpExposure {
  workspaceId: string;
  contextItemKinds: string[];
  widgetTypes: string[];
  toolProfiles: McpToolProfileExposure[];
  layoutBreakpoints: string[];
  generatedAt: string;
}

export interface TemplateWidgetSpec {
  key: string;
  widgetType: string;
  configVersion: number;
  config: Record<string, unknown>;
  appearance: Record<string, unknown>;
}

export interface WorkspaceTemplateManifestV1 {
  schemaVersion: 1;
  templateVersion: number;
  nameKey: string | null;
  appearance: Record<string, unknown>;
  defaultLayoutMode: WorkspaceLayoutMode;
  widgets: TemplateWidgetSpec[];
  layouts: Record<string, unknown>;
}

export interface WorkspaceTemplate {
  id: string;
  name: string;
  origin: 'builtin' | 'personal';
  schemaVersion: number;
  templateVersion: number;
  nameKey: string | null;
  previewKey: string | null;
  manifest: WorkspaceTemplateManifestV1;
  createdAt: string;
  updatedAt: string;
}
