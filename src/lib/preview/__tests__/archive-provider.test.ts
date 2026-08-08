import assert from 'node:assert/strict';
import test from 'node:test';
import type { PreviewContext, PreviewRequest } from '../contracts';
import { PreviewProviderError } from '../errors';
import { archiveProvider } from '../providers/archive';

function ctxFor(listEntries: Array<{ name: string; size: number; isDir: boolean }>): PreviewContext {
  let authorized = false;
  return {
    authorizeFile: async (path) => {
      authorized = true;
      return { path, name: 'a.zip', kind: 'archive', size: 100, mtime: 1 };
    },
    readText: async () => ({ content: '', truncated: false, size: 0, mtime: 1, kind: 'text', encoding: 'utf-8' }),
    toAssetUrl: () => '',
    prepareHtml: async () => ({ content: '', fsBase: '', serverPort: 0 }),
    listArchive: async () => ({ entries: listEntries, truncated: listEntries.length > 1000, totalSize: 100 }),
    __authorized: () => authorized,
  } as PreviewContext & { __authorized(): boolean };
}

test('archive provider returns typed model after authorize + listArchive', async () => {
  const ctx = ctxFor([{ name: 'a.txt', size: 5, isDir: false }]);
  const req: PreviewRequest = { source: { type: 'file', path: '/a.zip', kind: 'archive' }, mode: 'preview', surface: 'files' };
  const model = await archiveProvider.prepare(req, ctx);
  assert.equal(model.kind, 'archive');
  if (model.kind === 'archive') {
    assert.equal(model.entries.length, 1);
    assert.equal(model.entries[0]?.name, 'a.txt');
    assert.equal(model.truncated, false);
  }
  assert.equal((ctx as unknown as { __authorized(): boolean }).__authorized(), true);
});

test('memory source is not accepted', () => {
  assert.equal(
    archiveProvider.accepts({ source: { type: 'memory', name: 'x.zip', content: '' }, mode: 'preview', surface: 'assistant' }),
    false,
  );
});

test('accepts by extension or kind', () => {
  assert.equal(archiveProvider.accepts({ source: { type: 'file', path: '/a.tar.gz', kind: 'text' }, mode: 'preview', surface: 'files' }), true);
  assert.equal(archiveProvider.accepts({ source: { type: 'file', path: '/a.zip', kind: 'archive' }, mode: 'preview', surface: 'files' }), true);
  assert.equal(archiveProvider.accepts({ source: { type: 'file', path: '/a.txt', kind: 'text' }, mode: 'preview', surface: 'files' }), false);
});

test('fatal permission error propagates without fallback', async () => {
  const ctx = {
    authorizeFile: async () => {
      throw new PreviewProviderError('permission_denied', 'blocked');
    },
    readText: async () => ({ content: '', truncated: false, size: 0, mtime: 1, kind: 'text', encoding: 'utf-8' }),
    toAssetUrl: () => '',
    prepareHtml: async () => ({ content: '', fsBase: '', serverPort: 0 }),
    listArchive: async () => ({ entries: [], truncated: false, totalSize: 0 }),
  } as PreviewContext;
  await assert.rejects(
    () => archiveProvider.prepare({ source: { type: 'file', path: '/secret/a.zip', kind: 'archive' }, mode: 'preview', surface: 'files' }, ctx),
    (e: unknown) => e instanceof PreviewProviderError && e.code === 'permission_denied' && e.recoverable === false,
  );
});

test('truncated listing is surfaced', async () => {
  const many = Array.from({ length: 1200 }, (_, i) => ({ name: `f${i}`, size: 1, isDir: false }));
  const ctx = ctxFor(many);
  const model = await archiveProvider.prepare({ source: { type: 'file', path: '/a.zip', kind: 'archive' }, mode: 'preview', surface: 'files' }, ctx);
  assert.equal(model.kind, 'archive');
  if (model.kind === 'archive') assert.equal(model.truncated, true);
});
