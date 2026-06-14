import { getDb } from './database';
import { checkPermission } from './bridge-host';

// ── Notification System ──

export interface Notification {
  id?: number;
  moduleId: string;
  title: string;
  body?: string;
  level: 'info' | 'warning' | 'error';
  read: number;
  createdAt: string;
}

export function sendNotification(
  moduleId: string,
  title: string,
  body?: string,
  level: 'info' | 'warning' | 'error' = 'info',
): void {
  // Permission check
  if (!checkPermission(moduleId, 'notification')) {
    throw new Error(`Module '${moduleId}' does not have 'notification' permission`);
  }

  getDb()
    .prepare(
      'INSERT INTO notifications (module_id, title, body, level, read, created_at) VALUES (?, ?, ?, ?, 0, datetime(\'now\'))',
    )
    .run(moduleId, title, body || null, level);
}

export function setBadge(moduleId: string, count: number): void {
  getDb()
    .prepare("INSERT INTO settings (id, value, updated_at) VALUES (?, ?, datetime('now')) ON CONFLICT(id) DO UPDATE SET value = excluded.value, updated_at = datetime('now')")
    .run(`badge:${moduleId}`, String(count));
}

export function getBadge(moduleId: string): number {
  const row = getDb()
    .prepare("SELECT value FROM settings WHERE id = ?")
    .get(`badge:${moduleId}`) as { value: string } | undefined;
  return row ? parseInt(row.value, 10) : 0;
}

export function getNotifications(
  options?: { unreadOnly?: boolean; limit?: number },
): Notification[] {
  let query = 'SELECT id, module_id as moduleId, title, body, level, read, created_at as createdAt FROM notifications';
  const params: unknown[] = [];

  if (options?.unreadOnly) {
    query += ' WHERE read = 0';
  }

  query += ' ORDER BY created_at DESC';

  if (options?.limit) {
    query += ' LIMIT ?';
    params.push(options.limit);
  }

  return getDb().prepare(query).all(...params) as Notification[];
}

export function markAsRead(notificationId: number): void {
  getDb().prepare('UPDATE notifications SET read = 1 WHERE id = ?').run(notificationId);
}

export function markAllAsRead(moduleId?: string): void {
  if (moduleId) {
    getDb().prepare('UPDATE notifications SET read = 1 WHERE module_id = ?').run(moduleId);
  } else {
    getDb().prepare('UPDATE notifications SET read = 1').run();
  }
}

export function getUnreadCount(): number {
  const row = getDb()
    .prepare('SELECT COUNT(*) as count FROM notifications WHERE read = 0')
    .get() as { count: number };
  return row.count;
}
