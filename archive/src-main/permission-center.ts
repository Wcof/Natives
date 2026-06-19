import { getDb } from './database';

// ── Types ──

export interface PermissionRecord {
  module_id: string;
  permission: string;
  granted: number;
}

export interface AuditEntry {
  id: number;
  module_id: string;
  permission: string;
  action: 'grant' | 'revoke' | 'deny' | 'approve';
  granted: number;
  reason: string | null;
  created_at: string;
}

// ── Permission Center API (TASK-001) ──

/**
 * List all permissions for a module.
 */
export function listModulePermissions(moduleId: string): PermissionRecord[] {
  return getDb()
    .prepare('SELECT module_id, permission, granted FROM module_permissions WHERE module_id = ? ORDER BY permission')
    .all(moduleId) as PermissionRecord[];
}

/**
 * Grant a permission with audit trail.
 */
export function grantPermissionWithAudit(moduleId: string, permission: string, reason?: string): void {
  const db = getDb();
  db.prepare('UPDATE module_permissions SET granted = 1 WHERE module_id = ? AND permission = ?').run(moduleId, permission);
  db.prepare(
    'INSERT INTO permission_audit_log (module_id, permission, action, granted, reason) VALUES (?, ?, \'grant\', 1, ?)'
  ).run(moduleId, permission, reason || null);
}

/**
 * Revoke a permission with audit trail.
 */
export function revokePermissionWithAudit(moduleId: string, permission: string, reason?: string): void {
  const db = getDb();
  db.prepare('UPDATE module_permissions SET granted = 0 WHERE module_id = ? AND permission = ?').run(moduleId, permission);
  db.prepare(
    'INSERT INTO permission_audit_log (module_id, permission, action, granted, reason) VALUES (?, ?, \'revoke\', 0, ?)'
  ).run(moduleId, permission, reason || null);
}

/**
 * Get audit trail for a module (or all modules).
 */
export function getAuditLog(moduleId?: string, limit = 50): AuditEntry[] {
  const db = getDb();
  if (moduleId) {
    return db.prepare(
      'SELECT id, module_id, permission, action, granted, reason, created_at FROM permission_audit_log WHERE module_id = ? ORDER BY id DESC LIMIT ?'
    ).all(moduleId, limit) as AuditEntry[];
  }
  return db.prepare(
    'SELECT id, module_id, permission, action, granted, reason, created_at FROM permission_audit_log ORDER BY id DESC LIMIT ?'
  ).all(limit) as AuditEntry[];
}

/**
 * Bulk-grant all permissions for a module (used during install approval).
 */
export function approveAllPermissions(moduleId: string, reason?: string): void {
  const db = getDb();
  const rows = db.prepare(
    'SELECT permission FROM module_permissions WHERE module_id = ? AND granted = 0'
  ).all(moduleId) as { permission: string }[];
  const insertAudit = db.prepare(
    'INSERT INTO permission_audit_log (module_id, permission, action, granted, reason) VALUES (?, ?, \'approve\', 1, ?)'
  );
  const updateGrant = db.prepare(
    'UPDATE module_permissions SET granted = 1 WHERE module_id = ? AND permission = ?'
  );
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
export function getAllKnownPermissions(): string[] {
  const rows = getDb()
    .prepare('SELECT DISTINCT permission FROM module_permissions ORDER BY permission')
    .all() as { permission: string }[];
  return rows.map((r) => r.permission);
}
