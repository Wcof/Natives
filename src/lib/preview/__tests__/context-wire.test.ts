// PreviewContext Host-wire 边界测试（PREV-005/PREV-006）
//
// PREV-005：Host wire 数据在边界处经 zod schema 校验，畸形数据 → 结构化
// host_error，不裸 as cast 穿透进 provider/renderer。
// PREV-006：fatal 错误 message 一律不含本地绝对路径；经错误分类器后
// 用户可见文案同样不含路径。

import assert from 'node:assert/strict';
import test from 'node:test';
import { classifyError } from '@/lib/error-classifier';
import type { AuthorizedPreviewFile } from '../contracts';
import { createPreviewContext, type PreviewArchiveShape, type PreviewHost, type PreviewStatShape } from '../context';
import { PreviewProviderError } from '../errors';

function makeHost(overrides: Partial<PreviewHost> = {}): PreviewHost {
  return {
    stat: async (path) => ({ found: true, path, name: 'f.txt', kind: 'text', size: 10, mtime: 1 }),
    readFile: async () => ({ content: 'x', truncated: false, size: 1, mtime: 1, kind: 'text', encoding: 'utf-8' }),
    listArchive: async () => ({ entries: [], truncated: false }),
    toAssetUrl: (path) => `asset://localhost${encodeURI(path)}`,
    prepareHtml: async () => ({ content: '<html/>', fsBase: '/', mtime: 0, serverPort: 0 }),
    ...overrides,
  };
}

function file(path: string): AuthorizedPreviewFile {
  return { path, name: 'f', kind: 'text', size: 1, mtime: 1 };
}

async function capture(p: Promise<unknown>): Promise<PreviewProviderError> {
  const result = await p.then(
    () => null,
    (e: unknown) => e,
  );
  assert.ok(result instanceof PreviewProviderError, `expected PreviewProviderError, got ${String(result)}`);
  return result as PreviewProviderError;
}

// ── PREV-005: zod 边界校验 ──

test('valid wire data flows through the validated boundary', async () => {
  const ctx = createPreviewContext(makeHost());
  const authorized = await ctx.authorizeFile('/a/b.txt');
  assert.equal(authorized.kind, 'text');
  assert.equal(authorized.name, 'f.txt');
  const result = await ctx.readText(authorized);
  assert.equal(result.content, 'x');
});

test('malformed readFile wire yields structured host_error (PREV-005)', async () => {
  const host = makeHost({ readFile: async () => ({ content: 42, truncated: 'yes' }) });
  const ctx = createPreviewContext(host);
  const err = await capture(ctx.readText(file('/Users/secret/a.txt')));
  assert.equal(err.code, 'host_error');
  assert.equal(err.recoverable, false);
  assert.ok(!err.message.includes('/Users/secret'));
});

test('malformed stat wire yields structured host_error (PREV-005)', async () => {
  const host = makeHost({ stat: async () => ({ found: 'yes' } as unknown as PreviewStatShape) });
  const ctx = createPreviewContext(host);
  const err = await capture(ctx.authorizeFile('/a/b.txt'));
  assert.equal(err.code, 'host_error');
});

test('malformed archive wire yields structured host_error (PREV-005)', async () => {
  const host = makeHost({ listArchive: async () => ({ entries: 'nope' } as unknown as PreviewArchiveShape) });
  const ctx = createPreviewContext(host);
  const err = await capture(ctx.listArchive(file('/a/z.zip')));
  assert.equal(err.code, 'host_error');
});

test('listArchive normalizes entries and computes totalSize', async () => {
  const host = makeHost({ listArchive: async () => ({ entries: [{ name: 'a.txt', size: 3 }], truncated: false }) });
  const ctx = createPreviewContext(host);
  const listing = await ctx.listArchive(file('/a/z.zip'));
  assert.deepEqual(listing.entries, [{ name: 'a.txt', size: 3, isDir: false }]);
  assert.equal(listing.totalSize, 3);
  assert.equal(listing.truncated, false);
});

// ── PREV-006: fatal message 与用户可见文案均不含绝对路径 ──

test('stat transport error message carries no local path (PREV-006)', async () => {
  const secretPath = '/Users/secret/notes.txt';
  const host = makeHost({
    stat: async () => { throw new Error(`ENOENT: no such file at ${secretPath}`); },
  });
  const ctx = createPreviewContext(host);
  const err = await capture(ctx.authorizeFile(secretPath));
  assert.equal(err.code, 'io_error');
  assert.ok(!err.message.includes(secretPath));
  assert.ok(!err.message.includes('secret'));
});

test('readFile transport error message carries no local path (PREV-006)', async () => {
  const secret = '/Users/secret/x.txt';
  const host = makeHost({ readFile: async () => { throw new Error(`read ${secret} failed`); } });
  const ctx = createPreviewContext(host);
  const err = await capture(ctx.readText(file(secret)));
  assert.equal(err.code, 'io_error');
  assert.ok(!err.message.includes('/Users/secret'));
});

test('not-found stat maps to path-free permission_denied (PREV-006)', async () => {
  const host = makeHost({ stat: async () => ({ found: false, path: '/Users/secret/none' }) });
  const ctx = createPreviewContext(host);
  const err = await capture(ctx.authorizeFile('/Users/secret/none'));
  assert.equal(err.code, 'permission_denied');
  assert.ok(!err.message.includes('/Users/secret'));
});

test('directory stat maps to path-free security_violation (PREV-006)', async () => {
  const host = makeHost({ stat: async () => ({ found: true, path: '/Users/secret/dir', isDir: true }) });
  const ctx = createPreviewContext(host);
  const err = await capture(ctx.authorizeFile('/Users/secret/dir'));
  assert.equal(err.code, 'security_violation');
  assert.ok(!err.message.includes('/Users/secret'));
});

test('underlying path leak never reaches classifier user-visible copy (PREV-006)', async () => {
  const secretPath = '/Users/secret/vault/key.pem';
  const host = makeHost({
    stat: async () => { throw new Error(`permission denied reading ${secretPath}`); },
  });
  const ctx = createPreviewContext(host);
  const err = await capture(ctx.authorizeFile(secretPath));
  assert.ok(!err.message.includes(secretPath));
  const classified = classifyError(err, { locale: 'en' });
  assert.ok(!classified.userMessage.includes('/Users/secret'));
  assert.ok(!classified.userMessage.includes('secret'));
  assert.ok(!classified.userMessage.includes('vault'));
});

test('classifier over preview fatal yields user-visible copy without absolute paths (PREV-006)', async () => {
  for (const message of ['stat failed', 'read failed', 'archive list failed', 'path not accessible', 'html prepare failed']) {
    const err = new PreviewProviderError('io_error', message);
    const classified = classifyError(err, { locale: 'zh' });
    assert.ok(!classified.userMessage.includes('/Users/'), `leak in ${message}: ${classified.userMessage}`);
    assert.ok(!classified.userMessage.includes('C:\\'), `leak in ${message}: ${classified.userMessage}`);
  }
});
