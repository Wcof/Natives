import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import * as fs from 'fs';
import * as path from 'path';
import { listDir, readFile, streamFile, writeFileAtomic, createEntry, renameEntry } from './file-manager';

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

const READ_DIR = path.join(process.env.HOME || '~', '.natives-test', 'readfile-test');

describe('readFile', () => {
  before(() => {
    fs.mkdirSync(READ_DIR, { recursive: true });
    fs.writeFileSync(path.join(READ_DIR, 'small.txt'), 'Hello, World!', 'utf-8');
    fs.writeFileSync(path.join(READ_DIR, 'multiline.txt'), 'line1\nline2\nline3\n', 'utf-8');
    fs.writeFileSync(path.join(READ_DIR, 'chinese.txt'), '你好，世界！', 'utf-8');

    // Create a file larger than 2MB to test truncation
    const largeSize = 3 * 1024 * 1024; // 3MB
    const buffer = Buffer.alloc(largeSize, 'A');
    fs.writeFileSync(path.join(READ_DIR, 'large.txt'), buffer);
  });

  after(() => {
    if (fs.existsSync(READ_DIR)) {
      fs.rmSync(READ_DIR, { recursive: true, force: true });
    }
  });

  it('should read small text file', async () => {
    const result = await readFile(path.join(READ_DIR, 'small.txt'));
    assert.equal(result.content, 'Hello, World!');
    assert.equal(result.truncated, false);
    assert.equal(result.size, 13);
    assert.equal(result.encoding, 'utf-8');
  });

  it('should read multiline text file', async () => {
    const result = await readFile(path.join(READ_DIR, 'multiline.txt'));
    assert.ok(result.content.includes('line2'));
    assert.equal(result.truncated, false);
  });

  it('should read Chinese text file', async () => {
    const result = await readFile(path.join(READ_DIR, 'chinese.txt'));
    assert.ok(result.content.includes('你好'));
    assert.equal(result.truncated, false);
  });

  it('should truncate files larger than 2MB', async () => {
    const result = await readFile(path.join(READ_DIR, 'large.txt'));
    assert.equal(result.truncated, true);
    assert.ok(result.content.length <= 256 * 1024);
    assert.equal(result.size, 3 * 1024 * 1024);
  });

  it('should throw ENOENT for non-existent file', async () => {
    await assert.rejects(
      () => readFile(path.join(READ_DIR, 'nonexistent.txt')),
      { code: 'ENOENT' },
    );
  });

  it('should include file size in result', async () => {
    const result = await readFile(path.join(READ_DIR, 'small.txt'));
    assert.equal(typeof result.size, 'number');
    assert.ok(result.size > 0);
  });
});

const STREAM_DIR = path.join(process.env.HOME || '~', '.natives-test', 'streamfile-test');

