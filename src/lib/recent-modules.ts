'use client';

import { startTransition, useState, useEffect, useCallback } from 'react';

// Lightweight LRU tracker for "recently used modules" on the Dashboard.
//
// Why localStorage instead of the `module_order` table: that table has no
// access-time field and is effectively unused (see ISSUE-3 audit). Adding a
// column would require a DB migration (deferred to US38). localStorage keeps
// this change front-end-only and zero-risk to existing data.
//
// Storage shape: stringified JSON array of module ids, most-recent-first,
// capped at MAX_ENTRIES (LRU eviction of the oldest).

const STORAGE_KEY = 'natives:recent_modules';
const MAX_ENTRIES = 8;

function readRaw(): string[] {
  if (typeof window === 'undefined') return [];
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    return Array.isArray(parsed) ? parsed.filter((x) => typeof x === 'string') : [];
  } catch {
    return [];
  }
}

function writeRaw(ids: string[]): void {
  if (typeof window === 'undefined') return;
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(ids.slice(0, MAX_ENTRIES)));
    // Notify listeners in the same tab (localStorage's `storage` event only
    // fires cross-tab, so we broadcast a custom event for same-tab updates).
    window.dispatchEvent(new CustomEvent('natives:recent-modules-changed'));
  } catch {
    /* localStorage unavailable — non-fatal */
  }
}

/** Record that a module was opened. Moves it to the front (LRU). */
export function pushRecentModule(moduleId: string): void {
  if (!moduleId) return;
  const ids = readRaw().filter((id) => id !== moduleId);
  ids.unshift(moduleId);
  writeRaw(ids);
}

/** Read the ordered list of recently-used module ids (most-recent-first). */
export function getRecentModules(limit = MAX_ENTRIES): string[] {
  return readRaw().slice(0, limit);
}

/**
 * Reactive hook returning the ordered recent module ids.
 * Re-renders when another part of the app calls pushRecentModule.
 */
export function useRecentModules(limit = MAX_ENTRIES): {
  ids: string[];
  refresh: () => void;
} {
  const [ids, setIds] = useState<string[]>(() => readRaw().slice(0, limit));

  const refresh = useCallback(() => {
    setIds(readRaw().slice(0, limit));
  }, [limit]);

  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect
    refresh();
    const onChange = () => refresh();
    // Cross-tab updates:
    window.addEventListener('storage', onChange);
    // Same-tab updates (from pushRecentModule):
    window.addEventListener('natives:recent-modules-changed', onChange);
    return () => {
      window.removeEventListener('storage', onChange);
      window.removeEventListener('natives:recent-modules-changed', onChange);
    };
  }, [refresh]);

  return { ids, refresh };
}
