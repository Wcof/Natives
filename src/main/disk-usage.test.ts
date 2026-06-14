import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import * as fs from 'fs';
import * as path from 'path';
import { getDiskUsage } from './disk-usage';

const DU_DIR = path.join(process.env.HOME || '~', '.natives-test', 'disk-usage-test');

describe('getDiskUsage', () => {
  before(() => {
    fs.mkdirSync(path.join(DU_DIR, 'subdir'), { recursive: true });
    fs.writeFileSync(path.join(DU_DIR, 'small.txt'), 'hello', 'utf-8');
    fs.writeFileSync(path.join(DU_DIR, 'subdir', 'nested.txt'), 'nested', 'utf-8');
  });

  after(() => {
    if (fs.existsSync(DU_DIR)) {
      fs.rmSync(DU_DIR, { recursive: true, force: true });
    }
  });

  it('should return sorted list of entries', async () => {
    const result = await getDiskUsage(DU_DIR);
    assert.ok(result.length >= 2);
    // Should be sorted by size descending
    for (let i = 1; i < result.length; i++) {
      assert.ok(result[i - 1]!.size >= result[i]!.size);
    }
  });

  it('should include files with correct size', async () => {
    const result = await getDiskUsage(DU_DIR);
    const small = result.find((e) => e.name === 'small.txt');
    assert.ok(small);
    assert.equal(small!.isDir, false);
    assert.ok(small!.size > 0);
    assert.ok(small!.sizeFormatted);
  });

  it('should include directories', async () => {
    const result = await getDiskUsage(DU_DIR);
    const subdir = result.find((e) => e.name === 'subdir');
    assert.ok(subdir);
    assert.equal(subdir!.isDir, true);
  });

  it('should throw for non-existent path', async () => {
    await assert.rejects(
      () => getDiskUsage('/nonexistent/path'),
    );
  });
});
