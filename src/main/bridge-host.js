"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.NATIVES_VERSION = void 0;
exports.handleBridgeRequest = handleBridgeRequest;
exports.checkPermission = checkPermission;
exports.grantPermission = grantPermission;
exports.revokePermission = revokePermission;
exports.getTheme = getTheme;
exports.setTheme = setTheme;
exports.getLocale = getLocale;
exports.setLocale = setLocale;
exports.markReady = markReady;
exports.markHeartbeat = markHeartbeat;
exports.markError = markError;
exports.getLifecycleState = getLifecycleState;
exports.cleanupLifecycle = cleanupLifecycle;
const database_1 = require("./database");
const bridge_notification_1 = require("./bridge-notification");
const bridge_ipc_1 = require("./bridge-ipc");
// ── HTTP Bridge Request Handler ──
async function handleBridgeRequest(namespace, method, moduleId, data) {
    const key = `${namespace}.${method}`;
    switch (key) {
        // Data operations
        case 'db.get': {
            if (!checkPermission(moduleId, 'db:read'))
                return { error: 'Permission denied' };
            const { key: k } = data;
            const value = (0, database_1.dbGet)(moduleId, k);
            return { value };
        }
        case 'db.set': {
            if (!checkPermission(moduleId, 'db:write'))
                return { error: 'Permission denied' };
            const { key: k, value: v } = data;
            (0, database_1.dbSet)(moduleId, k, v);
            return { ok: true };
        }
        case 'db.delete': {
            if (!checkPermission(moduleId, 'db:write'))
                return { error: 'Permission denied' };
            const { key: k } = data;
            (0, database_1.dbDelete)(moduleId, k);
            return { ok: true };
        }
        case 'db.list': {
            if (!checkPermission(moduleId, 'db:read'))
                return { error: 'Permission denied' };
            const { prefix } = data;
            return { keys: (0, database_1.dbList)(moduleId, prefix) };
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
            const { message } = data;
            markError(moduleId, message);
            // Write crash notification to DB
            try {
                (0, bridge_notification_1.sendNotification)('__system__', `Plugin ${moduleId} crashed`, message || 'Heartbeat timeout', 'error');
            }
            catch { /* ignore */ }
            // Notify renderer about the crash
            try {
                const { BrowserWindow } = require('electron');
                const win = BrowserWindow.getAllWindows()[0];
                if (win && !win.isDestroyed()) {
                    win.webContents.send('notification', {
                        type: 'crash',
                        moduleId,
                        message,
                    });
                }
            }
            catch { /* ignore */ }
            return { ok: true };
        }
        // Meta
        case 'meta.info': {
            return {
                moduleId,
                nativesVersion: exports.NATIVES_VERSION,
            };
        }
        // Environment variables
        case 'env.get': {
            if (!checkPermission(moduleId, 'env:read'))
                return { error: 'Permission denied' };
            const { key } = data;
            try {
                const db = (0, database_1.getDb)();
                if (!db)
                    return { error: 'Database not available' };
                // Use getDefaultProfile() (by is_default flag) instead of hardcoded name='default'
                const { getDefaultProfile, getEncryptionKey, decrypt } = require('../lib/env-injector');
                const profile = getDefaultProfile();
                if (!profile)
                    return { value: null };
                const row = db.prepare('SELECT value_encrypted FROM env_variables WHERE profile_id = ? AND key = ?').get(profile.id, key);
                if (!row)
                    return { value: null };
                // ★ P2-2: Use shared decrypt() which handles safeStorage + AES-256-GCM
                return { value: decrypt(row.value_encrypted, getEncryptionKey()) };
            }
            catch (err) {
                return { error: `Failed to read env variable: ${err.message}` };
            }
        }
        // Notifications
        case 'notification.send': {
            if (!checkPermission(moduleId, 'notification'))
                return { error: 'Permission denied' };
            const { title, body, level } = data;
            (0, bridge_notification_1.sendNotification)(moduleId, title, body, (level || 'info'));
            return { ok: true };
        }
        case 'notification.badge': {
            if (!checkPermission(moduleId, 'notification'))
                return { error: 'Permission denied' };
            const { count } = data;
            (0, bridge_notification_1.setBadge)(moduleId, count);
            return { ok: true };
        }
        // IPC
        case 'ipc.send': {
            if (!checkPermission(moduleId, 'ipc:send'))
                return { error: 'Permission denied' };
            const { target, payload } = data;
            (0, bridge_ipc_1.sendMessage)(moduleId, target, 'bridge', payload);
            return { ok: true };
        }
        case 'ipc.broadcast': {
            if (!checkPermission(moduleId, 'ipc:send'))
                return { error: 'Permission denied' };
            const { payload: bcPayload } = data;
            (0, bridge_ipc_1.broadcastMessage)(moduleId, 'bridge', bcPayload);
            return { ok: true };
        }
        default:
            return { error: `Unknown bridge method: ${key}` };
    }
}
// ── Permission Checking ──
function checkPermission(moduleId, permission) {
    const row = (0, database_1.getDb)()
        .prepare('SELECT granted FROM module_permissions WHERE module_id = ? AND permission = ?')
        .get(moduleId, permission);
    return row?.granted === 1;
}
function grantPermission(moduleId, permission) {
    (0, database_1.getDb)()
        .prepare('UPDATE module_permissions SET granted = 1 WHERE module_id = ? AND permission = ?')
        .run(moduleId, permission);
}
function revokePermission(moduleId, permission) {
    (0, database_1.getDb)()
        .prepare('UPDATE module_permissions SET granted = 0 WHERE module_id = ? AND permission = ?')
        .run(moduleId, permission);
}
// ── Theme ──
function getTheme() {
    const row = (0, database_1.getDb)()
        .prepare("SELECT value FROM settings WHERE key = 'theme'")
        .get();
    return { theme: row?.value || 'terminal-volt' };
}
function setTheme(theme) {
    (0, database_1.getDb)()
        .prepare("INSERT INTO settings (key, value, updated_at) VALUES ('theme', ?, datetime('now')) ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = datetime('now')")
        .run(theme);
}
// ── Locale ──
function getLocale() {
    const row = (0, database_1.getDb)()
        .prepare("SELECT value FROM settings WHERE key = 'locale'")
        .get();
    return { locale: row?.value || 'zh-CN' };
}
function setLocale(locale) {
    (0, database_1.getDb)()
        .prepare("INSERT INTO settings (key, value, updated_at) VALUES ('locale', ?, datetime('now')) ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = datetime('now')")
        .run(locale);
}
const lifecycleStates = new Map();
function markReady(moduleId) {
    lifecycleStates.set(moduleId, {
        ready: true,
        readyAt: new Date().toISOString(),
        heartbeatCount: 0,
        lastHeartbeatAt: null,
        error: null,
    });
}
function markHeartbeat(moduleId) {
    const state = lifecycleStates.get(moduleId);
    if (state) {
        state.heartbeatCount++;
        state.lastHeartbeatAt = new Date().toISOString();
    }
    else {
        lifecycleStates.set(moduleId, {
            ready: false,
            readyAt: null,
            heartbeatCount: 1,
            lastHeartbeatAt: new Date().toISOString(),
            error: null,
        });
    }
}
function markError(moduleId, error) {
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
function getLifecycleState(moduleId) {
    return lifecycleStates.get(moduleId);
}
function cleanupLifecycle(moduleId) {
    lifecycleStates.delete(moduleId);
}
// ── Meta ──
exports.NATIVES_VERSION = '0.1.0';
//# sourceMappingURL=bridge-host.js.map