'use client';

/**
 * Workspace V2 — session store.
 *
 * Holds the runtime `WorkspaceSessionSnapshot` of the currently open workspace
 * (no dedicated backend session table — the session is the open workspace's
 * live state assembled from the seven v27 tables).
 */

import type { WorkspaceSessionSnapshot } from './contracts';
import { getSessionSnapshot } from './client';

type Listener = (session: WorkspaceSessionSnapshot | null) => void;

let current: WorkspaceSessionSnapshot | null = null;
const listeners = new Set<Listener>();

export function getSession(): WorkspaceSessionSnapshot | null {
  return current;
}

export function setSession(session: WorkspaceSessionSnapshot | null): void {
  current = session;
  for (const listener of listeners) listener(current);
}

export function clearSession(): void {
  setSession(null);
}

export function subscribeSession(listener: Listener): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** Re-fetch the session snapshot from the host and publish it. */
export async function refreshSession(workspaceId: string): Promise<WorkspaceSessionSnapshot | null> {
  try {
    const session = await getSessionSnapshot(workspaceId);
    setSession(session);
    return session;
  } catch {
    return current;
  }
}
