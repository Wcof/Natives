/**
 * V2 Workspace — shared view/session TS contracts (Wave1 · C-001..C-031).
 *
 * These shapes mirror the frozen workspace-v2-contract §3/§4/§5 snapshot
 * contracts (WorkspaceSnapshot / WorkspaceSessionSnapshot) so the composition
 * can be snapshot-first and never depend on async-loading to paint.
 *
 * NOTE (contract assumption): the canonical shared module is owned by the
 * platform/data track (agent B/A); when it lands, re-export these aliases from
 * there instead of re-declaring — see docs/development/handoff-c.md.
 */

export type WorkspaceViewKind = 'grid' | 'canvas' | 'data';

/** A single item in a responsive grid layout (react-grid-layout compatible). */
export interface GridLayoutItem {
  i: string;
  x: number;
  y: number;
  w: number;
  h: number;
  minW?: number;
  minH?: number;
  maxW?: number;
  maxH?: number;
  isBounded?: boolean;
}

/** Responsive layouts keyed by breakpoint (lg/md/sm). */
export type GridLayouts = Record<'lg' | 'md' | 'sm', GridLayoutItem[]>;

/** Layout breakpoints for the CompactGrid (12/8/4 columns). */
export const GRID_BREAKPOINTS = { lg: 1200, md: 996, sm: 768 };
export const GRID_COLUMNS: Record<'lg' | 'md' | 'sm', number> = { lg: 12, md: 8, sm: 4 };
export const GRID_ROW_HEIGHT = 32;
export const GRID_MARGIN: [number, number] = [12, 12];
export const GRID_PADDING: [number, number] = [16, 16];

/** Data view controlled state (persisted per view id). */
export interface DataFilter {
  field: string;
  op: 'eq' | 'neq' | 'contains' | 'gt' | 'lt';
  value: string | number | boolean;
}

export interface DataViewState {
  mode: 'list' | 'table' | 'board' | 'calendar';
  columns: string[];
  hiddenColumns: string[];
  sort?: { field: string; dir: 'asc' | 'desc' } | null;
  filters: DataFilter[];
  groupBy?: string | null;
  calendarField?: string;
}

/** Per-view configuration carried inside the snapshot (survives close/reopen). */
export interface WorkspaceViewConfig {
  id: string;
  kind: WorkspaceViewKind;
  title: string;
  /** Grid layout only used when kind === 'grid'. */
  gridLayouts?: GridLayouts;
  /** Canvas nodes only used when kind === 'canvas'. */
  canvasNodes?: unknown[];
  /** Data view state only used when kind === 'data'. */
  data?: DataViewState;
  /** Module-owned opaque config; preserved verbatim. */
  config?: Record<string, unknown>;
}

/** Tab entry in the tab strip. Close != Delete: the view config is retained. */
export interface WorkspaceTab {
  id: string;
  title: string;
  kind: WorkspaceViewKind;
  pinned?: boolean;
  icon?: string;
}

/** Ephemeral runtime session facts (not persisted / cache only). */
export interface WorkspaceSessionSnapshot {
  activeTabId: string | null;
  activeBreakpoint: 'lg' | 'md' | 'sm';
  /** True while the workspace surface is the focused app surface. */
  isActive: boolean;
  lastActiveAt: number;
  activityCount: number;
  /** Recently closed tabs (Close != Delete → reopen). */
  closedTabs: WorkspaceTab[];
}

/** Persisted, versioned workspace document (snapshot-first source of truth). */
export interface WorkspaceSnapshot {
  version: number;
  id: string;
  name: string;
  tabs: WorkspaceTab[];
  views: Record<string, WorkspaceViewConfig>;
  session: WorkspaceSessionSnapshot;
  updatedAt: number;
}

export const WORKSPACE_SNAPSHOT_VERSION = 1;
export const WORKSPACE_STORAGE_KEY = 'natives.workspace.v2.snapshot';

