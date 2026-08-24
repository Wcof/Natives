'use client';

import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from 'react';
import type { GridLayouts } from '@/lib/workspace/views/types';
import type { WorkspaceLayoutMode, WorkspaceSessionSnapshot, WorkspaceSnapshot, WorkspaceTemplate } from '@/lib/workspace/contracts';
import { closeSession, createWorkspace as createHostWorkspace, getSessionSnapshot, getWorkspace, listTemplates, openSession, removeWidget, reorderSessions, resetWidget, restoreTemplate, saveLayout, savePersonalTemplate, updateWorkspace, upsertWidget } from '@/lib/workspace/client';
import { setSession } from '@/lib/workspace/session-store';
import { setSnapshot } from '@/lib/workspace/snapshot-store';
import { onWorkspaceChanged } from '@/lib/workspace/events';
import { createDefaultConfig, getWidget, serializeWidgetConfig } from '@/lib/workspace/widgets';
import { applyTheme } from '@/lib/theme-engine';

export type WorkspaceHostStatus = 'pending' | 'ready' | 'error';

interface WorkspaceSessionApi {
  reload: () => void;
  openWorkspace: (id: string) => Promise<void>;
  createWorkspace: (templateId?: string) => Promise<void>;
  closeWorkspace: (id: string) => Promise<void>;
  reorderWorkspaces: (ids: string[]) => Promise<void>;
  setEditing: (editing: boolean) => void;
  setLayoutMode: (mode: WorkspaceLayoutMode) => Promise<void>;
  setTheme: (theme: 'dark' | 'light') => Promise<void>;
  addWidget: (type: string) => Promise<void>;
  removeWidget: (id: string) => Promise<void>;
  updateWidgetConfig: (id: string, config: Record<string, unknown>) => Promise<void>;
  resetWidget: (id: string) => Promise<void>;
  saveStructuredLayout: (layouts: GridLayouts, active: 'lg' | 'md' | 'sm') => Promise<void>;
  saveFreeLayout: (document: unknown) => Promise<void>;
  restoreTemplate: (id: string) => Promise<void>;
  saveTemplate: (name: string) => Promise<void>;
}

interface WorkspaceSessionContextValue {
  session: WorkspaceSessionSnapshot | null;
  snapshot: WorkspaceSnapshot | null;
  templates: WorkspaceTemplate[];
  status: WorkspaceHostStatus;
  error: string | null;
  editing: boolean;
  api: WorkspaceSessionApi;
}

const Context = createContext<WorkspaceSessionContextValue | null>(null);

