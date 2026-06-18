"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.listModulePermissions = listModulePermissions;
exports.grantPermissionWithAudit = grantPermissionWithAudit;
exports.revokePermissionWithAudit = revokePermissionWithAudit;
exports.getAuditLog = getAuditLog;
exports.approveAllPermissions = approveAllPermissions;
exports.getAllKnownPermissions = getAllKnownPermissions;
const database_1 = require("./database");
// ── Permission Center API (TASK-001) ──
/**
 * List all permissions for a module.
 */
function listModulePermissions(moduleId) {
    return (0, database_1.getDb)()
        .prepare('SELECT module_id, permission, granted FROM module_permissions WHERE module_id = ? ORDER BY permission')
        .all(moduleId);
}
/**
 * Grant a permission with audit trail.
 */
function grantPermissionWithAudit(moduleId, permission, reason) {
    const db = (0, database_1.getDb)();
    db.prepare('UPDATE module_permissions SET granted = 1 WHERE module_id = ? AND permission = ?').run(moduleId, permission);
    db.prepare('INSERT INTO permission_audit_log (module_id, permission, action, granted, reason) VALUES (?, ?, \'grant\', 1, ?)').run(moduleId, permission, reason || null);
}
/**
 * Revoke a permission with audit trail.
 */
function revokePermissionWithAudit(moduleId, permission, reason) {
    const db = (0, database_1.getDb)();
    db.prepare('UPDATE module_permissions SET granted = 0 WHERE module_id = ? AND permission = ?').run(moduleId, permission);
    db.prepare('INSERT INTO permission_audit_log (module_id, permission, action, granted, reason) VALUES (?, ?, \'revoke\', 0, ?)').run(moduleId, permission, reason || null);
}
/**
 * Get audit trail for a module (or all modules).
 */
function getAuditLog(moduleId, limit = 50) {
    const db = (0, database_1.getDb)();
    if (moduleId) {
        return db.prepare('SELECT id, module_id, permission, action, granted, reason, created_at FROM permission_audit_log WHERE module_id = ? ORDER BY id DESC LIMIT ?').all(moduleId, limit);
    }
    return db.prepare('SELECT id, module_id, permission, action, granted, reason, created_at FROM permission_audit_log ORDER BY id DESC LIMIT ?').all(limit);
}
/**
 * Bulk-grant all permissions for a module (used during install approval).
 */
function approveAllPermissions(moduleId, reason) {
    const db = (0, database_1.getDb)();
    const rows = db.prepare('SELECT permission FROM module_permissions WHERE module_id = ? AND granted = 0').all(moduleId);
    const insertAudit = db.prepare('INSERT INTO permission_audit_log (module_id, permission, action, granted, reason) VALUES (?, ?, \'approve\', 1, ?)');
    const updateGrant = db.prepare('UPDATE module_permissions SET granted = 1 WHERE module_id = ? AND permission = ?');
    const txn = db.transaction(() => {
        for (const row of rows) {
            updateGrant.run(moduleId, row.permission);
            insertAudit.run(moduleId, row.permission, reason || 'Bulk approval on install');
        }
    });
    txn();
}
/**
 * Get all unique permission names across all modules.
 */
function getAllKnownPermissions() {
    const rows = (0, database_1.getDb)()
        .prepare('SELECT DISTINCT permission FROM module_permissions ORDER BY permission')
        .all();
    return rows.map((r) => r.permission);
}
//# sourceMappingURL=permission-center.js.map