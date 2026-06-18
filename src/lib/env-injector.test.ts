import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import * as path from 'path';
import * as fs from 'fs';
import * as os from 'os';

const TEST_DIR = path.join(os.tmpdir(), 'natives-env-test');
process.env.NATIVES_DB_DIR = TEST_DIR;

import { initDb, closeDb, getDb } from '../main/database';
import {
  createProfile,
  deleteProfile,
  setVariable,
  getVariables,
  getDefaultProfile,
  injectEnv,
  listProfiles,
  getEncryptionKey,
} from './env-injector';

describe('EnvInjector', () => {
  let testKey: string;

  before(() => {
    if (fs.existsSync(TEST_DIR)) {
      fs.rmSync(TEST_DIR, { recursive: true, force: true });
    }
    fs.mkdirSync(TEST_DIR, { recursive: true });
    initDb();
    testKey = getEncryptionKey();
    // Clean up any leftover state from previous runs
    const db = getDb();
    db.exec('DELETE FROM env_variables');
    db.exec('DELETE FROM env_profiles');
  });

  after(() => {
    closeDb();
    if (fs.existsSync(TEST_DIR)) {
      fs.rmSync(TEST_DIR, { recursive: true, force: true });
    }
  });

  it('should create and list profiles', () => {
    createProfile('Work');
    createProfile('Personal');

    const profiles = listProfiles();
    assert.ok(profiles.find((p) => p.name === 'Work'));
    assert.ok(profiles.find((p) => p.name === 'Personal'));
  });

  it('should set and get variables', async () => {
    await setVariable('Work', 'API_KEY', 'sk-test-key', testKey);
    await setVariable('Work', 'MODEL', 'gpt-4', testKey);

    const vars = await getVariables('Work', testKey);
    assert.equal(vars.API_KEY, 'sk-test-key');
    assert.equal(vars.MODEL, 'gpt-4');
  });

  it('should isolate variables between profiles', async () => {
    const workVars = await getVariables('Work', testKey);
    const personalVars = await getVariables('Personal', testKey);

    assert.ok(Object.keys(workVars).length > 0);
    assert.equal(Object.keys(personalVars).length, 0);
  });

  it('should set and get default profile', () => {
    getDefaultProfile(); // Should not throw

    const profiles = listProfiles();
    const work = profiles.find((p) => p.name === 'Work')!;
    const personal = profiles.find((p) => p.name === 'Personal')!;

    // Mark Personal as default
    const db = getDb();
    db.prepare('UPDATE env_profiles SET is_default = 1 WHERE id = ?').run(personal.id);
    db.prepare('UPDATE env_profiles SET is_default = 0 WHERE id = ?').run(work.id);

    const defaultProfile = getDefaultProfile();
    assert.equal(defaultProfile?.name, 'Personal');
  });

  it('should inject env into target object', async () => {
    const env: Record<string, string> = { PATH: '/usr/bin' };
    await injectEnv('Work', env, testKey);

    assert.equal(env.PATH, '/usr/bin');
    assert.equal(env.API_KEY, 'sk-test-key');
    assert.equal(env.MODEL, 'gpt-4');
  });

  it('should delete profile', () => {
    deleteProfile('Personal');
    const profiles = listProfiles();
    assert.equal(profiles.find((p) => p.name === 'Personal'), undefined);
  });
});
