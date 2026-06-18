import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import * as path from 'path';
import * as fs from 'fs';
import * as os from 'os';

const TEST_DIR = path.join(os.tmpdir(), 'natives-state-test');
process.env.NATIVES_DB_DIR = TEST_DIR;

import { initDb, closeDb } from '../main/database';
import { saveModuleState, loadModuleState, clearModuleState } from './state-persistence';

describe('StatePersistence', () => {
  before(() => {
    if (fs.existsSync(TEST_DIR)) {
      fs.rmSync(TEST_DIR, { recursive: true, force: true });
    }
    fs.mkdirSync(TEST_DIR, { recursive: true });
    initDb();
  });

  after(() => {
    closeDb();
    if (fs.existsSync(TEST_DIR)) {
      fs.rmSync(TEST_DIR, { recursive: true, force: true });
    }
  });

  it('should save and load module state', () => {
    saveModuleState('com.test.app', { count: 42, name: 'test', items: [1, 2, 3] });

    const state = loadModuleState('com.test.app');
    assert.ok(state);
    assert.equal(state!.count, 42);
    assert.equal(state!.name, 'test');
    assert.deepEqual(state!.items, [1, 2, 3]);
  });

  it('should isolate state between modules', () => {
    saveModuleState('module-a', { data: 'aaa' });
    saveModuleState('module-b', { data: 'bbb' });

    const a = loadModuleState('module-a');
    const b = loadModuleState('module-b');
    assert.equal(a!.data, 'aaa');
    assert.equal(b!.data, 'bbb');
  });

  it('should return null for unknown module', () => {
    const state = loadModuleState('nonexistent');
    assert.equal(state, null);
  });

  it('should clear state', () => {
    saveModuleState('com.test.temp', { temp: true });
    assert.ok(loadModuleState('com.test.temp'));

    clearModuleState('com.test.temp');
    assert.equal(loadModuleState('com.test.temp'), null);
  });

  it('should overwrite existing state', () => {
    saveModuleState('com.test.overwrite', { version: 1 });
    saveModuleState('com.test.overwrite', { version: 2 });

    const state = loadModuleState('com.test.overwrite');
    assert.equal(state!.version, 2);
  });
});
