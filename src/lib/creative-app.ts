/**
 * Pure helpers for Creative App UI mapping / action availability.
 * Kept free of React for node:test coverage.
 */

import type {
  CreativeAppActions,
  CreativeAppOperation,
  CreativeAppOperationKind,
  CreativeAppOperationPhase,
  CreativeAppSource,
  CreativeAppState,
  CreativeAppSummary,
} from '@/lib/tauri-adapter';

export function shouldReloadCreativeCatalog(channel: string): boolean {
  return channel === 'creative-app' || channel === 'module';
}

export function sourceBadge(source: CreativeAppSource): 'internal' | 'github' | 'local' {
  if (source === 'external_github') return 'github';
  if (source === 'local_project') return 'local';
  return 'internal';
}

export function isActionBusy(state: CreativeAppState): boolean {
  return (
    state === 'installing' ||
    state === 'starting' ||
    state === 'stopping' ||
    state === 'deleting'
  );
}

export function mergeActionsWithBusy(
  actions: CreativeAppActions,
  busy: boolean,
): CreativeAppActions {
  if (!busy) return actions;
  return {
    canOpen: false,
    canStart: false,
    canStop: false,
    canDelete: false,
    canRetry: false,
  };
}

export function sortCreativeApps(apps: CreativeAppSummary[]): CreativeAppSummary[] {
  const rank = (s: CreativeAppState): number => {
    switch (s) {
      case 'running':
        return 0;
      case 'available':
        return 1;
      case 'installed_stopped':
        return 2;
      case 'start_failed':
      case 'install_failed':
      case 'cleanup_failed':
      case 'orphaned':
        return 3;
      default:
        return 4;
    }
  };
  return [...apps].sort((a, b) => {
    const d = rank(a.state) - rank(b.state);
    if (d !== 0) return d;
    return a.title.localeCompare(b.title);
  });
}

export function defaultDeleteOptions() {
  return { removeVolumes: false, removeImages: false };
}

/**
 * Whether starting this app should open its GUI automatically.
 * Only local projects carry autoOpen; external/github never auto-open from list.
 */
export function shouldAutoOpenAfterStart(app: CreativeAppSummary): boolean {
  if (app.source !== 'local_project') return false;
  return Boolean(app.localProject?.autoOpen);
}

/** Open surface for a summary — workshop iframe vs child webview URL. */
export function openSurfaceKind(
  app: CreativeAppSummary,
): 'workshop' | 'local_url' {
  return app.source === 'internal' ? 'workshop' : 'local_url';
}

/** Delete dialog needs Docker volume/image options only for external containers. */
export function deleteNeedsDockerOptions(source: CreativeAppSource): boolean {
  return source === 'external_github';
}

/**
 * Unified lifecycle prefers creativeApp.* for all sources.
 * Internal enable/disable/uninstall used to bypass creativeApp; adapters now
 * own that path so the command layer is one entry.
 */
export function prefersUnifiedLifecycleApi(): boolean {
  return true;
}

// ── Operation journal projection (batch 2 CR-203) ─────────────────────────

/** An operation still in flight (pending/waiting/running/compensating). */
export function isOperationActive(op: CreativeAppOperation): boolean {
  return (
    op.phase === 'pending' ||
    op.phase === 'waiting' ||
    op.phase === 'running' ||
    op.phase === 'compensating'
  );
}

/** An operation that reached a terminal phase (succeeded/failed/compensated/cancelled). */
export function isOperationTerminal(op: CreativeAppOperation): boolean {
  return !isOperationActive(op);
}

/**
 * Upsert an operation into the renderer's map (event-driven incremental
 * update). Returns a fresh map so React state stays immutable.
 */
export function upsertOperation(
  map: ReadonlyMap<number, CreativeAppOperation>,
  op: CreativeAppOperation,
): Map<number, CreativeAppOperation> {
  const next = new Map(map);
  next.set(op.id, op);
  return next;
}

/**
 * Derive the per-app busy set from Host operation facts (CR-203): an app is
 * busy when it has a non-terminal operation. Operations carry the unified
 * application id; the UI keys busy on the source id, so the mapping goes
 * through each summary's applicationId.
 */
export function deriveBusyIds(
  apps: CreativeAppSummary[],
  operations: CreativeAppOperation[],
): Set<string> {
  const sourceByAppId = new Map<string, string>();
  for (const app of apps) {
    if (app.applicationId) sourceByAppId.set(app.applicationId, app.id);
  }
  const busy = new Set<string>();
  for (const op of operations) {
    if (!isOperationActive(op)) continue;
    if (op.applicationId && sourceByAppId.has(op.applicationId)) {
      busy.add(sourceByAppId.get(op.applicationId)!);
    }
  }
  return busy;
}

/** i18n key path for a mutation kind's label (e.g. 'creative.operation.start'). */
export function operationLabelKey(kind: CreativeAppOperationKind): string {
  return `creative.operation.${kind}`;
}

/** i18n key path for an operation phase label (e.g. 'creative.operation.running'). */
export function operationPhaseLabelKey(phase: CreativeAppOperationPhase): string {
  return `creative.operation.${phase}`;
}
