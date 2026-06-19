import { describe, it, before, after } from 'node:test';
import assert from 'node:assert/strict';
import * as fs from 'fs';
import * as path from 'path';

const E2E_DIR = path.join(process.env.HOME || '~', '.natives-test', 'e2e-flow');

describe('E2E: File Browse → Preview → Search → Operations', () => {
  before(() => {
    fs.mkdirSync(E2E_DIR, { recursive: true });
    fs.writeFileSync(path.join(E2E_DIR, 'hello.txt'), 'Hello World\nThis is a test\nLine 3', 'utf-8');
    fs.writeFileSync(path.join(E2E_DIR, 'typescript.ts'), 'const x: number = 42;\nconsole.log(x);', 'utf-8');
    fs.mkdirSync(path.join(E2E_DIR, 'subdir'));
    fs.writeFileSync(path.join(E2E_DIR, 'subdir', 'nested.md'), '# Nested\n\nContent here', 'utf-8');
  });

  after(() => {
    if (fs.existsSync(E2E_DIR)) fs.rmSync(E2E_DIR, { recursive: true, force: true });
  });

  it('1. listDir should return all entries', async () => {
    const { listDir } = await import('./file-manager');
    const entries = await listDir(E2E_DIR);
    assert.ok(entries.length >= 3);
    const names = entries.map((e) => e.name);
    assert.ok(names.includes('hello.txt'));
    assert.ok(names.includes('typescript.ts'));
    assert.ok(names.includes('subdir'));
  });

  it('2. readFile should read file content', async () => {
    const { readFile } = await import('./file-manager');
    const result = await readFile(path.join(E2E_DIR, 'hello.txt'));
    assert.ok(result.content.includes('Hello World'));
    assert.equal(result.truncated, false);
  });

  it('3. streamFile should stream file', async () => {
    const { streamFile } = await import('./file-manager');
    const result = await streamFile(path.join(E2E_DIR, 'hello.txt'));
    assert.equal(result.contentType, 'text/plain');
    const chunks: Buffer[] = [];
    for await (const chunk of result.stream) chunks.push(chunk as Buffer);
    const content = Buffer.concat(chunks).toString('utf-8');
    assert.ok(content.includes('Hello World'));
  });

  it('4. grepContent should find matching lines', async () => {
    const { grepContent } = await import('../lib/search-engine');
    const results = await grepContent('Hello', E2E_DIR);
    assert.ok(results.length >= 1);
    assert.ok(results.some((r) => r.line === 1 && r.preview.includes('Hello')));
  });

  it('5. searchFiles should find by filename', async () => {
    const { searchFiles } = await import('../lib/search-engine');
    const results = await searchFiles('typescript', E2E_DIR);
    assert.ok(results.length >= 1);
    assert.equal(results[0]!.name, 'typescript.ts');
  });

  it('6. getGitStatus should detect non-git dir', async () => {
    const { getGitStatus } = await import('./git');
    const result = await getGitStatus(E2E_DIR);
    assert.equal(result, null);
  });

  it('7. getDiskUsage should return sizes', async () => {
    const { getDiskUsage } = await import('./disk-usage');
    const result = await getDiskUsage(E2E_DIR);
    assert.ok(result.length >= 3);
    // Should be sorted by size descending
    for (let i = 1; i < result.length; i++) {
      assert.ok(result[i - 1]!.size >= result[i]!.size);
    }
  });

  it('8. writeFileAtomic + readFile roundtrip', async () => {
    const { writeFileAtomic, readFile } = await import('./file-manager');
    const filePath = path.join(E2E_DIR, 'roundtrip.txt');
    const writeResult = await writeFileAtomic(filePath, 'roundtrip content');
    assert.equal(writeResult.conflict, false);

    const readResult = await readFile(filePath);
    assert.equal(readResult.content, 'roundtrip content');
  });

  it('9. createEntry + renameEntry + trashEntry flow', async () => {
    const { createEntry, renameEntry, trashEntry } = await import('./file-manager');
    const filePath = path.join(E2E_DIR, 'flow-test.txt');
    const renamedPath = path.join(E2E_DIR, 'flow-renamed.txt');

    await createEntry(filePath, 'file');
    assert.ok(fs.existsSync(filePath));

    await renameEntry(filePath, renamedPath);
    assert.equal(fs.existsSync(filePath), false);
    assert.ok(fs.existsSync(renamedPath));

    await trashEntry(renamedPath);
    assert.equal(fs.existsSync(renamedPath), false);
  });

  it('10. importFiles copies files with auto-increment', async () => {
    const { importFiles } = await import('./file-manager');
    const src = path.join(E2E_DIR, 'hello.txt');
    const result = await importFiles([src], E2E_DIR);
    // hello.txt exists, so should create hello (1).txt
    assert.ok(result.some((p) => p.includes('(1)')));
  });
});