/** Semantic token names the workspace may consume (frozen §7 subset). */
export const WORKSPACE_TOKENS = {
  surface: 'var(--surface)',
  surfaceHover: 'var(--surface-hover)',
  border: 'var(--border)',
  borderSubtle: 'var(--border-subtle)',
  primary: 'var(--primary)',
  primaryForeground: 'var(--primary-foreground)',
  primarySoft: 'var(--primary-soft)',
  text: 'var(--text)',
  textSecondary: 'var(--text-secondary)',
  textDisabled: 'var(--text-disabled)',
  danger: 'var(--danger)',
} as const;

/** Create a pristine default snapshot synchronously (no blank screen). */
export function createDefaultWorkspaceSnapshot(): WorkspaceSnapshot {
  const now = Date.now();
  const gridId = 'view-grid-home';
  const canvasId = 'view-canvas-board';
  const dataId = 'view-data-board';

  const grid: WorkspaceViewConfig = {
    id: gridId,
    kind: 'grid',
    title: 'Start Here',
    gridLayouts: {
      lg: [
        { i: 'note-welcome', x: 0, y: 0, w: 6, h: 4, minW: 3, minH: 3, isBounded: true },
        { i: 'todo-focus', x: 6, y: 0, w: 6, h: 4, minW: 3, minH: 3, isBounded: true },
        { i: 'note-snippets', x: 0, y: 4, w: 4, h: 4, minW: 2, minH: 3, isBounded: true },
        { i: 'stats-quick', x: 4, y: 4, w: 8, h: 4, minW: 3, minH: 3, isBounded: true },
      ],
      md: [
        { i: 'note-welcome', x: 0, y: 0, w: 4, h: 4, minW: 3, minH: 3, isBounded: true },
        { i: 'todo-focus', x: 4, y: 0, w: 4, h: 4, minW: 3, minH: 3, isBounded: true },
        { i: 'note-snippets', x: 0, y: 4, w: 4, h: 4, minW: 2, minH: 3, isBounded: true },
        { i: 'stats-quick', x: 0, y: 8, w: 8, h: 4, minW: 3, minH: 3, isBounded: true },
      ],
      sm: [
        { i: 'note-welcome', x: 0, y: 0, w: 4, h: 4, minW: 3, minH: 3, isBounded: true },
        { i: 'todo-focus', x: 0, y: 4, w: 4, h: 4, minW: 3, minH: 3, isBounded: true },
        { i: 'note-snippets', x: 0, y: 8, w: 4, h: 4, minW: 2, minH: 3, isBounded: true },
        { i: 'stats-quick', x: 0, y: 12, w: 4, h: 4, minW: 3, minH: 3, isBounded: true },
      ],
    },
  };

  const canvas: WorkspaceViewConfig = {
    id: canvasId,
    kind: 'canvas',
    title: 'Idea Board',
    canvasNodes: [],
  };

  const data: WorkspaceViewConfig = {
    id: dataId,
    kind: 'data',
    title: 'Tasks',
    data: {
      mode: 'board',
      columns: ['name', 'status', 'assignee', 'due'],
      hiddenColumns: [],
      filters: [],
      sort: null,
      groupBy: 'status',
      calendarField: 'due',
    },
  };

  const views: Record<string, WorkspaceViewConfig> = { [gridId]: grid, [canvasId]: canvas, [dataId]: data };

  return {
    version: WORKSPACE_SNAPSHOT_VERSION,
    id: 'workspace-default',
    name: 'Workspace',
    tabs: [
      { id: gridId, title: grid.title, kind: 'grid', pinned: true, icon: 'layout-grid' },
      { id: canvasId, title: canvas.title, kind: 'canvas', pinned: false, icon: 'layers' },
      { id: dataId, title: data.title, kind: 'data', pinned: false, icon: 'table' },
    ],
    views,
    session: {
      activeTabId: gridId,
      activeBreakpoint: 'lg',
      isActive: false,
      lastActiveAt: now,
      activityCount: 0,
      closedTabs: [],
    },
    updatedAt: now,
  };
}
