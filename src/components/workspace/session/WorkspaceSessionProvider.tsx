'use client';

/**
 * Workspace session provider (C-001..C-006).
 *
 * Owns the single snapshot of record for the composition:
 *  - snapshot-first: state is hydrated synchronously from the cache, so the
 *    first paint is never blank.
 *  - inactive tabs carry only metadata + the snapshot cache; their runtime
 *    state is not mounted (views render exclusively for the active tab).
 *  - every mutation goes through the reducer; the provider debounces the
 *    persistence write (never per-pointer).
 */

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useReducer,
  useRef,
} from 'react';
import type {
  WorkspaceSnapshot,
  WorkspaceTab,
  WorkspaceViewConfig,
} from '@/lib/workspace/views/types';
import {
  createWorkspaceSnapshotSaver,
  hydrateWorkspaceSnapshot,
} from './workspacePersistence';

export type WorkspaceAction =
  | { type: 'hydrate'; snapshot: WorkspaceSnapshot }
  | { type: 'activate'; tabId: string | null }
  | { type: 'open-tab'; tab: WorkspaceTab; view: WorkspaceViewConfig }
  | { type: 'close-tab'; tabId: string }
  | { type: 'reopen-tab'; tabId: string }
  | { type: 'pin-tab'; tabId: string; pinned: boolean }
  | { type: 'reorder-tab'; tabId: string; toIndex: number }
  | { type: 'add-view'; view: WorkspaceViewConfig }
  | { type: 'delete-view'; viewId: string }
  | { type: 'update-view'; viewId: string; patch: Partial<WorkspaceViewConfig> }
  | { type: 'set-breakpoint'; breakpoint: 'lg' | 'md' | 'sm' }
  | { type: 'set-active'; isActive: boolean }
  | { type: 'bump-activity' };

export function workspaceReducer(
  state: WorkspaceSnapshot,
  action: WorkspaceAction,
): WorkspaceSnapshot {
  const now = Date.now();
  switch (action.type) {
    case 'hydrate':
      return action.snapshot;
    case 'activate': {
      const tabs = state.tabs;
      const target = tabs.find((tab) => tab.id === action.tabId);
      return {
        ...state,
        session: {
          ...state.session,
          activeTabId: target ? target.id : null,
          lastActiveAt: now,
          activityCount: state.session.activityCount + 1,
        },
      };
    }
    case 'open-tab': {
      if (state.views[action.view.id]) {
        // Already open → just activate it.
        return {
          ...state,
          session: { ...state.session, activeTabId: action.view.id, lastActiveAt: now },
        };
      }
      return {
        ...state,
        tabs: [...state.tabs, action.tab],
        views: { ...state.views, [action.view.id]: action.view },
        session: {
          ...state.session,
          activeTabId: action.view.id,
          lastActiveAt: now,
          activityCount: state.session.activityCount + 1,
        },
      };
    }
    case 'close-tab': {
      // Close != Delete: the view config is retained and the tab moves to the
      // "recently closed" list so it can be reopened.
      const tab = state.tabs.find((item) => item.id === action.tabId);
      if (!tab) return state;
      const closedTabs = [tab, ...state.session.closedTabs.filter((item) => item.id !== tab.id)].slice(0, 8);
      const tabs = state.tabs.filter((item) => item.id !== action.tabId);
      const active = state.session.activeTabId === tab.id ? (tabs[tabs.length - 1]?.id ?? null) : state.session.activeTabId;
      return { ...state, tabs, session: { ...state.session, activeTabId: active, closedTabs } };
    }
    case 'reopen-tab': {
      const closed = state.session.closedTabs.find((item) => item.id === action.tabId);
      const view = state.views[action.tabId];
      if (!closed || !view) return state;
      return {
        ...state,
        tabs: [...state.tabs, { ...closed }],
        session: {
          ...state.session,
          activeTabId: closed.id,
          closedTabs: state.session.closedTabs.filter((item) => item.id !== closed.id),
        },
      };
    }
    case 'pin-tab':
      return {
        ...state,
        tabs: state.tabs.map((tab) =>
          tab.id === action.tabId ? { ...tab, pinned: action.pinned } : tab,
        ),
      };
    case 'reorder-tab': {
      const from = state.tabs.findIndex((tab) => tab.id === action.tabId);
      if (from < 0) return state;
      const to = Math.max(0, Math.min(state.tabs.length - 1, action.toIndex));
      if (from === to) return state;
      const tabs = [...state.tabs];
      const moved = tabs.splice(from, 1)[0];
      if (!moved) return state;
      tabs.splice(to, 0, moved);
      return { ...state, tabs };
    }
    case 'add-view': {
      const view = action.view;
      return {
        ...state,
        tabs: [...state.tabs, { id: view.id, title: view.title, kind: view.kind, icon: actionViewIcon(view) }],
        views: { ...state.views, [view.id]: view },
        session: { ...state.session, activeTabId: view.id, lastActiveAt: now },
      };
    }
    case 'delete-view': {
      // Delete = remove view config + tab entirely (distinct from Close).
      const views = { ...state.views };
      delete views[action.viewId];
      return {
        ...state,
        tabs: state.tabs.filter((tab) => tab.id !== action.viewId),
        views,
        session: {
          ...state.session,
          activeTabId: state.session.activeTabId === action.viewId ? null : state.session.activeTabId,
          closedTabs: state.session.closedTabs.filter((tab) => tab.id !== action.viewId),
        },
      };
    }
    case 'update-view': {
      const current = state.views[action.viewId];
      if (!current) return state;
      return {
        ...state,
        views: {
          ...state.views,
          [action.viewId]: { ...current, ...action.patch },
        },
      };
    }
    case 'set-breakpoint':
      return { ...state, session: { ...state.session, activeBreakpoint: action.breakpoint } };
    case 'set-active':
      return {
        ...state,
        session: { ...state.session, isActive: action.isActive, lastActiveAt: action.isActive ? now : state.session.lastActiveAt },
      };
    case 'bump-activity':
      return { ...state, session: { ...state.session, activityCount: state.session.activityCount + 1 } };
    default:
      return state;
  }
}

