import assert from 'node:assert/strict';
import test from 'node:test';
import type { PreviewContext, PreviewRequest } from '../contracts';
import { PreviewProviderError } from '../errors';
import { csvProvider, parseCsv, parseCsvLine, CSV_MAX_ROWS } from '../providers/csv';

test('parseCsvLine handles quotes, commas and escaped quotes', () => {
  assert.deepEqual(parseCsvLine('a,"b,c",d'), ['a', 'b,c', 'd']);
  assert.deepEqual(parseCsvLine('"he said ""hi"""'), ['he said "hi"']);
  assert.deepEqual(parseCsvLine('1,2,3'), ['1', '2', '3']);
});

test('parseCsv supports quoted newlines (R11)', () => {
  const csv = 'h1,h2\n"multi\nline",v2\n';
  const parsed = parseCsv(csv);
  assert.equal(parsed.truncated, false);
  assert.deepEqual(parsed.headers, ['h1', 'h2']);
  assert.deepEqual(parsed.rows, [['multi\nline', 'v2']]);
});

test('parseCsv bounds rows (R-P4)', () => {
  const big = Array.from({ length: CSV_MAX_ROWS + 100 }, (_, i) => `r${i},x`).join('\n');
  const parsed = parseCsv(big);
  assert.equal(parsed.truncated, true);
  assert.ok(parsed.rows.length <= CSV_MAX_ROWS);
});

test('provider returns typed csv model for file source', async () => {
  const ctx: PreviewContext = {
    authorizeFile: async (path) => ({ path, name: 'a.csv', kind: 'text', size: 10, mtime: 1 }),
    readText: async () => ({ content: 'a,b\n1,2', truncated: false, size: 7, mtime: 1, kind: 'text', encoding: 'utf-8' }),
    toAssetUrl: () => 'asset://localhost/x',
    prepareHtml: async () => ({ content: '', fsBase: '', serverPort: 0 }),
    listArchive: async () => ({ entries: [], truncated: false, totalSize: 0 }),
  };
  const req: PreviewRequest = { source: { type: 'file', path: '/a.csv', kind: 'text' }, mode: 'preview', surface: 'files' };
  const model = await csvProvider.prepare(req, ctx);
  assert.equal(model.kind, 'csv');
  if (model.kind === 'csv') {
    assert.deepEqual(model.headers, ['a', 'b']);
    assert.deepEqual(model.rows, [['1', '2']]);
  }
});

test('file source must be authorized before readText', async () => {
  let authorized = false;
  const ctx: PreviewContext = {
    authorizeFile: async (path) => {
      authorized = true;
      return { path, name: 'a.csv', kind: 'text', size: 10, mtime: 1 };
    },
    readText: async () => ({ content: 'a,b\n1,2', truncated: false, size: 7, mtime: 1, kind: 'text', encoding: 'utf-8' }),
    toAssetUrl: () => 'asset://localhost/x',
    prepareHtml: async () => ({ content: '', fsBase: '', serverPort: 0 }),
    listArchive: async () => ({ entries: [], truncated: false, totalSize: 0 }),
  };
  await csvProvider.prepare({ source: { type: 'file', path: '/a.csv', kind: 'text' }, mode: 'preview', surface: 'files' }, ctx);
  assert.equal(authorized, true);
});

test('fatal io error propagates without fallback', async () => {
  const ctx: PreviewContext = {
    authorizeFile: async () => {
      throw new PreviewProviderError('permission_denied', 'blocked');
    },
    readText: async () => ({ content: '', truncated: false, size: 0, mtime: 1, kind: 'text', encoding: 'utf-8' }),
    toAssetUrl: () => '',
    prepareHtml: async () => ({ content: '', fsBase: '', serverPort: 0 }),
    listArchive: async () => ({ entries: [], truncated: false, totalSize: 0 }),
  };
  await assert.rejects(
    () => csvProvider.prepare({ source: { type: 'file', path: '/secret/a.csv', kind: 'text' }, mode: 'preview', surface: 'files' }, ctx),
    (e: unknown) => e instanceof PreviewProviderError && e.code === 'permission_denied' && e.recoverable === false,
  );
});

test('empty csv is recoverable unsupported (allows code fallback)', async () => {
  const ctx: PreviewContext = {
    authorizeFile: async (path) => ({ path, name: 'a.csv', kind: 'text', size: 0, mtime: 1 }),
    readText: async () => ({ content: '', truncated: false, size: 0, mtime: 1, kind: 'text', encoding: 'utf-8' }),
    toAssetUrl: () => '',
    prepareHtml: async () => ({ content: '', fsBase: '', serverPort: 0 }),
    listArchive: async () => ({ entries: [], truncated: false, totalSize: 0 }),
  };
  await assert.rejects(
    () => csvProvider.prepare({ source: { type: 'file', path: '/a.csv', kind: 'text' }, mode: 'preview', surface: 'files' }, ctx),
    (e: unknown) => e instanceof PreviewProviderError && e.recoverable === true,
  );
});
