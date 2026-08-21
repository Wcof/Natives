'use client';

/**
 * Workspace view registry (C-001..C-031) — maps a view kind to its component
 * and creates new view configs. Keeps the composition page free of per-kind
 * switches.
 */

import type {
  DataViewState,
  GridLayouts,
  WorkspaceViewConfig,
  WorkspaceViewKind,
} from '@/lib/workspace/views/types';

/** Create a blank view config for the "New view" menu. */
export function createWorkspaceViewConfig(kind: WorkspaceViewKind, index: number): WorkspaceViewConfig {
  const id = `view-${kind}-${Date.now().toString(36)}-${index}`;
  const base = { id, title: defaultTitle(kind, index) };
  switch (kind) {
    case 'canvas':
      return { ...base, kind, canvasNodes: [] };
    case 'data':
      return {
        ...base,
        kind,
        data: {
          mode: 'board',
          columns: ['name', 'status', 'assignee', 'due'],
          hiddenColumns: [],
          filters: [],
          sort: null,
          groupBy: 'status',
          calendarField: 'due',
        } satisfies DataViewState,
      };
    case 'grid':
    default:
      return { ...base, kind, gridLayouts: emptyGridLayouts() };
  }
}

function defaultTitle(kind: WorkspaceViewKind, index: number): string {
  switch (kind) {
    case 'canvas':
      return `Canvas ${index + 1}`;
    case 'data':
      return `Data ${index + 1}`;
    default:
      return `Grid ${index + 1}`;
  }
}

export function emptyGridLayouts(): GridLayouts {
  return { lg: [], md: [], sm: [] };
}
