'use client';

/**
 * Workspace V2 — session store.
 *
 * Disposable renderer cache of the Host-owned open Workspace sessions.
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
export async function refreshSession(): Promise<WorkspaceSessionSnapshot> {
  const session = await getSessionSnapshot();
  setSession(session);
  return session;
}
