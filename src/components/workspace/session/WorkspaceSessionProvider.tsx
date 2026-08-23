'use client';

/**
 * Workspace session provider (C-001..C-006).
 *
 * Owns the single snapshot of record for the composition:
 *  - host is the source of truth: on mount the workspace + session snapshot
 *    is loaded through the typed IPC client (listWorkspaces → getWorkspace /
 *    createWorkspace) and cached in snapshot-store; the localStorage read is
 *    a synchronous warm-up for the first paint only (never authoritative).
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
  useState,
} from 'react';
import type {
  WorkspaceSnapshot,
  WorkspaceTab,
  WorkspaceViewConfig,
} from '@/lib/workspace/views/types';
import type { WorkspaceSnapshot as HostWorkspaceSnapshot } from '@/lib/workspace/contracts';
import {
  createWorkspace,
  getWorkspace,
  listWorkspaces,
  removeWidget,
  saveLayout,
  upsertWidget,
} from '@/lib/workspace/client';
import {
  getSnapshot,
  invalidate,
  setSnapshot,
} from '@/lib/workspace/snapshot-store';
import {
  createDefaultConfig,
  getWidget,
  serializeWidgetConfig,
} from '@/lib/workspace/widgets';
import {
  createWorkspaceSnapshotSaver,
  hydrateWorkspaceSnapshot,
} from './workspacePersistence';
import type { GridLayouts } from '@/lib/workspace/views/types';

export type WorkspaceAction =
  | { type: 'hydrate'; snapshot: WorkspaceSnapshot }
  | { type: 'host-sync'; snapshot: HostWorkspaceSnapshot }
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
    case 'host-sync': {
      // Host is authoritative for the workspace identity (id/name). Content
      // (widgets/layouts/viewStates) lives in the snapshot store, which the
      // grid view reads directly; the UI's tab/view strip is a module-owned
      // surface (its own ids + persisted canvas/data state), so it is kept.
      // Runtime bits (activeBreakpoint, recently-closed list, active tab) stay
      // UI-owned. We do NOT replace state.tabs with host tab rows: those carry
      // host ids with no matching view config, which would render dead tabs.
      return {
        ...state,
        id: action.snapshot.workspace.id,
        name: action.snapshot.workspace.name,
        updatedAt: now,
      };
    }
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
  /**
   * Write surface (Slice 14): create a widget row on the host
   * (client.upsertWidget) and refresh the snapshot from the host.
   * Rejects with a real error (e.g. G-008 `Conflict`) on failure.
   */
  addWidget: (widgetType: string) => Promise<void>;
  /** Remove a widget row on the host (client.removeWidget) + refresh. */
  removeWidget: (widgetId: string) => Promise<void>;
  /**
   * Persist the responsive grid layouts on the host (client.saveLayout,
   * one row per breakpoint) and refresh the snapshot from the host.
   * This is the ONLY layout persistence point — drag/resize pointer moves
   * never write to the host.
   */
  saveLayout: (layouts: GridLayouts, activeBreakpoint?: 'lg' | 'md' | 'sm') => Promise<void>;
}

/** Hydration status of the host read model (Slice 13). */
export type WorkspaceHostStatus = 'pending' | 'ready' | 'error';

interface WorkspaceSessionContextValue {
  snapshot: WorkspaceSnapshot;
  dispatch: React.Dispatch<WorkspaceAction>;
  api: WorkspaceSessionApi;
  /** Resolved host workspace id (null until hydration resolves). */
  workspaceId: string | null;
  /** 'pending' while the host snapshot is loading; 'ready' after it landed. */
  hostStatus: WorkspaceHostStatus;
  /** Real error text when the host fetch failed (browser dev / host down). */
  hostError: string | null;
  /** Re-run the host hydration (used by the error banner). */
  reloadHost: () => void;
}

const WorkspaceSessionContext = createContext<WorkspaceSessionContextValue | null>(null);

