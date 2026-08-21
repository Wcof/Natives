/**
 * Workspace session snapshot persistence (C-001..C-006, snapshot-first).
 *
 * A single versioned JSON document is kept in localStorage (cache). Reads are
 * synchronous so the composition paints immediately from the cache; writes are
 * debounced (drag/resize never writes per-pointer). Wave2 swaps this to the
 * shared settings/persistence layer (see handoff contract assumptions).
 */

import { createDefaultWorkspaceSnapshot, WORKSPACE_SNAPSHOT_VERSION, WORKSPACE_STORAGE_KEY } from '@/lib/workspace/views/types';
import type { WorkspaceSnapshot } from '@/lib/workspace/views/types';

/** Synchronous cache read — first paint never waits on async I/O. */
export function loadWorkspaceSnapshotCache(): WorkspaceSnapshot | null {
  if (typeof window === 'undefined') return null;
  const raw = window.localStorage.getItem(WORKSPACE_STORAGE_KEY);
  if (!raw) return null;
  try {
    const parsed = JSON.parse(raw) as WorkspaceSnapshot;
    if (!parsed || parsed.version !== WORKSPACE_SNAPSHOT_VERSION) return null;
    return parsed;
  } catch {
    return null;
  }
}

/** Debounced snapshot saver (mirrors home's createDocumentSaver semantics). */
export function createWorkspaceSnapshotSaver() {
  let timer: ReturnType<typeof setTimeout> | null = null;
  let pending: WorkspaceSnapshot | null = null;

  return {
    schedule(snapshot: WorkspaceSnapshot) {
      pending = snapshot;
      if (timer) return;
      timer = setTimeout(() => {
        if (pending && typeof window !== 'undefined') {
          try {
            window.localStorage.setItem(WORKSPACE_STORAGE_KEY, JSON.stringify(pending));
          } catch {
            // Cache write failure is non-fatal; keep the in-memory snapshot.
          }
        }
        timer = null;
        pending = null;
      }, 250);
    },
    /** Flush immediately (e.g. before unload). */
    flush() {
      if (timer) {
        clearTimeout(timer);
        timer = null;
      }
      if (pending && typeof window !== 'undefined') {
        try {
          window.localStorage.setItem(WORKSPACE_STORAGE_KEY, JSON.stringify(pending));
        } catch {
          // non-fatal
        }
        pending = null;
      }
    },
  };
}

/** Rehydrate from cache, else build the default document synchronously. */
export function hydrateWorkspaceSnapshot(): WorkspaceSnapshot {
  const cached = loadWorkspaceSnapshotCache();
  if (cached) {
    // Defensive shape normalization — never let a corrupt cache blank the UI.
    return {
      ...cached,
      tabs: Array.isArray(cached.tabs) ? cached.tabs : [],
      views: cached.views && typeof cached.views === 'object' ? cached.views : {},
      session: { ...createDefaultWorkspaceSnapshot().session, ...cached.session },
    };
  }
  return createDefaultWorkspaceSnapshot();
}
