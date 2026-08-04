/**
 * Pure helpers for Creative App UI mapping / action availability.
 * Kept free of React for node:test coverage.
 */

import type {
  CreativeAppActions,
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
