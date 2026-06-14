import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import * as path from 'path';
import * as fs from 'fs';

const TEST_DB_DIR = path.join(process.env.HOME || '~', '.natives-bridge-test');
process.env.NATIVES_DB_DIR = TEST_DB_DIR;

import { initDb, closeDb, getDb } from './database';
import {
  checkPermission,
  grantPermission,
  getTheme,
  setTheme,
  getLocale,
  setLocale,
  markReady,
  markHeartbeat,
  markError,
  getLifecycleState,
  cleanupLifecycle,
  NATIVES_VERSION,
} from './bridge-host';

describe('BridgeHost', () => {
  before(() => {
    if (fs.existsSync(TEST_DB_DIR)) {
      fs.rmSync(TEST_DB_DIR, { recursive: true });
    }
    fs.mkdirSync(TEST_DB_DIR, { recursive: true });
    initDb();

    // Ensure test module permissions exist
    const ddb = getDb();
    ddb.prepare("INSERT OR IGNORE INTO modules (id, name, version, entry, type) VALUES ('test-module', 'Test', '1.0.0', 'index.html', 'web')").run();
    ddb.prepare("INSERT OR IGNORE INTO module_permissions (module_id, permission, granted) VALUES ('test-module', 'db:read', 0), ('test-module', 'db:write', 0), ('test-module', 'notification', 0)").run();
  });

  after(() => {
    closeDb();
    if (fs.existsSync(TEST_DB_DIR)) {
      fs.rmSync(TEST_DB_DIR, { recursive: true });
    }
  });

  describe('Permissions', () => {
    it('should reject permission by default', () => {
      assert.equal(checkPermission('test-module', 'db:read'), false);
    });

    it('should grant and check permission', () => {
      grantPermission('test-module', 'db:read');
      assert.equal(checkPermission('test-module', 'db:read'), true);
    });

    it('should reject for non-existent module', () => {
      assert.equal(checkPermission('unknown', 'db:read'), false);
    });
  });

  describe('Theme & Locale', () => {
    it('should return default theme', () => {
      const { theme } = getTheme();
      assert.equal(theme, 'terminal-volt');
    });

    it('should set and get theme', () => {
      setTheme('warm-archive');
      const { theme } = getTheme();
      assert.equal(theme, 'warm-archive');
    });

    it('should return default locale', () => {
      const { locale } = getLocale();
      assert.equal(locale, 'zh-CN');
    });

    it('should set and get locale', () => {
      setLocale('en');
      const { locale } = getLocale();
      assert.equal(locale, 'en');
    });
  });

  describe('Lifecycle', () => {
    it('should track ready state', () => {
      markReady('lifecycle-test');
      const state = getLifecycleState('lifecycle-test');
      assert.equal(state?.ready, true);
      assert.ok(state?.readyAt);
    });

    it('should track heartbeat', () => {
      markHeartbeat('hb-test');
      markHeartbeat('hb-test');
      const state = getLifecycleState('hb-test');
      assert.equal(state?.heartbeatCount, 2);
    });

    it('should track errors', () => {
      markError('err-test', 'Something went wrong');
      const state = getLifecycleState('err-test');
      assert.equal(state?.error, 'Something went wrong');
    });

    it('should clean up lifecycle state', () => {
      markReady('cleanup-test');
      assert.ok(getLifecycleState('cleanup-test'));
      cleanupLifecycle('cleanup-test');
      assert.equal(getLifecycleState('cleanup-test'), undefined);
    });
  });

  describe('Meta', () => {
    it('should export current natives version', () => {
      assert.equal(NATIVES_VERSION, '0.1.0');
    });
  });
});
