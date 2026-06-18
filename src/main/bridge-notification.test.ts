import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import * as path from 'path';
import * as fs from 'fs';
import * as os from 'os';

const TEST_DB_DIR = path.join(os.tmpdir(), 'natives-notif-test');
process.env.NATIVES_DB_DIR = TEST_DB_DIR;

import { initDb, closeDb, getDb } from './database';
import {
  sendNotification,
  getNotifications,
  markAsRead,
  markAllAsRead,
  getUnreadCount,
  setBadge,
  getBadge,
} from './bridge-notification';

describe('BridgeNotification', () => {
  before(() => {
    if (fs.existsSync(TEST_DB_DIR)) {
      // Close any existing db handle first
      try { closeDb(); } catch { /* ignore */ }
      // Use a longer delay to ensure lock is released
      fs.rmSync(TEST_DB_DIR, { recursive: true, force: true });
    }
    fs.mkdirSync(TEST_DB_DIR, { recursive: true });
    initDb();

    // Clear any stale data
    const ddb = getDb();
    ddb.exec('DELETE FROM notifications');
    ddb.exec('DELETE FROM module_permissions');
    ddb.exec('DELETE FROM modules');

    // Setup test module with notification permission
    ddb.prepare("INSERT OR IGNORE INTO modules (id, name, version, entry, type) VALUES ('notif-module', 'Notif', '1.0.0', 'index.html', 'web'), ('no-perm', 'NoPerm', '1.0.0', 'index.html', 'web')").run();
    ddb.prepare("INSERT OR IGNORE INTO module_permissions (module_id, permission, granted) VALUES ('notif-module', 'notification', 1)").run();
  });

  after(() => {
    closeDb();
    if (fs.existsSync(TEST_DB_DIR)) {
      fs.rmSync(TEST_DB_DIR, { recursive: true });
    }
  });

  it('should send and retrieve notifications', () => {
    // Check pre-condition
    assert.equal(getNotifications().length, 0);

    sendNotification('notif-module', 'Hello', 'This is a test', 'info');

    const notifs = getNotifications();
    assert.equal(notifs.length, 1, `Expected 1 notification, got ${notifs.length}`);
    assert.equal(notifs[0]!.title, 'Hello');
    assert.equal(notifs[0]!.body, 'This is a test');
    assert.equal(notifs[0]!.moduleId, 'notif-module');
  });

  it('should reject notification without permission', () => {
    assert.throws(() => {
      sendNotification('no-perm', 'Should not work', 'nope');
    }, /notification/);
  });

  it('should mark as read', () => {
    const notifs = getNotifications({ unreadOnly: true });
    assert.equal(notifs.length, 1); // previous notif is unread

    markAsRead(notifs[0]!.id!);

    const unread = getNotifications({ unreadOnly: true });
    assert.equal(unread.length, 0);
  });

  it('should mark all as read', () => {
    sendNotification('notif-module', 'Test 2', 'body');
    sendNotification('notif-module', 'Test 3', 'body');

    assert.equal(getUnreadCount(), 2);
    markAllAsRead();
    assert.equal(getUnreadCount(), 0);
  });

  it('should get unread count', () => {
    sendNotification('notif-module', 'Unread test', 'body');
    assert.equal(getUnreadCount(), 1);
  });

  it('should set and get badge', () => {
    setBadge('notif-module', 5);
    assert.equal(getBadge('notif-module'), 5);

    setBadge('notif-module', 0);
    assert.equal(getBadge('notif-module'), 0);
  });
});