export function WorkspaceSessionProvider({ children }: { children: React.ReactNode }) {
  // Synchronous hydration → the tree paints immediately (snapshot-first).
  // warm first-paint cache only - Host is the authority; host read failure surfaces the error state
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

  // ── Host hydration (Slice 13) ─────────────────────────────────────────
  // The host (SQLite v27 via typed IPC) is the single data authority.
  // After mount: listWorkspaces → pick the active one (or [0]) →
  // getWorkspace → snapshot-store.setSnapshot + reducer 'host-sync'.
  // The localStorage warm-up above is first-paint only — never authoritative.
  const [hostStatus, setHostStatus] = useState<'pending' | 'ready' | 'error'>('pending');
  const [hostError, setHostError] = useState<string | null>(null);
  const [hostWorkspaceId, setHostWorkspaceId] = useState<string | null>(null);
  const [reloadKey, setReloadKey] = useState(0);

  /** Write the host read model into BOTH stores: snapshot-store (cache) + UI reducer. */
  const applyHostSnapshot = useCallback(
    (host: HostWorkspaceSnapshot | null) => {
      if (!host || !host.workspace) return;
      setSnapshot(host.workspace.id, host);
      setHostWorkspaceId(host.workspace.id);
      dispatch({ type: 'host-sync', snapshot: host });
      setHostStatus('ready');
    },
    [dispatch],
  );

  const reload = useCallback(() => {
    setHostError(null);
    setHostStatus('pending');
    if (hostWorkspaceId) invalidate(hostWorkspaceId);
    setReloadKey((k) => k + 1);
  }, [hostWorkspaceId]);

  // ── Write surface (Slice 14) ──────────────────────────────────────────
  // Every mutation calls the exact matching client fn with the active
  // workspaceId + expectedRevision (the revision of the snapshot last seen
  // by this client — a stale concurrent write surfaces the host's `Conflict`
  // error, never silently dropped), then re-fetches the snapshot through the
  // SAME getWorkspace the read path uses and writes it via setSnapshot so the
  // grid re-renders from the Host.

  /** Re-fetch the host snapshot and write it into both stores (read path). */
  const refreshHostSnapshot = useCallback(async (id: string): Promise<void> => {
    const fresh = await getWorkspace(id);
    setSnapshot(id, fresh);
    if (fresh) dispatch({ type: 'host-sync', snapshot: fresh });
  }, [dispatch]);

  const addWidget = useCallback(
    async (widgetType: string): Promise<void> => {
      if (!hostWorkspaceId) throw new Error('workspace not ready');
      const def = getWidget(widgetType);
      if (!def) throw new Error(`unregistered widget type: ${widgetType}`);
      const config = serializeWidgetConfig(createDefaultConfig(def));
      const revision = getSnapshot(hostWorkspaceId)?.revision;
      await upsertWidget(hostWorkspaceId, { widgetType, config }, revision);
      await refreshHostSnapshot(hostWorkspaceId);
    },
    [hostWorkspaceId, refreshHostSnapshot],
  );

  const removeWidgetAction = useCallback(
    async (widgetId: string): Promise<void> => {
      if (!hostWorkspaceId) throw new Error('workspace not ready');
      const revision = getSnapshot(hostWorkspaceId)?.revision;
      await removeWidget(hostWorkspaceId, widgetId, revision);
      await refreshHostSnapshot(hostWorkspaceId);
    },
    [hostWorkspaceId, refreshHostSnapshot],
  );

  const saveLayoutAction = useCallback(
    async (layouts: GridLayouts, activeBreakpoint?: 'lg' | 'md' | 'sm'): Promise<void> => {
      if (!hostWorkspaceId) throw new Error('workspace not ready');
      // One host row per breakpoint (UNIQUE(workspace_id, breakpoint) upsert).
      // The first save carries `expectedRevision`; the following ones omit it
      // because each successful write bumps the host revision (the client has
      // no way to know the new value). A stale concurrent write therefore
      // fails the whole batch up front with the host `Conflict` error —
      // before the re-fetch — never silently dropped.
      const breakpoints: ('lg' | 'md' | 'sm')[] = activeBreakpoint
        ? [...(['lg', 'md', 'sm'] as const).filter((bp) => bp !== activeBreakpoint), activeBreakpoint]
        : ['lg', 'md', 'sm'];
      let revision = getSnapshot(hostWorkspaceId)?.revision;
      for (const bp of breakpoints) {
        await saveLayout(hostWorkspaceId, bp, JSON.stringify(layouts[bp] ?? []), revision);
        revision = undefined; // host bumped the revision; skip re-checks within the batch
      }
      await refreshHostSnapshot(hostWorkspaceId);
    },
    [hostWorkspaceId, refreshHostSnapshot],
  );

  useEffect(() => {
    let cancelled = false;
    (async () => {
      setHostStatus('pending');
      setHostError(null);
      try {
        // 1) Resolve the active workspace id (fall back to the first one).
        const workspaces = await listWorkspaces();
        if (cancelled) return;
        const active =
          workspaces.find((w) => w.isActive) ??
          [...workspaces].sort((a, b) => a.position - b.position)[0] ??
          null;
        if (!active) {
          // No workspace on the host yet → create one (PLAN a1b).
          const created = await createWorkspace({ name: 'Workspace' });
          if (cancelled) return;
          applyHostSnapshot(created);
          return;
        }
        const workspaceId = active.id;
        setHostWorkspaceId(workspaceId);
        // 2) Serve the warm cache instantly (if any), then revalidate.
        const cached = getSnapshot(workspaceId);
        if (cached) applyHostSnapshot(cached);
        // 3) Host read model wins over any local cache.
        const snapshot = await getWorkspace(workspaceId);
        if (cancelled) return;
        applyHostSnapshot(snapshot);
      } catch (err) {
        if (cancelled) return;
        // Browser dev (no Tauri bridge) / host failure: keep the local
        // fallback visible, surface a real error state — never fabricate data.
        console.warn('[workspace] host hydration failed:', err);
        setHostStatus('error');
        setHostError(err instanceof Error ? err.message : String(err));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [reloadKey]);

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
      addWidget,
      removeWidget: removeWidgetAction,
      saveLayout: saveLayoutAction,
    }),
    [addWidget, removeWidgetAction, saveLayoutAction],
  );

  const value = useMemo(
    () => ({
      snapshot,
      dispatch,
      api,
      workspaceId: hostWorkspaceId,
      hostStatus,
      hostError,
      reloadHost: reload,
    }),
    [snapshot, api, hostWorkspaceId, hostStatus, hostError, reload],
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
