import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import * as path from 'path';
import * as fs from 'fs';
import * as os from 'os';

// These tests need better-sqlite3; for unit test we use the database module
// which requires NATIVES_DB_DIR to be set
const TEST_DB_DIR = path.join(os.tmpdir(), 'natives-installer-test');
process.env.NATIVES_DB_DIR = TEST_DB_DIR;

import { initDb, closeDb } from './database';
import { installModule, uninstallModule, enableModule, disableModule, getInstalledModules } from './installer';

const TEST_MODULES_DIR = path.join(process.env.HOME || '~', '.natives', 'modules');

describe('Installer', () => {
  before(() => {
    if (fs.existsSync(TEST_DB_DIR)) {
      fs.rmSync(TEST_DB_DIR, { recursive: true });
    }
    fs.mkdirSync(TEST_DB_DIR, { recursive: true });
    initDb();
  });

  after(() => {
    closeDb();
    if (fs.existsSync(TEST_DB_DIR)) {
      fs.rmSync(TEST_DB_DIR, { recursive: true });
    }
    if (fs.existsSync(TEST_MODULES_DIR)) {
      fs.rmSync(TEST_MODULES_DIR, { recursive: true });
    }
  });

  it('should install a module from directory', () => {
    const srcDir = path.join(TEST_DB_DIR, 'source-module');
    fs.mkdirSync(srcDir, { recursive: true });
    fs.writeFileSync(
      path.join(srcDir, 'manifest.json'),
      JSON.stringify({
        id: 'com.test.fromdir',
        name: 'From Dir',
        version: '1.0.0',
        entry: 'index.html',
        type: 'web',
        permissions: ['db:read'],
      }),
      'utf-8',
    );
    fs.writeFileSync(path.join(srcDir, 'index.html'), '<h1>From Dir</h1>', 'utf-8');

    const result = installModule(srcDir);
    assert.equal(result.success, true);

    // Check files were copied
    const destDir = path.join(TEST_MODULES_DIR, 'com.test.fromdir');
    assert.equal(fs.existsSync(path.join(destDir, 'manifest.json')), true);

    // Check DB entry
    const modules = getInstalledModules();
    const mod = modules.find((m) => m.id === 'com.test.fromdir');
    assert.ok(mod);
    assert.equal(mod!.name, 'From Dir');
  });

  it('should reject invalid manifest', () => {
    const srcDir = path.join(TEST_DB_DIR, 'bad-module');
    fs.mkdirSync(srcDir, { recursive: true });
    fs.writeFileSync(path.join(srcDir, 'manifest.json'), JSON.stringify({ name: 'No ID' }), 'utf-8');

    const result = installModule(srcDir);
    assert.equal(result.success, false);
    assert.ok(result.error!.includes('manifest'));
  });

  it('should uninstall a module', () => {
    // First install
    const srcDir = path.join(TEST_DB_DIR, 'uninstall-me');
    fs.mkdirSync(srcDir, { recursive: true });
    fs.writeFileSync(
      path.join(srcDir, 'manifest.json'),
      JSON.stringify({
        id: 'com.test.uninstall',
        name: 'Uninstall Me',
        version: '1.0.0',
        entry: 'index.html',
        type: 'web',
      }),
      'utf-8',
    );
    fs.writeFileSync(path.join(srcDir, 'index.html'), '<h1>Uninstall</h1>', 'utf-8');
    installModule(srcDir);

    // Then uninstall
    const result = uninstallModule('com.test.uninstall');
    assert.equal(result.success, true);

    // Check files deleted
    const modDir = path.join(TEST_MODULES_DIR, 'com.test.uninstall');
    assert.equal(fs.existsSync(modDir), false);

    // Check DB entry deleted
    const modules = getInstalledModules();
    assert.equal(modules.find((m) => m.id === 'com.test.uninstall'), undefined);
  });

  it('should enable/disable module status', () => {
    // Install
    const srcDir = path.join(TEST_DB_DIR, 'toggle-module');
    fs.mkdirSync(srcDir, { recursive: true });
    fs.writeFileSync(
      path.join(srcDir, 'manifest.json'),
      JSON.stringify({
        id: 'com.test.toggle',
        name: 'Toggle',
        version: '1.0.0',
        entry: 'index.html',
        type: 'web',
      }),
      'utf-8',
    );
    installModule(srcDir);

    // Initially enabled
    let mod = getInstalledModules().find((m) => m.id === 'com.test.toggle');
    assert.equal(mod!.enabled, 1);

    // Disable
    disableModule('com.test.toggle');
    mod = getInstalledModules().find((m) => m.id === 'com.test.toggle');
    assert.equal(mod!.enabled, 0);

    // Re-enable
    enableModule('com.test.toggle');
    mod = getInstalledModules().find((m) => m.id === 'com.test.toggle');
    assert.equal(mod!.enabled, 1);
  });
});
