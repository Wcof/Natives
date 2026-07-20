'use client';

import { useCallback, useEffect, useRef } from 'react';
import { useAsyncData } from '@/hooks/useAsyncData';

export interface ModuleCatalogItem {
  id: string;
  name: string;
  version: string;
  enabled: number | boolean;
  state: string;
  description?: string;
  author?: string;
  [key: string]: unknown;
}

export type ModuleCatalogSource = 'list' | 'scan';

export interface UseModuleCatalogOptions {
  source?: ModuleCatalogSource;
  enabled?: boolean;
}

/**
 * Personal Creations / web-module catalog with R-T6 hot refresh.
 * Reloads only when db-state-changed channel === 'module'.
 */
export function useModuleCatalog<T = ModuleCatalogItem>(
  options: UseModuleCatalogOptions = {},
) {
  const { source = 'list', enabled = true } = options;
  const sourceRef = useRef(source);
  sourceRef.current = source;

  const fetcher = useCallback(async (): Promise<T[]> => {
    const api = typeof window !== 'undefined' ? window.nativesAPI : undefined;
    if (!api?.module) return [];

    if (sourceRef.current === 'scan') {
      if (!api.module.scan) return [];
      const result = await api.module.scan();
      return (Array.isArray(result) ? result : []) as T[];
    }

    if (!api.module.list) return [];
    const result = await api.module.list();
    return (Array.isArray(result) ? result : []) as T[];
  }, []);

  const { data, loading, error, reload, setError } = useAsyncData<T[]>(fetcher, []);

  useEffect(() => {
    if (!enabled) return;
    void reload();
  }, [enabled, reload]);

  useEffect(() => {
    if (!enabled) return;
    const api = typeof window !== 'undefined' ? window.nativesAPI : undefined;
    if (!api?.onDbStateChanged) return;

    const unsub = api.onDbStateChanged((_event, channel) => {
      if (channel !== 'module') return;
      void reload();
    });
    return () => {
      unsub?.();
    };
  }, [enabled, reload]);

  return {
    modules: data ?? ([] as T[]),
    data: data ?? ([] as T[]),
    loading,
    error,
    reload,
    setError,
  };
}
