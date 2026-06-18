// ── State Persistence Layers ──
//
// Hot layer:   current visible iframe, JS memory preserved
// Warm layer:  up to 5 background iframes, kept in DOM (hidden)
// Cold layer:  beyond warm limit, iframe destroyed
// Persistent:  module_data table via db.set()/db.get()
//
// This module provides the persistent layer helpers and
// lifecycle hooks for state preservation.

import { getDb } from '../main/database';

// ── State Keyspace ──

const STATE_PREFIX = '_state:';

export function saveModuleState(moduleId: string, state: Record<string, unknown>): void {
  const db = getDb();
  const key = `${STATE_PREFIX}${moduleId}`;
  const value = JSON.stringify(state);
  db.prepare(
    'INSERT INTO module_data (module_id, key, value) VALUES (?, ?, ?) ON CONFLICT(module_id, key) DO UPDATE SET value = excluded.value',
  ).run('__system__', key, value);
}

export function loadModuleState(moduleId: string): Record<string, unknown> | null {
  const db = getDb();
  const key = `${STATE_PREFIX}${moduleId}`;
  const row = db
    .prepare('SELECT value FROM module_data WHERE module_id = ? AND key = ?')
    .get('__system__', key) as { value: string } | undefined;

  if (!row) return null;
  try {
    return JSON.parse(row.value);
  } catch {
    return null;
  }
}

export function clearModuleState(moduleId: string): void {
  const db = getDb();
  const key = `${STATE_PREFIX}${moduleId}`;
  db.prepare('DELETE FROM module_data WHERE module_id = ? AND key = ?').run('__system__', key);
}

// ── Save on Unload ──

export function createStatePreservationHook(
  moduleId: string,
  getState: () => Record<string, unknown>,
): () => void {
  return () => {
    try {
      const state = getState();
      saveModuleState(moduleId, state);
    } catch {
      // Silently fail — state preservation should not crash the app
    }
  };
}

// ── Layer Configuration ──

export const LAYER_CONFIG = {
  hot: { label: 'Hot', maxCount: 1 },
  warm: { label: 'Warm', maxCount: 5 },
  cold: { label: 'Cold', maxCount: Infinity, autoDestroy: true },
  persistent: { label: 'Persistent', storage: 'module_data' },
} as const;
