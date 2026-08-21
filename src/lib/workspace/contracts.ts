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
export type WorkspaceBreakpoint = 'lg' | 'md' | 'sm';

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
  createdAt: string;
  updatedAt: string;
}

/** A tab inside a workspace. `refId` is an opaque host reference — never a secret. */
export interface WorkspaceTab {
  id: string;
  workspaceId: string;
  tabType: string;
  title: string;
  refId: string | null;
  url: string | null;
  position: number;
  isActive: boolean;
  pinned: boolean;
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
  config: Record<string, unknown>;
  hidden: boolean;
  position: number;
  createdAt: string;
  updatedAt: string;
}

/** One responsive layout per breakpoint; `layout` is the react-grid-layout item array. */
export interface WorkspaceLayout {
  id: string;
  workspaceId: string;
  breakpoint: WorkspaceBreakpoint;
  layout: unknown[];
  isActive: boolean;
  createdAt: string;
  updatedAt: string;
}

/** Keyed view state for a workspace (split / window / scroll / selection). */
export interface WorkspaceViewState {
  id: string;
  workspaceId: string;
  viewKey: string;
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
  tabs: WorkspaceTab[];
  contextItems: WorkspaceContextItem[];
  widgets: WorkspaceWidget[];
  layouts: WorkspaceLayout[];
  viewStates: WorkspaceViewState[];
  toolProfiles: WorkspaceToolProfile[];
}

/**
 * Runtime session read model of the currently open workspace. Backend has no
 * dedicated session table — a session is the open workspace's live state.
 */
export interface WorkspaceSessionSnapshot {
  workspaceId: string;
  activeTabId: string | null;
  tabs: WorkspaceTab[];
  contextItems: WorkspaceContextItem[];
  widgets: WorkspaceWidget[];
  layouts: WorkspaceLayout[];
  viewStates: WorkspaceViewState[];
  toolProfiles: WorkspaceToolProfile[];
}

// ──────────────────────────────────────────────
// Command inputs (typed IPC)
// ──────────────────────────────────────────────

export interface WorkspaceCreateInput {
  name: string;
  kind?: WorkspaceKind;
  icon?: string | null;
  description?: string | null;
  theme?: WorkspaceTheme;
}

/** Workspace patch; empty string clears the nullable column. */
export interface WorkspaceUpdateInput {
  name?: string;
  icon?: string;
  description?: string;
  theme?: WorkspaceTheme;
  position?: number;
}

export interface WorkspaceTabInput {
  tabType: string;
  title?: string;
  refId?: string | null;
  url?: string | null;
}

/** Tab patch; empty string clears the nullable column. */
export interface WorkspaceTabUpdateInput {
  title?: string;
  refId?: string;
  url?: string;
  isActive?: boolean;
  pinned?: boolean;
  position?: number;
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
  hidden?: boolean;
}
