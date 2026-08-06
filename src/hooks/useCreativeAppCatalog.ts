'use client';

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useAsyncData } from '@/hooks/useAsyncData';
import { deriveBusyIds, isOperationActive } from '@/lib/creative-app';
import type { CreativeAppOperation, CreativeAppSummary } from '@/lib/tauri-adapter';

/**
 * Unified Personal Creations catalog.
 * Reloads on db-state-changed channel === 'creative-app' | 'module'.
 * Reconciles on page entry/visibility recovery; running external apps get a
 * sparse 30s visible calibration instead of a fixed 5s Docker poll.
 *
 * Batch 2 CR-203: busy/error state projects the Host operation journal. The
 * renderer subscribes to `creative-operation` events (incremental) and takes an
 * initial snapshot, so an app is busy when the Host says a mutation is running
 * — not when this session happened to call a command. `busyIds` remains the
 * public interface (derived adapter), so existing components keep working.
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
        // Matches the Host backfill id convention for internal modules.
        applicationId: `app-internal-${m.id}`,
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

  // ── Operation journal projection (batch 2 CR-203) ───────────────────────

  /** Active operations keyed by operation id (in-flight Host mutations). */
  const [operations, setOperations] = useState<Map<number, CreativeAppOperation>>(new Map());
  /** Optimistic busy from commands this session started (also covers internal). */
  const [inFlight, setInFlight] = useState<Set<string>>(new Set());

  // Initial snapshot: any operation still in flight (e.g. a Docker install
  // started before this page mounted) must busy the app immediately.
  useEffect(() => {
    if (!enabled) return;
    const api = typeof window !== 'undefined' ? window.nativesAPI : undefined;
    if (!api?.creativeApp?.operations) return;
    api.creativeApp
      .operations()
      .then((ops) => {
        const next = new Map<number, CreativeAppOperation>();
        for (const op of ops ?? []) next.set(op.id, op);
        setOperations(next);
      })
      .catch(() => {});
  }, [enabled]);

  // Subscribe to operation changed events: incremental update from the Host,
  // and a catalog reload when an operation reaches a terminal phase (the app's
  // state/lastError changed). Cleaned up on unmount (R-E13).
  useEffect(() => {
    if (!enabled) return;
    const api = typeof window !== 'undefined' ? window.nativesAPI : undefined;
    if (!api?.creativeApp?.onOperationChanged) return;
    const unsub = api.creativeApp.onOperationChanged((op) => {
      setOperations((prev) => {
        if (!isOperationActive(op)) {
          // Terminal: drop it from the active projection.
          if (!prev.has(op.id)) return prev;
          const next = new Map(prev);
          next.delete(op.id);
          return next;
        }
        const next = new Map(prev);
        next.set(op.id, op);
        return next;
      });
      if (!isOperationActive(op)) {
        // Terminal: refresh the catalog (state / lastError changed).
        void reload();
      }
    });
    return () => {
      unsub?.();
    };
  }, [enabled, reload]);

  const derivedBusy = useMemo(
    () => deriveBusyIds(data ?? [], [...operations.values()]),
    [data, operations],
  );
  const busyIds = useMemo(() => {
    if (inFlight.size === 0) return derivedBusy;
    const all = new Set(derivedBusy);
    for (const id of inFlight) all.add(id);
    return all;
  }, [derivedBusy, inFlight]);

  const withBusy = useCallback(async <T,>(id: string, fn: () => Promise<T>): Promise<T | undefined> => {
    setInFlight((prev) => new Set(prev).add(id));
    try {
      return await fn();
    } finally {
      setInFlight((prev) => {
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

  /** Newest active operation for a source id, if the Host is mutating it. */
  const activeOperationFor = useCallback(
    (appId: string): CreativeAppOperation | undefined => {
      const app = (data ?? []).find((a) => a.id === appId);
      if (!app?.applicationId) return undefined;
      let newest: CreativeAppOperation | undefined;
      for (const op of operations.values()) {
        if (op.applicationId !== app.applicationId) continue;
        if (!isOperationActive(op)) continue;
        if (!newest || op.id > newest.id) newest = op;
      }
      return newest;
    },
    [data, operations],
  );

  return {
    apps: data ?? [],
    loading,
    error,
    reload,
    setError,
    busyIds,
    withBusy,
    operations: [...operations.values()],
    activeOperationFor,
  };
}