function actionViewIcon(view: WorkspaceViewConfig): string {
  switch (view.kind) {
    case 'canvas':
      return 'layers';
    case 'data':
      return 'table';
    default:
      return 'layout-grid';
  }
}

export interface WorkspaceSessionApi {
  activate: (tabId: string) => void;
  closeTab: (tabId: string) => void;
  reopenTab: (tabId: string) => void;
  pinTab: (tabId: string, pinned: boolean) => void;
  reorderTab: (tabId: string, toIndex: number) => void;
  addView: (view: WorkspaceViewConfig) => void;
  deleteView: (viewId: string) => void;
  updateView: (viewId: string, patch: Partial<WorkspaceViewConfig>) => void;
  setBreakpoint: (breakpoint: 'lg' | 'md' | 'sm') => void;
  setActive: (isActive: boolean) => void;
  flush: () => void;
}

interface WorkspaceSessionContextValue {
  snapshot: WorkspaceSnapshot;
  dispatch: React.Dispatch<WorkspaceAction>;
  api: WorkspaceSessionApi;
}

const WorkspaceSessionContext = createContext<WorkspaceSessionContextValue | null>(null);

export function WorkspaceSessionProvider({ children }: { children: React.ReactNode }) {
  // Synchronous hydration → the tree paints immediately (snapshot-first).
  const [snapshot, dispatch] = useReducer(
    workspaceReducer,
    undefined,
    hydrateWorkspaceSnapshot,
  );
  const saverRef = useRef(createWorkspaceSnapshotSaver());
  const snapshotRef = useRef(snapshot);
  snapshotRef.current = snapshot;

  // Debounced persistence — never per-pointer.
  useEffect(() => {
    saverRef.current.schedule(snapshot);
  }, [snapshot]);

  useEffect(() => {
    const flush = () => saverRef.current.flush();
    window.addEventListener('beforeunload', flush);
    return () => {
      window.removeEventListener('beforeunload', flush);
      saverRef.current.flush();
    };
  }, []);

  const api = useMemo<WorkspaceSessionApi>(
    () => ({
      activate: (tabId) => dispatch({ type: 'activate', tabId }),
      closeTab: (tabId) => dispatch({ type: 'close-tab', tabId }),
      reopenTab: (tabId) => dispatch({ type: 'reopen-tab', tabId }),
      pinTab: (tabId, pinned) => dispatch({ type: 'pin-tab', tabId, pinned }),
      reorderTab: (tabId, toIndex) => dispatch({ type: 'reorder-tab', tabId, toIndex }),
      addView: (view) => dispatch({ type: 'add-view', view }),
      deleteView: (viewId) => dispatch({ type: 'delete-view', viewId }),
      updateView: (viewId, patch) => dispatch({ type: 'update-view', viewId, patch }),
      setBreakpoint: (breakpoint) => dispatch({ type: 'set-breakpoint', breakpoint }),
      setActive: (isActive) => dispatch({ type: 'set-active', isActive }),
      flush: () => saverRef.current.flush(),
    }),
    [],
  );

  const value = useMemo(
    () => ({ snapshot, dispatch, api }),
    [snapshot, api],
  );

  return (
    <WorkspaceSessionContext.Provider value={value}>
      {children}
    </WorkspaceSessionContext.Provider>
  );
}

export function useWorkspaceSession(): WorkspaceSessionContextValue {
  const ctx = useContext(WorkspaceSessionContext);
  if (!ctx) throw new Error('useWorkspaceSession must be used inside WorkspaceSessionProvider');
  return ctx;
}

/** True when a view config id is the active tab (convenience selector). */
export function useActiveViewId(): string | null {
  const { snapshot } = useWorkspaceSession();
  return snapshot.session.activeTabId;
}
