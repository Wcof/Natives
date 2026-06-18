import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import * as fs from 'fs';
import * as path from 'path';
import { readConfig, updateConfig } from './config-manager';

const TEST_DIR = path.join(process.env.HOME || '~', '.natives-config-test');
const TEST_FILE = path.join(TEST_DIR, 'test-config.json');

describe('ConfigManager', () => {
  before(() => {
    if (fs.existsSync(TEST_DIR)) {
      fs.rmSync(TEST_DIR, { recursive: true });
    }
    fs.mkdirSync(TEST_DIR, { recursive: true });
  });

  after(() => {
    if (fs.existsSync(TEST_DIR)) {
      fs.rmSync(TEST_DIR, { recursive: true });
    }
  });

  it('should return empty object for non-existent file', () => {
    const cfg = readConfig<Record<string, unknown>>('/tmp/nonexistent.json');
    assert.deepEqual(cfg, {});
  });

  it('should write and read config correctly', async () => {
    await updateConfig<{ name: string; count: number }>(TEST_FILE, (cfg) => ({
      ...cfg,
      name: 'test',
      count: 42,
    }));

    const cfg = readConfig<{ name: string; count: number }>(TEST_FILE);
    assert.equal(cfg.name, 'test');
    assert.equal(cfg.count, 42);
  });

  it('should partially update existing config', async () => {
    await updateConfig<{ name: string; count: number; extra?: string }>(TEST_FILE, (cfg) => ({
      ...cfg,
      extra: 'added',
    }));

    const cfg = readConfig<{ name: string; count: number; extra?: string }>(TEST_FILE);
    assert.equal(cfg.name, 'test');
    assert.equal(cfg.count, 42);
    assert.equal(cfg.extra, 'added');
  });

  it('should survive interrupted writes (file not corrupted)', async () => {
    // Write valid data first
    await updateConfig<{ status: string }>(TEST_FILE, () => ({ status: 'ok' }));

    // Simulate crash during write by writing directly to tmp
    const tmp = path.join(TEST_DIR, '.test-config.json.tmp');
    const badData = '{corrupted';
    fs.writeFileSync(tmp, badData, 'utf-8');
    // Do NOT rename - simulate crash before rename

    // Read should still get the original data
    const cfg = readConfig<{ status: string }>(TEST_FILE);
    assert.equal(cfg.status, 'ok');

    // Clean up tmp
    if (fs.existsSync(tmp)) fs.unlinkSync(tmp);
  });

  it('should serialize concurrent writes', async () => {
    const results: number[] = [];
    const promises = [1, 2, 3, 4, 5].map((i) =>
      updateConfig<{ values: number[] }>(TEST_FILE, (cfg) => ({
        ...cfg,
        values: [...(cfg.values || []), i],
      })),
    );

    await Promise.all(promises);

    const cfg = readConfig<{ values: number[] }>(TEST_FILE);
    assert.equal(cfg.values.length, 5);
    assert.deepEqual(cfg.values, [1, 2, 3, 4, 5]);
  });
});
