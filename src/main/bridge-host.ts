import { getDb, dbGet, dbSet, dbDelete, dbList } from './database';

// ── HTTP Bridge Request Handler ──

export async function handleBridgeRequest(
  namespace: string,
  method: string,
  moduleId: string,
  data: unknown
): Promise<unknown> {
  const key = `${namespace}.${method}`;

  switch (key) {
    // Data operations
    case 'db.get': {
      if (!checkPermission(moduleId, 'db:read')) return { error: 'Permission denied' };
      const { key: k } = data as { key: string };
      const value = dbGet(moduleId, k);
      return { value };
    }
    case 'db.set': {
      if (!checkPermission(moduleId, 'db:write')) return { error: 'Permission denied' };
      const { key: k, value: v } = data as { key: string; value: string };
      dbSet(moduleId, k, v);
      return { ok: true };
    }
    case 'db.delete': {
      if (!checkPermission(moduleId, 'db:write')) return { error: 'Permission denied' };
      const { key: k } = data as { key: string };
      dbDelete(moduleId, k);
      return { ok: true };
    }
    case 'db.list': {
      if (!checkPermission(moduleId, 'db:read')) return { error: 'Permission denied' };
      const { prefix } = data as { prefix?: string };
      return { keys: dbList(moduleId, prefix) };
    }

    // Settings
    case 'settings.getTheme': {
      return getTheme();
    }
    case 'settings.getLocale': {
      return getLocale();
    }

    // Lifecycle
    case 'lifecycle.ready': {
      markReady(moduleId);
      return { ok: true };
    }
    case 'lifecycle.heartbeat': {
      markHeartbeat(moduleId);
      return { ok: true };
    }
    case 'lifecycle.error': {
      const { message } = data as { message: string };
      markError(moduleId, message);
      return { ok: true };
    }

    // Meta
    case 'meta.info': {
      return {
        moduleId,
        nativesVersion: NATIVES_VERSION,
      };
    }

    default:
      return { error: `Unknown bridge method: ${key}` };
  }
}

// ── Permission Checking ──

export function checkPermission(moduleId: string, permission: string): boolean {
  const row = getDb()
    .prepare('SELECT granted FROM module_permissions WHERE module_id = ? AND permission = ?')
    .get(moduleId, permission) as { granted: number } | undefined;
  return row?.granted === 1;
}

export function grantPermission(moduleId: string, permission: string): void {
  getDb()
    .prepare('UPDATE module_permissions SET granted = 1 WHERE module_id = ? AND permission = ?')
    .run(moduleId, permission);
}

export function revokePermission(moduleId: string, permission: string): void {
  getDb()
    .prepare('UPDATE module_permissions SET granted = 0 WHERE module_id = ? AND permission = ?')
    .run(moduleId, permission);
}

// ── Theme ──

export function getTheme(): { theme: string } {
  const row = getDb()
    .prepare("SELECT value FROM settings WHERE key = 'theme'")
    .get() as { value: string } | undefined;
  return { theme: row?.value || 'terminal-volt' };
}

export function setTheme(theme: string): void {
  getDb()
    .prepare("INSERT INTO settings (key, value, updated_at) VALUES ('theme', ?, datetime('now')) ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = datetime('now')")
    .run(theme);
}

// ── Locale ──

export function getLocale(): { locale: string } {
  const row = getDb()
    .prepare("SELECT value FROM settings WHERE key = 'locale'")
    .get() as { value: string } | undefined;
  return { locale: row?.value || 'zh-CN' };
}

export function setLocale(locale: string): void {
  getDb()
    .prepare("INSERT INTO settings (key, value, updated_at) VALUES ('locale', ?, datetime('now')) ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = datetime('now')")
    .run(locale);
}

// ── Lifecycle (heartbeat tracking via IframeSandboxManager) ──
// Lifecycle events are tracked in iframe-sandbox.ts
// This module provides the Bridge-side handlers

export interface LifecycleState {
  ready: boolean;
  readyAt: string | null;
  heartbeatCount: number;
  lastHeartbeatAt: string | null;
  error: string | null;
}

const lifecycleStates = new Map<string, LifecycleState>();

export function markReady(moduleId: string): void {
  lifecycleStates.set(moduleId, {
    ready: true,
    readyAt: new Date().toISOString(),
    heartbeatCount: 0,
    lastHeartbeatAt: null,
    error: null,
  });
}

export function markHeartbeat(moduleId: string): void {
  const state = lifecycleStates.get(moduleId);
  if (state) {
    state.heartbeatCount++;
    state.lastHeartbeatAt = new Date().toISOString();
  } else {
    lifecycleStates.set(moduleId, {
      ready: false,
      readyAt: null,
      heartbeatCount: 1,
      lastHeartbeatAt: new Date().toISOString(),
      error: null,
    });
  }
}

export function markError(moduleId: string, error: string): void {
  const state = lifecycleStates.get(moduleId) || {
    ready: false,
    readyAt: null,
    heartbeatCount: 0,
    lastHeartbeatAt: null,
    error: null,
  };
  state.error = error;
  lifecycleStates.set(moduleId, state);
}

export function getLifecycleState(moduleId: string): LifecycleState | undefined {
  return lifecycleStates.get(moduleId);
}

export function cleanupLifecycle(moduleId: string): void {
  lifecycleStates.delete(moduleId);
}

// ── Meta ──

export const NATIVES_VERSION = '0.1.0';
