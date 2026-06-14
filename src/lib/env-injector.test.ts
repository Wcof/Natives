import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import * as path from 'path';
import * as fs from 'fs';

const TEST_DIR = path.join(process.env.HOME || '~', '.natives-env-test');
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
} from './env-injector';

describe('EnvInjector', () => {
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
      fs.rmSync(TEST_DIR, { recursive: true });
    }
  });

  it('should create and list profiles', () => {
    createProfile('Work');
    createProfile('Personal');

    const profiles = listProfiles();
    assert.ok(profiles.find((p) => p.name === 'Work'));
    assert.ok(profiles.find((p) => p.name === 'Personal'));
  });

  it('should set and get variables', () => {
    setVariable('Work', 'API_KEY', 'sk-test-key');
    setVariable('Work', 'MODEL', 'gpt-4');

    const vars = getVariables('Work');
    assert.equal(vars.API_KEY, 'sk-test-key');
    assert.equal(vars.MODEL, 'gpt-4');
  });

  it('should isolate variables between profiles', () => {
    const workVars = getVariables('Work');
    const personalVars = getVariables('Personal');

    assert.ok(Object.keys(workVars).length > 0);
    assert.equal(Object.keys(personalVars).length, 0);
  });

  it('should set and get default profile', () => {
    getDefaultProfile(); // Should not throw

    // Work is first created, should be default
    // Set Personal as default
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

  it('should inject env into target object', () => {
    const env: Record<string, string> = { PATH: '/usr/bin' };
    injectEnv('Work', env);

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
