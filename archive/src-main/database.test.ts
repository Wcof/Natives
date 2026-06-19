import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import * as path from 'path';
import * as fs from 'fs';
import * as os from 'os';

const TEST_DB_DIR = path.join(os.tmpdir(), 'natives-database-test');

describe('Database', () => {
  let db: import('better-sqlite3').Database;
  let mod: typeof import('./database');

  before(async () => {
    // Clean test dir
    if (fs.existsSync(TEST_DB_DIR)) {
      fs.rmSync(TEST_DB_DIR, { recursive: true });
    }
    fs.mkdirSync(TEST_DB_DIR, { recursive: true });

    // Set env BEFORE dynamic import so module-level const picks it up
    process.env.NATIVES_DB_DIR = TEST_DB_DIR;

    // Dynamic import ensures paths are evaluated after env var is set
    mod = await import('./database');
    db = mod.initDb();
  });

  after(() => {
    mod.closeDb();
    if (fs.existsSync(TEST_DB_DIR)) {
      fs.rmSync(TEST_DB_DIR, { recursive: true });
    }
  });

  it('should initialize with all 10 tables', () => {
    const tables = db.pragma('table_list') as Array<{ name: string }>;
    const names = tables.map((t) => t.name).filter((n) => !n.startsWith('sqlite_'));
    assert.ok(names.includes('modules'));
    assert.ok(names.includes('module_permissions'));
    assert.ok(names.includes('settings'));
    assert.ok(names.includes('module_data'));
    assert.ok(names.includes('workshop_cache'));
    assert.ok(names.includes('env_profiles'));
    assert.ok(names.includes('env_variables'));
    assert.ok(names.includes('notifications'));
    assert.ok(names.includes('module_order'));
    assert.ok(names.includes('permission_audit_log'));
    assert.equal(names.length, 10);
  });

  it('should support CRUD operations on module_data', () => {
    mod.dbSet('test-module', 'greeting', 'hello');
    mod.dbSet('test-module', 'count', '42');

    assert.equal(mod.dbGet('test-module', 'greeting'), 'hello');
    assert.equal(mod.dbGet('test-module', 'count'), '42');

    mod.dbDelete('test-module', 'greeting');
    assert.equal(mod.dbGet('test-module', 'greeting'), undefined);
    assert.equal(mod.dbGet('test-module', 'count'), '42');
  });

  it('should namespace data by module_id', () => {
    mod.dbSet('module-a', 'key', 'value-a');
    mod.dbSet('module-b', 'key', 'value-b');

    assert.equal(mod.dbGet('module-a', 'key'), 'value-a');
    assert.equal(mod.dbGet('module-b', 'key'), 'value-b');
  });

  it('should list keys with optional prefix', () => {
    mod.dbSet('list-module', 'a:one', '1');
    mod.dbSet('list-module', 'a:two', '2');
    mod.dbSet('list-module', 'b:one', '3');

    const allKeys = mod.dbList('list-module');
    assert.ok(allKeys.includes('a:one'));
    assert.ok(allKeys.includes('a:two'));
    assert.ok(allKeys.includes('b:one'));

    const prefixedKeys = mod.dbList('list-module', 'a:');
    assert.equal(prefixedKeys.length, 2);
    assert.ok(prefixedKeys.includes('a:one'));
    assert.ok(prefixedKeys.includes('a:two'));
  });

  it('should handle re-initialization gracefully (idempotent)', () => {
    // Re-initializing should not fail
    const db2 = mod.initDb();
    const tables = db2.pragma('table_list') as Array<{ name: string }>;
    const names = tables.map((t) => t.name).filter((n) => !n.startsWith('sqlite_'));
    assert.equal(names.length, 10);

    // Data still accessible
    assert.equal(mod.dbGet('test-module', 'count'), '42');
  });
});
