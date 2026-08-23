'use client';

/**
 * Workspace V2 — snapshot store.
 *
 * Client-side cache of complete `WorkspaceSnapshot` read models keyed by
 * workspace id. The cache is invalidated by workspace change events or
 * explicitly; never treat it as authoritative persistence (SQLite v27 is).
 */

import type { WorkspaceSnapshot } from './contracts';
import { getWorkspace } from './client';

type Listener = (snapshot: WorkspaceSnapshot | null) => void;

const cache = new Map<string, WorkspaceSnapshot>();
const MAX_SNAPSHOTS = 8;
const listeners = new Map<string, Set<Listener>>();

function emit(workspaceId: string, snapshot: WorkspaceSnapshot | null): void {
  const set = listeners.get(workspaceId);
  if (set) for (const listener of set) listener(snapshot);
}

export function getSnapshot(workspaceId: string): WorkspaceSnapshot | null {
  return cache.get(workspaceId) ?? null;
}

export function setSnapshot(workspaceId: string, snapshot: WorkspaceSnapshot | null): void {
  if (snapshot === null) {
    cache.delete(workspaceId);
  } else {
    cache.delete(workspaceId);
    cache.set(workspaceId, snapshot);
    while (cache.size > MAX_SNAPSHOTS) cache.delete(cache.keys().next().value as string);
  }
  emit(workspaceId, snapshot);
}

export function invalidate(workspaceId: string): void {
  cache.delete(workspaceId);
  emit(workspaceId, null);
}

export function invalidateAll(): void {
  for (const id of [...cache.keys()]) invalidate(id);
}

export function subscribeSnapshot(workspaceId: string, listener: Listener): () => void {
  let set = listeners.get(workspaceId);
  if (!set) {
    set = new Set();
    listeners.set(workspaceId, set);
  }
  set.add(listener);
  return () => {
    set?.delete(listener);
    if (set?.size === 0) listeners.delete(workspaceId);
  };
}

/** Fetch (and cache) the complete snapshot from the host. */
export async function loadSnapshot(workspaceId: string): Promise<WorkspaceSnapshot | null> {
  const snapshot = await getWorkspace(workspaceId);
  setSnapshot(workspaceId, snapshot);
  return snapshot;
}
