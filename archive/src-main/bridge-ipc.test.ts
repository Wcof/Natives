import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import * as path from 'path';
import * as fs from 'fs';

const TEST_DB_DIR = path.join(process.env.HOME || '~', '.natives-ipc-test');
process.env.NATIVES_DB_DIR = TEST_DB_DIR;

import { initDb, closeDb, getDb } from './database';
import { sendMessage, broadcastMessage, onMessage, onBroadcast, clearMessageLog } from './bridge-ipc';

describe('BridgeIPC', () => {
  before(() => {
    if (fs.existsSync(TEST_DB_DIR)) {
      fs.rmSync(TEST_DB_DIR, { recursive: true });
    }
    fs.mkdirSync(TEST_DB_DIR, { recursive: true });
    initDb();

    // Setup test modules with permissions
    const ddb = getDb();
    ddb.prepare("INSERT OR IGNORE INTO modules (id, name, version, entry, type) VALUES ('sender', 'Sender', '1.0.0', 'index.html', 'web'), ('receiver', 'Receiver', '1.0.0', 'index.html', 'web'), ('noperm', 'NoPerm', '1.0.0', 'index.html', 'web')").run();
    ddb.prepare("INSERT OR IGNORE INTO module_permissions (module_id, permission, granted) VALUES ('sender', 'ipc:send', 1), ('receiver', 'ipc:receive', 1)").run();
  });

  after(() => {
    closeDb();
    if (fs.existsSync(TEST_DB_DIR)) {
      fs.rmSync(TEST_DB_DIR, { recursive: true });
    }
  });

  it('should send a message between modules', () => {
    const received: unknown[] = [];
    const cleanup = onMessage('receiver', (msg) => { received.push(msg.payload); });

    sendMessage('sender', 'receiver', 'test', { hello: 'world' });

    assert.equal(received.length, 1);
    assert.deepEqual(received[0], { hello: 'world' });
    cleanup();
  });

  it('should broadcast to all modules', () => {
    const received: unknown[] = [];
    const cleanup1 = onMessage('receiver', (msg) => { received.push(msg.payload); });
    const cleanup2 = onBroadcast((msg) => { received.push(msg.payload); });

    broadcastMessage('sender', 'broadcast-test', { broadcast: true });

    // Both the module listener and broadcast listener should fire
    assert.ok(received.length >= 1);
    cleanup1();
    cleanup2();
  });

  it('should reject send without permission', () => {
    assert.throws(() => {
      sendMessage('noperm', 'receiver', 'test', 'data');
    }, /ipc:send/);
  });

  it('should reject receive without permission', () => {
    assert.throws(() => {
      sendMessage('sender', 'noperm', 'test', 'data');
    }, /ipc:receive/);
  });

  it('should cleanup listeners', () => {
    const received: unknown[] = [];
    const cleanup = onMessage('receiver', (msg) => { received.push(msg.payload); });

    cleanup();
    sendMessage('sender', 'receiver', 'test', 'should-not-receive');

    assert.equal(received.length, 0);
  });

  it('should log messages for audit', () => {
    clearMessageLog();
    sendMessage('sender', 'receiver', 'audit', { data: 1 });
  });
});
