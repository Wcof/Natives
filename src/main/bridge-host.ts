import { getDb } from './database';

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
    .prepare("SELECT value FROM settings WHERE id = 'theme'")
    .get() as { value: string } | undefined;
  return { theme: row?.value || 'terminal-volt' };
}

export function setTheme(theme: string): void {
  getDb()
    .prepare("INSERT INTO settings (id, value, updated_at) VALUES ('theme', ?, datetime('now')) ON CONFLICT(id) DO UPDATE SET value = excluded.value, updated_at = datetime('now')")
    .run(theme);
}

// ── Locale ──

export function getLocale(): { locale: string } {
  const row = getDb()
    .prepare("SELECT value FROM settings WHERE id = 'locale'")
    .get() as { value: string } | undefined;
  return { locale: row?.value || 'zh-CN' };
}

export function setLocale(locale: string): void {
  getDb()
    .prepare("INSERT INTO settings (id, value, updated_at) VALUES ('locale', ?, datetime('now')) ON CONFLICT(id) DO UPDATE SET value = excluded.value, updated_at = datetime('now')")
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
