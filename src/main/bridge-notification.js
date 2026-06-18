"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.sendNotification = sendNotification;
exports.setBadge = setBadge;
exports.getBadge = getBadge;
exports.getNotifications = getNotifications;
exports.markAsRead = markAsRead;
exports.markAllAsRead = markAllAsRead;
exports.getUnreadCount = getUnreadCount;
const database_1 = require("./database");
const bridge_host_1 = require("./bridge-host");
function sendNotification(moduleId, title, body, level = 'info') {
    // Permission check — skip for system notifications
    if (moduleId !== '__system__' && !(0, bridge_host_1.checkPermission)(moduleId, 'notification')) {
        throw new Error(`Module '${moduleId}' does not have 'notification' permission`);
    }
    (0, database_1.getDb)()
        .prepare('INSERT INTO notifications (module_id, title, body, level, read, created_at) VALUES (?, ?, ?, ?, 0, datetime(\'now\'))')
        .run(moduleId, title, body || null, level);
}
function setBadge(moduleId, count) {
    (0, database_1.getDb)()
        .prepare("INSERT INTO settings (key, value, updated_at) VALUES (?, ?, datetime('now')) ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = datetime('now')")
        .run(`badge:${moduleId}`, String(count));
}
function getBadge(moduleId) {
    const row = (0, database_1.getDb)()
        .prepare("SELECT value FROM settings WHERE key = ?")
        .get(`badge:${moduleId}`);
    return row ? (parseInt(row.value, 10) || 0) : 0;
}
function getNotifications(options) {
    let query = 'SELECT id, module_id as moduleId, title, body, level, read, created_at as createdAt FROM notifications';
    const params = [];
    if (options?.unreadOnly) {
        query += ' WHERE read = 0';
    }
    query += ' ORDER BY created_at DESC';
    if (options?.limit) {
        query += ' LIMIT ?';
        params.push(options.limit);
    }
    return (0, database_1.getDb)().prepare(query).all(...params);
}
function markAsRead(notificationId) {
    (0, database_1.getDb)().prepare('UPDATE notifications SET read = 1 WHERE id = ?').run(notificationId);
}
function markAllAsRead(moduleId) {
    if (moduleId) {
        (0, database_1.getDb)().prepare('UPDATE notifications SET read = 1 WHERE module_id = ?').run(moduleId);
    }
    else {
        (0, database_1.getDb)().prepare('UPDATE notifications SET read = 1').run();
    }
}
function getUnreadCount() {
    const row = (0, database_1.getDb)()
        .prepare('SELECT COUNT(*) as count FROM notifications WHERE read = 0')
        .get();
    return row.count;
}
//# sourceMappingURL=bridge-notification.js.map