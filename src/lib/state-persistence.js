"use strict";
// ── State Persistence Layers ──
//
// Hot layer:   current visible iframe, JS memory preserved
// Warm layer:  up to 5 background iframes, kept in DOM (hidden)
// Cold layer:  beyond warm limit, iframe destroyed
// Persistent:  module_data table via db.set()/db.get()
//
// This module provides the persistent layer helpers and
// lifecycle hooks for state preservation.
Object.defineProperty(exports, "__esModule", { value: true });
exports.LAYER_CONFIG = void 0;
exports.saveModuleState = saveModuleState;
exports.loadModuleState = loadModuleState;
exports.clearModuleState = clearModuleState;
exports.createStatePreservationHook = createStatePreservationHook;
const database_1 = require("../main/database");
// ── State Keyspace ──
const STATE_PREFIX = '_state:';
function saveModuleState(moduleId, state) {
    const db = (0, database_1.getDb)();
    const key = `${STATE_PREFIX}${moduleId}`;
    const value = JSON.stringify(state);
    db.prepare('INSERT INTO module_data (module_id, key, value) VALUES (?, ?, ?) ON CONFLICT(module_id, key) DO UPDATE SET value = excluded.value').run('__system__', key, value);
}
function loadModuleState(moduleId) {
    const db = (0, database_1.getDb)();
    const key = `${STATE_PREFIX}${moduleId}`;
    const row = db
        .prepare('SELECT value FROM module_data WHERE module_id = ? AND key = ?')
        .get('__system__', key);
    if (!row)
        return null;
    try {
        return JSON.parse(row.value);
    }
    catch {
        return null;
    }
}
function clearModuleState(moduleId) {
    const db = (0, database_1.getDb)();
    const key = `${STATE_PREFIX}${moduleId}`;
    db.prepare('DELETE FROM module_data WHERE module_id = ? AND key = ?').run('__system__', key);
}
// ── Save on Unload ──
function createStatePreservationHook(moduleId, getState) {
    return () => {
        try {
            const state = getState();
            saveModuleState(moduleId, state);
        }
        catch {
            // Silently fail — state preservation should not crash the app
        }
    };
}
// ── Layer Configuration ──
exports.LAYER_CONFIG = {
    hot: { label: 'Hot', maxCount: 1 },
    warm: { label: 'Warm', maxCount: 5 },
    cold: { label: 'Cold', maxCount: Infinity, autoDestroy: true },
    persistent: { label: 'Persistent', storage: 'module_data' },
};
//# sourceMappingURL=state-persistence.js.map