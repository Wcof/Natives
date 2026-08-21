'use client';

/**
 * Workspace V2 — event surface.
 *
 * The Host emits `db-state-changed` events (channel `workspace`) on every
 * workspace mutation (see `src-tauri/src/commands/workspace.rs`). The same
 * payload is also broadcast on the dedicated `workspace` event name when the
 * bridge exposes per-domain events.
 *
 * Subscription helper resolves the active listener:
 * 1. `window.nativesAPI.event.listen` (repo bridge).
 * 2. `@tauri-apps/api/event` `listen` (Tauri v2) — lazily imported.
 *
 * Contract assumption: event names/payloads below must match the Host
 * `emit_db_state_changed` channel contract; verified by the parent.
 */

export const WORKSPACE_CHANNEL = 'workspace';
export const DB_STATE_CHANGED_EVENT = 'db-state-changed';
export const WORKSPACE_EVENT = 'workspace';

export type WorkspaceEventKind =
  | 'created'
  | 'updated'
  | 'deleted'
  | 'activeChanged'
  | 'sessionOpened'
  | 'sessionClosed'
  | 'tabChanged'
  | 'contextChanged'
  | 'widgetChanged'
  | 'layoutChanged'
  | 'viewStateChanged'
  | 'toolProfileChanged';

/** Payload broadcast by Host workspace commands. */
export interface WorkspaceChangedPayload {
  workspaceId?: string;
  event?: WorkspaceEventKind;
  channel?: string;
}

type Unlisten = () => void;
type Listener = (payload: WorkspaceChangedPayload) => void;

interface NativesEventAPI {
  listen?: (event: string, cb: (payload: unknown) => void) => Promise<Unlisten> | Unlisten;
}

function normalizePayload(raw: unknown): WorkspaceChangedPayload {
  if (raw && typeof raw === 'object') {
    const obj = raw as Record<string, unknown>;
    // db-state-changed envelopes sometimes nest under `data`.
    const inner = (obj.data && typeof obj.data === 'object' ? obj.data : obj) as Record<
      string,
      unknown
    >;
    return {
      workspaceId: typeof inner.workspaceId === 'string' ? inner.workspaceId : undefined,
      event: typeof inner.event === 'string' ? (inner.event as WorkspaceEventKind) : undefined,
      channel: typeof inner.channel === 'string' ? inner.channel : undefined,
    };
  }
  return {};
}

/**
 * Subscribe to workspace change events. Returns an unlisten function.
 * Accepts both the dedicated `workspace` event and the legacy
 * `db-state-changed` channel (filtered to `channel === 'workspace'`).
 */
export async function onWorkspaceChanged(cb: Listener): Promise<Unlisten> {
  const handle = (payload: WorkspaceChangedPayload) => {
    if (payload.channel && payload.channel !== WORKSPACE_CHANNEL) return;
    cb(payload);
  };

  // 1. Repo bridge namespace, when present.
  const eventApi = (window as unknown as { nativesAPI?: { event?: NativesEventAPI } })
    .nativesAPI?.event;
  if (eventApi?.listen) {
    const unlistens = await Promise.all([
      eventApi.listen(WORKSPACE_EVENT, (raw) => handle(normalizePayload(raw))),
      eventApi.listen(DB_STATE_CHANGED_EVENT, (raw) => handle(normalizePayload(raw))),
    ]);
    return () => unlistens.forEach((fn) => fn());
  }

  // 2. Tauri v2 event API (lazy import; absent in plain browser dev).
  try {
    const { listen } = await import('@tauri-apps/api/event');
    const unlistens = await Promise.all([
      listen<unknown>(WORKSPACE_EVENT, (e) => handle(normalizePayload(e.payload))),
      listen<unknown>(DB_STATE_CHANGED_EVENT, (e) => handle(normalizePayload(e.payload))),
    ]);
    return () => unlistens.forEach((fn) => fn());
  } catch {
    // No event bridge available (browser dev) — no-op subscription.
    return () => {};
  }
}