describe('streamFile', () => {
  before(() => {
    fs.mkdirSync(STREAM_DIR, { recursive: true });
    fs.writeFileSync(path.join(STREAM_DIR, 'hello.txt'), 'Hello, World! This is a test file.', 'utf-8');
    fs.writeFileSync(path.join(STREAM_DIR, 'typescript.ts'), 'const x: number = 42;', 'utf-8');
  });

  after(() => {
    if (fs.existsSync(STREAM_DIR)) {
      fs.rmSync(STREAM_DIR, { recursive: true, force: true });
    }
  });

  it('should return full file stream without range', async () => {
    const result = await streamFile(path.join(STREAM_DIR, 'hello.txt'));
    assert.equal(result.contentType, 'text/plain');
    assert.equal(result.totalSize, 34);

    // Collect stream data
    const chunks: Buffer[] = [];
    for await (const chunk of result.stream) {
      chunks.push(chunk as Buffer);
    }
    result.stream.destroy();
    const content = Buffer.concat(chunks).toString('utf-8');
    assert.equal(content, 'Hello, World! This is a test file.');
    assert.equal(result.contentRange, undefined);
  });

  it('should read a range with start only', async () => {
    const result = await streamFile(path.join(STREAM_DIR, 'hello.txt'), { start: 7 });
    assert.equal(result.contentType, 'text/plain');

    const chunks: Buffer[] = [];
    for await (const chunk of result.stream) {
      chunks.push(chunk as Buffer);
    }
    const content = Buffer.concat(chunks).toString('utf-8');
    assert.equal(content, 'World! This is a test file.');
    assert.ok(result.contentRange!.startsWith('bytes 7-33/34'));
  });

  it('should detect text content type for .md files', async () => {
    fs.writeFileSync(path.join(STREAM_DIR, 'readme.md'), '# Hello', 'utf-8');
    const result = await streamFile(path.join(STREAM_DIR, 'readme.md'));
    assert.equal(result.contentType, 'text/markdown');
    result.stream.destroy();
  });

  it('should detect image content type for .png files', async () => {
    // Create a minimal valid PNG
    const pngBuffer = Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]);
    fs.writeFileSync(path.join(STREAM_DIR, 'image.png'), pngBuffer);
    const result = await streamFile(path.join(STREAM_DIR, 'image.png'));
    assert.equal(result.contentType, 'image/png');
    result.stream.destroy();
  });

  it('should return generic type for unknown extensions', async () => {
    fs.writeFileSync(path.join(STREAM_DIR, 'data.bin'), 'binary', 'utf-8');
    const result = await streamFile(path.join(STREAM_DIR, 'data.bin'));
    assert.equal(result.contentType, 'application/octet-stream');
    result.stream.destroy();
  });

  it('should throw ENOENT for non-existent file', async () => {
    await assert.rejects(
      () => streamFile(path.join(STREAM_DIR, 'nonexistent.txt')),
      { code: 'ENOENT' },
    );
  });
});

const ATOMIC_DIR = path.join(process.env.HOME || '~', '.natives-test', 'atomic-test');

describe('writeFileAtomic', () => {
  before(() => {
    fs.mkdirSync(ATOMIC_DIR, { recursive: true });
  });

  after(() => {
    if (fs.existsSync(ATOMIC_DIR)) {
      fs.rmSync(ATOMIC_DIR, { recursive: true, force: true });
    }
  });

  it('should write file content atomically', async () => {
    const filePath = path.join(ATOMIC_DIR, 'test.txt');
    const result = await writeFileAtomic(filePath, 'Hello, Atomic World!');
    assert.equal(result.conflict, false);
    assert.equal(typeof result.mtime, 'number');
    assert.ok(result.mtime > 0);

    // Verify content was written
    const content = fs.readFileSync(filePath, 'utf-8');
    assert.equal(content, 'Hello, Atomic World!');
  });

  it('should detect mtime conflict when expectedMtime does not match', async () => {
    const filePath = path.join(ATOMIC_DIR, 'conflict-test.txt');
    // Create file first
    await writeFileAtomic(filePath, 'original content');
    const stat = fs.statSync(filePath);

    // Write with wrong mtime
    const wrongMtime = stat.mtimeMs - 1000;
    const result = await writeFileAtomic(filePath, 'new content', wrongMtime);
    assert.equal(result.conflict, true);
  });

  it('should write successfully when expectedMtime matches', async () => {
    const filePath = path.join(ATOMIC_DIR, 'match-test.txt');
    const first = await writeFileAtomic(filePath, 'first write');
    assert.equal(first.conflict, false);

    // Now write with the correct mtime
    const result = await writeFileAtomic(filePath, 'second write', first.mtime);
    assert.equal(result.conflict, false);
    const content = fs.readFileSync(filePath, 'utf-8');
    assert.equal(content, 'second write');
  });

  it('should handle new file with expectedMtime (file does not exist)', async () => {
    const filePath = path.join(ATOMIC_DIR, 'new-file.txt');
    const result = await writeFileAtomic(filePath, 'new content', 12345);
    // File doesn't exist, so no conflict
    assert.equal(result.conflict, false);
    const content = fs.readFileSync(filePath, 'utf-8');
    assert.equal(content, 'new content');
  });
});

const CREATE_DIR = path.join(process.env.HOME || '~', '.natives-test', 'create-test');

