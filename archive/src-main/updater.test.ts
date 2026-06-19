import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import * as path from 'path';
import * as fs from 'fs';
import * as os from 'os';
import { compareVersions, checkForUpdates } from './updater';

const TEST_DIR = path.join(os.tmpdir(), 'natives-updater-test');
const MODULES_DIR = path.join(TEST_DIR, 'modules', 'update-module');

describe('Updater', () => {
  before(() => {
    process.env.NATIVES_DB_DIR = TEST_DIR;
    // Ensure DB is ready
    const { initDb } = require('./database');
    initDb();
  });

  after(() => {
    try { const { closeDb } = require('./database'); closeDb(); } catch { /* ignore */ }
    if (fs.existsSync(TEST_DIR)) {
      fs.rmSync(TEST_DIR, { recursive: true, force: true });
    }
  });

  describe('compareVersions', () => {
    it('should return 0 for equal versions', () => {
      assert.equal(compareVersions('1.0.0', '1.0.0'), 0);
    });

    it('should return 1 when first is newer', () => {
      assert.equal(compareVersions('2.0.0', '1.0.0'), 1);
    });

    it('should return -1 when first is older', () => {
      assert.equal(compareVersions('1.0.0', '2.0.0'), -1);
    });

    it('should handle patch versions', () => {
      assert.equal(compareVersions('1.0.1', '1.0.0'), 1);
      assert.equal(compareVersions('1.0.0', '1.0.1'), -1);
    });

    it('should handle unequal length versions', () => {
      assert.equal(compareVersions('1.0', '1.0.0'), 0);
      assert.equal(compareVersions('2', '1.9.9'), 1);
    });
  });

  describe('checkForUpdates', () => {
    it('should return empty array when no modules installed', () => {
      const results = checkForUpdates();
      assert.ok(Array.isArray(results));
    });

    it('should detect newer version from manifest', () => {
      // Create test module directory and DB entry
      if (!fs.existsSync(TEST_DIR)) fs.mkdirSync(TEST_DIR, { recursive: true });
      if (!fs.existsSync(MODULES_DIR)) fs.mkdirSync(MODULES_DIR, { recursive: true });

      // Write manifest with newer version
      fs.writeFileSync(
        path.join(MODULES_DIR, 'manifest.json'),
        JSON.stringify({ id: 'update-module', name: 'Update Test', version: '2.0.0', entry: 'index.html', type: 'web' }),
        'utf-8',
      );

      // Setup DB with older version
      const { initDb } = require('./database');
      initDb();
      const db = require('./database').getDb();
      db.prepare("INSERT OR IGNORE INTO modules (id, name, version, entry, type) VALUES ('update-module', 'Update Test', '1.0.0', 'index.html', 'web')").run();

      const results = checkForUpdates();
      const update = results.find((r) => r.moduleId === 'update-module');
      assert.ok(update);
      assert.equal(update!.status, 'update-available');
    });
  });
});
