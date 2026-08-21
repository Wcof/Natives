/**
 * Data view state persistence (C-027..C-031).
 *
 * Controlled view state (mode/columns/sort/filter/groupBy) is persisted per
 * view id through a versioned localStorage record. Writes are debounced; reads
 * are synchronous so the Data View renders immediately (snapshot-first, no
 * blank screen).
 */

import { useEffect, useRef, useState } from 'react';
import type { DataViewState } from './types';

const DATA_VIEW_STORAGE_PREFIX = 'natives.workspace.v2.dataview.';

const DEFAULT_DATA_VIEW_STATE: DataViewState = {
  mode: 'list',
  columns: ['name', 'status', 'assignee', 'due'],
  hiddenColumns: [],
  filters: [],
  sort: null,
  groupBy: null,
  calendarField: 'due',
};

function safeParse(raw: string | null, fallback: DataViewState): DataViewState {
  if (!raw) return fallback;
  try {
    const parsed = JSON.parse(raw) as Partial<DataViewState>;
    return {
      ...fallback,
      ...parsed,
      filters: Array.isArray(parsed.filters) ? parsed.filters : [],
      hiddenColumns: Array.isArray(parsed.hiddenColumns) ? parsed.hiddenColumns : [],
    };
  } catch {
    return fallback;
  }
}

/** Synchronous load — used for the first render (no async gap). */
export function loadDataViewState(viewId: string, fallback?: Partial<DataViewState>): DataViewState {
  const base = { ...DEFAULT_DATA_VIEW_STATE, ...fallback };
  if (typeof window === 'undefined') return base;
  return safeParse(window.localStorage.getItem(`${DATA_VIEW_STORAGE_PREFIX}${viewId}`), base);
}

/** Debounced write. */
export function saveDataViewState(viewId: string, state: DataViewState): void {
  if (typeof window === 'undefined') return;
  window.localStorage.setItem(`${DATA_VIEW_STORAGE_PREFIX}${viewId}`, JSON.stringify(state));
}

/**
 * Reactive variant used inside the Data View component: hydrates synchronously
 * from the cache and persists (debounced) on change.
 */
export function useDataViewState(
  viewId: string,
  fallback?: Partial<DataViewState>,
): [DataViewState, (patch: Partial<DataViewState>) => void] {
  const [state, setState] = useState<DataViewState>(() =>
    loadDataViewState(viewId, fallback),
  );
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const latestRef = useRef(state);
  latestRef.current = state;

  useEffect(() => {
    if (timerRef.current) clearTimeout(timerRef.current);
    timerRef.current = setTimeout(() => saveDataViewState(viewId, latestRef.current), 300);
    return () => {
      if (timerRef.current) clearTimeout(timerRef.current);
    };
  }, [state, viewId]);

  const patch = (update: Partial<DataViewState>) =>
    setState((prev) => ({ ...prev, ...update }));

  return [state, patch];
}
