'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { useAsyncData } from '@/hooks/useAsyncData';
import type { CreativeAppSummary } from '@/lib/tauri-adapter';

/**
 * Unified Personal Creations catalog.
 * Reloads on db-state-changed channel === 'creative-app' | 'module'.
 * Reconciles on page entry/visibility recovery; running external apps get a
 * sparse 30s visible calibration instead of a fixed 5s Docker poll.
 */
export function useCreativeAppCatalog(options: { enabled?: boolean } = {}) {
  const { enabled = true } = options;
  const visibleRef = useRef(true);

  const fetcher = useCallback(async (): Promise<CreativeAppSummary[]> => {
    const api = typeof window !== 'undefined' ? window.nativesAPI : undefined;
    if (!api?.creativeApp?.list) {
      // Fallback: map internal modules only
      if (!api?.module?.list) return [];
      const modules = (await api.module.list()) as Array<{
        id: string;
        name: string;
        version: string;
        enabled: number | boolean;
        state: string;
        description?: string;
      }>;
      return modules.map((m) => ({
        id: m.id,
        source: 'internal' as const,
        runtime: 'workshop_static' as const,
        title: m.name,
        description: m.description,
        version: m.version,
        state: m.enabled ? ('available' as const) : ('disabled' as const),
        actions: {
          canOpen: Boolean(m.enabled),
          canStart: !m.enabled,
          canStop: Boolean(m.enabled),
          canDelete: true,
          canRetry: false,
        },
      }));
    }
    const result = await api.creativeApp.list();
    return Array.isArray(result) ? result : [];
  }, []);

  const { data, loading, error, reload, setError } = useAsyncData<CreativeAppSummary[]>(
    fetcher,
    [],
  );
  const dataRef = useRef<CreativeAppSummary[]>([]);

  useEffect(() => {
    dataRef.current = data ?? [];
  }, [data]);

  useEffect(() => {
    if (!enabled) return;
    void reload();
  }, [enabled, reload]);

  useEffect(() => {
    if (!enabled) return;
    const api = typeof window !== 'undefined' ? window.nativesAPI : undefined;
    if (!api?.onDbStateChanged) return;
    const unsub = api.onDbStateChanged((_event, channel) => {
      if (channel !== 'creative-app' && channel !== 'module') return;
      void reload();
    });
    return () => {
      unsub?.();
    };
  }, [enabled, reload]);

  useEffect(() => {
    if (!enabled) return;
    const reconcile = () => {
      const api = window.nativesAPI;
      if (!api?.creativeApp?.reconcile) return;
      void api.creativeApp.reconcile().then(() => reload()).catch(() => {});
    };
    const onVis = () => {
      const visible = document.visibilityState === 'visible';
      if (visible && !visibleRef.current) reconcile();
      visibleRef.current = visible;
    };
    document.addEventListener('visibilitychange', onVis);
    visibleRef.current = document.visibilityState === 'visible';
    if (visibleRef.current) reconcile();
    const timer = setInterval(() => {
      if (!visibleRef.current) return;
      const runningExternal = dataRef.current.some(
        (app) => app.source === 'external_github' && app.state === 'running',
      );
      if (runningExternal) reconcile();
    }, 30000);
    return () => {
      clearInterval(timer);
      document.removeEventListener('visibilitychange', onVis);
    };
  }, [enabled, reload]);

  const [busyIds, setBusyIds] = useState<Set<string>>(new Set());

  const withBusy = useCallback(async <T,>(id: string, fn: () => Promise<T>): Promise<T | undefined> => {
    setBusyIds((prev) => new Set(prev).add(id));
    try {
      return await fn();
    } finally {
      setBusyIds((prev) => {
        const next = new Set(prev);
        next.delete(id);
        return next;
      });
      const api = typeof window !== 'undefined' ? window.nativesAPI : undefined;
      const reconcile = api?.creativeApp?.reconcile;
      if (reconcile) void reconcile().catch(() => {}).finally(() => reload());
      else void reload();
    }
  }, [reload]);

  return {
    apps: data ?? [],
    loading,
    error,
    reload,
    setError,
    busyIds,
    withBusy,
  };
}
