import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import * as fs from 'fs';
import * as path from 'path';
import { listDir } from './file-manager';

const TEST_DIR = path.join(process.env.HOME || '~', '.natives-test', 'listdir-test');

describe('listDir', () => {
  before(() => {
    // Create test directory structure
    fs.mkdirSync(path.join(TEST_DIR, 'subdir'), { recursive: true });
    fs.writeFileSync(path.join(TEST_DIR, 'a-file.txt'), 'a', 'utf-8');
    fs.writeFileSync(path.join(TEST_DIR, 'b-file.js'), 'b', 'utf-8');
    fs.writeFileSync(path.join(TEST_DIR, '中文文件.md'), 'c', 'utf-8');
    fs.writeFileSync(path.join(TEST_DIR, '啊测试.txt'), 'd', 'utf-8');
    fs.writeFileSync(path.join(TEST_DIR, '.env'), 'e', 'utf-8');
    fs.writeFileSync(path.join(TEST_DIR, '.DS_Store'), '', 'utf-8');

    // Create a symlink to a-file.txt
    fs.symlinkSync(
      path.join(TEST_DIR, 'a-file.txt'),
      path.join(TEST_DIR, 'link-to-a.txt'),
    );

    // Create a broken symlink
    fs.symlinkSync(
      path.join(TEST_DIR, 'nonexistent.txt'),
      path.join(TEST_DIR, 'broken-link'),
    );
  });

  after(() => {
    if (fs.existsSync(TEST_DIR)) {
      fs.rmSync(TEST_DIR, { recursive: true, force: true });
    }
  });

  it('should return all files and directories', async () => {
    const entries = await listDir(TEST_DIR);
    assert.ok(entries.length >= 7);
    const names = entries.map((e) => e.name).sort();
    assert.ok(names.includes('a-file.txt'));
    assert.ok(names.includes('b-file.js'));
    assert.ok(names.includes('subdir'));
    assert.ok(names.includes('link-to-a.txt'));
  });

  it('should filter .DS_Store', async () => {
    const entries = await listDir(TEST_DIR);
    const names = entries.map((e) => e.name);
    assert.equal(names.includes('.DS_Store'), false);
  });

  it('should include hidden files when showHidden is true', async () => {
    const entries = await listDir(TEST_DIR, { showHidden: true });
    const names = entries.map((e) => e.name);
    assert.ok(names.includes('.env'));
    // .DS_Store should still be filtered
    assert.equal(names.includes('.DS_Store'), false);
  });

  it('should return correct FileEntry properties', async () => {
    const entries = await listDir(TEST_DIR, { showHidden: true });
    const aFile = entries.find((e) => e.name === 'a-file.txt')!;
    assert.ok(aFile);
    assert.equal(aFile.isDir, false);
    assert.equal(aFile.kind, 'text');
    assert.equal(typeof aFile.size, 'number');
    assert.ok(aFile.size > 0);
    assert.equal(aFile.hidden, false);
    assert.equal(typeof aFile.mtime, 'number');
    assert.equal(typeof aFile.btime, 'number');
  });

  it('should mark directories correctly', async () => {
    const entries = await listDir(TEST_DIR);
    const subdir = entries.find((e) => e.name === 'subdir')!;
    assert.ok(subdir);
    assert.equal(subdir.isDir, true);
    assert.equal(subdir.kind, 'other');
  });

  it('should resolve symlinks', async () => {
    const entries = await listDir(TEST_DIR);
    const link = entries.find((e) => e.name === 'link-to-a.txt')!;
    assert.ok(link);
    assert.equal(link.symlink, path.join(TEST_DIR, 'a-file.txt'));
  });

  it('should handle broken symlinks', async () => {
    const entries = await listDir(TEST_DIR);
    const broken = entries.find((e) => e.name === 'broken-link')!;
    assert.ok(broken);
    assert.ok(broken.symlink);
  });

  it('should sort by name (asc) by default', async () => {
    const entries = await listDir(TEST_DIR, { showHidden: true });
    const names = entries.map((e) => e.name);
    for (let i = 1; i < names.length; i++) {
      assert.ok(names[i - 1]!.localeCompare(names[i]!, 'zh-CN', { numeric: true }) <= 0);
    }
  });

  it('should sort by mtime desc', async () => {
    const entries = await listDir(TEST_DIR, { sortBy: 'mtime', sortDir: 'desc' });
    for (let i = 1; i < entries.length; i++) {
      assert.ok(entries[i - 1]!.mtime >= entries[i]!.mtime);
    }
  });

  it('should sort by size desc', async () => {
    // Create files with different sizes
    fs.writeFileSync(path.join(TEST_DIR, 'large-file.bin'), Buffer.alloc(1024));
    fs.writeFileSync(path.join(TEST_DIR, 'small-file.bin'), Buffer.alloc(10));

    const entries = await listDir(TEST_DIR, { sortBy: 'size', sortDir: 'desc' });
    const filtered = entries.filter((e) => e.name.endsWith('.bin'));
    assert.ok(filtered.length >= 2);
    assert.equal(filtered[0]!.name, 'large-file.bin');
  });

  it('should throw for non-existent directory', async () => {
    await assert.rejects(
      () => listDir('/nonexistent/path/that/does/not/exist'),
      { code: 'ENOENT' },
    );
  });

  it('should throw for file path (not directory)', async () => {
    await assert.rejects(
      () => listDir(path.join(TEST_DIR, 'a-file.txt')),
    );
  });
});