describe('createEntry', () => {
  before(() => {
    fs.mkdirSync(CREATE_DIR, { recursive: true });
  });

  after(() => {
    if (fs.existsSync(CREATE_DIR)) {
      fs.rmSync(CREATE_DIR, { recursive: true, force: true });
    }
  });

  it('should create a file', async () => {
    const filePath = path.join(CREATE_DIR, 'new-file.txt');
    await createEntry(filePath, 'file');
    assert.ok(fs.existsSync(filePath));
    assert.ok(fs.statSync(filePath).isFile());
  });

  it('should create a directory', async () => {
    const dirPath = path.join(CREATE_DIR, 'new-dir');
    await createEntry(dirPath, 'dir');
    assert.ok(fs.existsSync(dirPath));
    assert.ok(fs.statSync(dirPath).isDirectory());
  });

  it('should reject null byte in path', async () => {
    await assert.rejects(
      () => createEntry(path.join(CREATE_DIR, 'bad\0file.txt'), 'file'),
      { code: 'EINVAL' },
    );
  });

  it('should reject creating file in non-existent directory', async () => {
    await assert.rejects(
      () => createEntry(path.join(CREATE_DIR, 'missing', 'file.txt'), 'file'),
    );
  });

  it('should reject creating directory that already exists', async () => {
    const dirPath = path.join(CREATE_DIR, 'exists-dir');
    await createEntry(dirPath, 'dir');
    await assert.rejects(
      () => createEntry(dirPath, 'dir'),
    );
  });
});

const RENAME_DIR = path.join(process.env.HOME || '~', '.natives-test', 'rename-test');

describe('renameEntry', () => {
  before(() => {
    fs.mkdirSync(RENAME_DIR, { recursive: true });
    fs.writeFileSync(path.join(RENAME_DIR, 'source.txt'), 'content', 'utf-8');
    fs.writeFileSync(path.join(RENAME_DIR, 'safe.txt'), 'safe', 'utf-8');
    fs.mkdirSync(path.join(RENAME_DIR, 'source-dir'));
  });

  after(() => {
    if (fs.existsSync(RENAME_DIR)) {
      fs.rmSync(RENAME_DIR, { recursive: true, force: true });
    }
  });

  it('should rename a file', async () => {
    const oldPath = path.join(RENAME_DIR, 'source.txt');
    const newPath = path.join(RENAME_DIR, 'dest.txt');
    await renameEntry(oldPath, newPath);
    assert.equal(fs.existsSync(oldPath), false);
    assert.ok(fs.existsSync(newPath));
  });

  it('should rename a directory', async () => {
    const oldDir = path.join(RENAME_DIR, 'source-dir');
    const newDir = path.join(RENAME_DIR, 'dest-dir');
    await renameEntry(oldDir, newDir);
    assert.equal(fs.existsSync(oldDir), false);
    assert.ok(fs.existsSync(newDir));
  });

  it('should auto-increment when target exists', async () => {
    // Recreate source file
    fs.writeFileSync(path.join(RENAME_DIR, 'dup-source.txt'), 'original', 'utf-8');
    fs.writeFileSync(path.join(RENAME_DIR, 'dup-target.txt'), 'existing', 'utf-8');

    await renameEntry(
      path.join(RENAME_DIR, 'dup-source.txt'),
      path.join(RENAME_DIR, 'dup-target.txt'),
    );

    // Source should be gone (renamed)
    assert.equal(fs.existsSync(path.join(RENAME_DIR, 'dup-source.txt')), false);
    // Original target still exists (not overwritten)
    assert.ok(fs.existsSync(path.join(RENAME_DIR, 'dup-target.txt')));
    // Renamed file should have been auto-incremented
    assert.ok(fs.existsSync(path.join(RENAME_DIR, 'dup-target (1).txt')));
  });

  it('should reject rename of non-existent file', async () => {
    await assert.rejects(
      () => renameEntry(
        path.join(RENAME_DIR, 'nonexistent.txt'),
        path.join(RENAME_DIR, 'any.txt'),
      ),
    );
  });

  it('should reject null byte in new path', async () => {
    await assert.rejects(
      () => renameEntry(
        path.join(RENAME_DIR, 'safe.txt'),
        path.join(RENAME_DIR, 'bad\0file.txt'),
      ),
      { code: 'EINVAL' },
    );
  });
});