export function WorkspaceSessionProvider({ children }: { children: React.ReactNode }) {
  const [session, setSessionState] = useState<WorkspaceSessionSnapshot | null>(null);
  const [snapshot, setSnapshotState] = useState<WorkspaceSnapshot | null>(null);
  const [templates, setTemplates] = useState<WorkspaceTemplate[]>([]);
  const [status, setStatus] = useState<WorkspaceHostStatus>('pending');
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState(false);
  const [reloadKey, setReloadKey] = useState(0);
  const snapshotRef = useRef(snapshot);
  snapshotRef.current = snapshot;

  const applySnapshot = useCallback((next: WorkspaceSnapshot | null) => {
    setSnapshotState(next);
    if (next) { setSnapshot(next.workspace.id, next); applyTheme(next.workspace.theme); }
  }, []);

  const refresh = useCallback(async (workspaceId?: string | null) => {
    const nextSession = await getSessionSnapshot();
    setSession(nextSession);
    setSessionState(nextSession);
    const id = workspaceId ?? nextSession.activeWorkspaceId;
    applySnapshot(id ? await getWorkspace(id) : null);
  }, [applySnapshot]);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      setStatus('pending'); setError(null);
      try {
        let next = await getSessionSnapshot();
        let id = next.activeWorkspaceId ?? next.openedTabs[0]?.workspaceId ?? next.workspaces[0]?.id;
        if (!id) {
          const created = await createHostWorkspace({ name: 'Personal Workspace', templateId: 'classic-personal-dashboard' });
          id = created.workspace.id;
          next = (await openSession(id)) ?? await getSessionSnapshot();
        } else if (!next.openedTabs.some((tab) => tab.workspaceId === id)) {
          next = (await openSession(id)) ?? next;
        }
        const [active, availableTemplates] = await Promise.all([getWorkspace(id), listTemplates()]);
        if (cancelled) return;
        setSession(next); setSessionState(next); applySnapshot(active); setTemplates(availableTemplates); setStatus('ready');
      } catch (cause) {
        if (cancelled) return;
        setStatus('error'); setError(cause instanceof Error ? cause.message : String(cause));
      }
    })();
    return () => { cancelled = true; };
  }, [reloadKey, applySnapshot]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    let timer: ReturnType<typeof setTimeout> | undefined;
    void onWorkspaceChanged((payload) => {
      if (disposed) return;
      clearTimeout(timer);
      timer = setTimeout(() => { void refresh(payload.workspaceId ?? snapshotRef.current?.workspace.id); }, 60);
    })
      .then((cleanup) => {
        if (disposed) {
          try {
            cleanup();
          } catch {
            // ignore cleanup errors on disposed component
          }
        } else {
          unlisten = cleanup;
        }
      })
      .catch(() => {});

    return () => {
      disposed = true;
      clearTimeout(timer);
      if (unlisten) {
        try {
          unlisten();
        } catch {
          // ignore cleanup errors on unmount
        }
        unlisten = undefined;
      }
    };
  }, [refresh]);

  const requireSnapshot = useCallback(() => {
    const current = snapshotRef.current;
    if (!current) throw new Error('workspace not ready');
    return current;
  }, []);

  const api = useMemo<WorkspaceSessionApi>(() => ({
    reload: () => setReloadKey((key) => key + 1),
    openWorkspace: async (id) => {
      const next = await openSession(id);
      if (next) { setSession(next); setSessionState(next); applySnapshot(await getWorkspace(id)); }
    },
    createWorkspace: async (templateId = 'classic-personal-dashboard') => {
      const created = await createHostWorkspace({ name: `Workspace ${Date.now().toString().slice(-4)}`, templateId });
      const next = await openSession(created.workspace.id);
      if (next) { setSession(next); setSessionState(next); applySnapshot(created); }
    },
    closeWorkspace: async (id) => {
      const next = await closeSession(id);
      setSession(next); setSessionState(next);
      const fallback = next.activeWorkspaceId ?? next.openedTabs[0]?.workspaceId ?? null;
      applySnapshot(fallback ? await getWorkspace(fallback) : null);
    },
    reorderWorkspaces: async (ids) => {
      const next = await reorderSessions(ids);
      setSession(next); setSessionState(next);
    },
    setEditing,
    setLayoutMode: async (mode) => {
      const current = requireSnapshot();
      applySnapshot(await updateWorkspace(current.workspace.id, { defaultLayoutMode: mode }, current.revision));
    },
    setTheme: async (theme) => {
      const current = requireSnapshot();
      applySnapshot(await updateWorkspace(current.workspace.id, { theme }, current.revision));
    },
    addWidget: async (type) => {
      const current = requireSnapshot();
      const def = getWidget(type);
      if (!def) throw new Error(`unregistered widget type: ${type}`);
      await upsertWidget(current.workspace.id, { widgetType: type, configVersion: 1, config: serializeWidgetConfig(createDefaultConfig(def)), enabled: true }, current.revision);
      await refresh(current.workspace.id);
    },
    removeWidget: async (id) => {
      const current = requireSnapshot();
      await removeWidget(current.workspace.id, id, current.revision);
      await refresh(current.workspace.id);
    },
    updateWidgetConfig: async (id, config) => {
      const current = requireSnapshot();
      const widget = current.widgets.find((item) => item.id === id);
      if (!widget) throw new Error('widget not found');
      await upsertWidget(current.workspace.id, { id, widgetType: widget.widgetType, configVersion: widget.configVersion, config, appearance: widget.appearance, enabled: widget.enabled, zIndex: widget.zIndex }, current.revision);
      await refresh(current.workspace.id);
    },
    resetWidget: async (id) => {
      const current = requireSnapshot();
      applySnapshot(await resetWidget(current.workspace.id, id, current.workspace.templateSourceId ?? undefined, current.revision));
    },
    saveStructuredLayout: async (layouts, active) => {
      const current = requireSnapshot();
      await saveLayout(current.workspace.id, 'structured', active, layouts[active] ?? [], current.revision);
      await refresh(current.workspace.id);
    },
    saveFreeLayout: async (document) => {
      const current = requireSnapshot();
      await saveLayout(current.workspace.id, 'free', 'free', document, current.revision);
      await refresh(current.workspace.id);
    },
    restoreTemplate: async (id) => {
      const current = requireSnapshot();
      applySnapshot(await restoreTemplate(current.workspace.id, id, current.revision));
    },
    saveTemplate: async (name) => {
      const current = requireSnapshot();
      await savePersonalTemplate(current.workspace.id, name);
      setTemplates(await listTemplates());
    },
  }), [applySnapshot, refresh, requireSnapshot]);

  return <Context.Provider value={{ session, snapshot, templates, status, error, editing, api }}>{children}</Context.Provider>;
}

export function useWorkspaceSession(): WorkspaceSessionContextValue {
  const value = useContext(Context);
  if (!value) throw new Error('useWorkspaceSession must be used inside WorkspaceSessionProvider');
  return value;
}
