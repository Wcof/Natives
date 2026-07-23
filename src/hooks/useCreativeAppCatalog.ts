'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { useAsyncData } from '@/hooks/useAsyncData';
import type { CreativeAppSummary } from '@/lib/tauri-adapter';

/**
 * Unified Personal Creations catalog.
 * Reloads on db-state-changed channel === 'creative-app' | 'module'.
 * When the page is visible, reconciles external Docker state every 5s.
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

  // Visibility-gated reconcile every 5s for Docker + local process exits
  useEffect(() => {
    if (!enabled) return;
    const onVis = () => {
      visibleRef.current = document.visibilityState === 'visible';
    };
    document.addEventListener('visibilitychange', onVis);
    onVis();
    const timer = setInterval(() => {
      if (!visibleRef.current) return;
      const api = window.nativesAPI;
      if (!api?.creativeApp?.reconcile) return;
      // reconcile already polls local exits on the backend.
      void api.creativeApp.reconcile().then(() => reload()).catch(() => {});
    }, 5000);
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
      void reload();
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
