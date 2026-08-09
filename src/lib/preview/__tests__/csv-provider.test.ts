import assert from 'node:assert/strict';
import test from 'node:test';
import type { PreviewContext, PreviewRequest } from '../contracts';
import { PreviewProviderError } from '../errors';
import {
  csvProvider,
  parseCsv,
  parseCsvLine,
  CSV_MAX_ROWS,
  CSV_MAX_COLUMNS,
  CSV_MAX_PARSE_BYTES,
} from '../providers/csv';

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

// ── P2-02：CSV 字节/列预算（read/parse 阶段有界，不先解析完整文件再 slice）──

test('parseCsv bounds columns beyond CSV_MAX_COLUMNS', () => {
  const wide = Array.from({ length: CSV_MAX_COLUMNS + 50 }, (_, i) => `c${i}`).join(',') + '\n';
  const parsed = parseCsv(wide);
  assert.equal(parsed.truncated, true);
  assert.ok(parsed.headers.length <= CSV_MAX_COLUMNS);
});

test('parseCsv stops parsing when input exceeds byte budget', () => {
  // 单行巨大（无换行），超过字节预算：不解析完整输入，行/列仍受列预算保护
  const hugeRow = Array.from({ length: Math.ceil(CSV_MAX_PARSE_BYTES / 2) }, (_, i) => `v${i}`).join(',');
  const parsed = parseCsv('h1,h2\n' + hugeRow);
  assert.equal(parsed.truncated, true);
  assert.ok(parsed.rows.length <= 1);
});

test('parseCsv byte budget truncates without swallowing already-parsed rows', () => {
  const bigBody = 'x,y\n' + ('1,2\n'.repeat(Math.ceil(CSV_MAX_PARSE_BYTES / 4) + 10));
  const parsed = parseCsv(bigBody);
  assert.equal(parsed.truncated, true);
  assert.deepEqual(parsed.headers, ['x', 'y']);
  assert.ok(parsed.rows.length > 0 && parsed.rows.length <= CSV_MAX_ROWS);
});

test('csv provider surfaces byte-budget truncation on oversized memory content', async () => {
  const bigContent = 'h1,h2\n' + ('a,b\n'.repeat(Math.ceil(CSV_MAX_PARSE_BYTES / 4) + 10));
  const ctx: PreviewContext = {
    authorizeFile: async (path) => ({ path, name: 'a.csv', kind: 'text', size: 0, mtime: 1 }),
    readText: async () => ({ content: '', truncated: false, size: 0, mtime: 1, kind: 'text', encoding: 'utf-8' }),
    toAssetUrl: () => '',
    prepareHtml: async () => ({ content: '', fsBase: '', serverPort: 0 }),
    listArchive: async () => ({ entries: [], truncated: false, totalSize: 0 }),
  };
  const model = await csvProvider.prepare({ source: { type: 'memory', name: 'big.csv', content: bigContent }, mode: 'preview', surface: 'assistant' }, ctx);
  assert.equal(model.kind, 'csv');
  if (model.kind === 'csv') assert.equal(model.truncated, true);
});
