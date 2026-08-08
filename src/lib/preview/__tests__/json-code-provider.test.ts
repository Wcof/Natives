import assert from 'node:assert/strict';
import test from 'node:test';
import type { PreviewContext, PreviewRequest } from '../contracts';
import { PreviewProviderError } from '../errors';
import { codeProvider } from '../providers/code';
import { jsonProvider, JSON_NODE_BUDGET, countJsonNodes } from '../providers/json';

function makeContext(overrides: Partial<PreviewContext> = {}): PreviewContext & {
  __authorizeCalls(): number;
  __readCalls(): number;
} {
  let authorizeCalls = 0;
  let readCalls = 0;
  const ctx: PreviewContext = {
    authorizeFile: async (path) => {
      authorizeCalls++;
      return { path, name: path.split('/').pop() ?? path, kind: 'text', size: 10, mtime: 1 };
    },
    readText: async () => {
      readCalls++;
      return { content: '{"a":1}', truncated: false, size: 8, mtime: 1, kind: 'text', encoding: 'utf-8' };
    },
    toAssetUrl: () => 'asset://localhost/x',
    prepareHtml: async () => ({ content: '', fsBase: '', serverPort: 0 }),
    listArchive: async () => ({ entries: [], truncated: false, totalSize: 0 }),
    ...overrides,
  };
  return Object.assign(ctx, {
    __authorizeCalls: () => authorizeCalls,
    __readCalls: () => readCalls,
  }) as PreviewContext & { __authorizeCalls(): number; __readCalls(): number };
}

function fileReq(path: string, content = '{"a":1}'): PreviewRequest {
  return { source: { type: 'file', path, kind: 'text' }, mode: 'preview', surface: 'files' };
}

test('invalid JSON falls back as recoverable parse_failed', async () => {
  const ctx = makeContext({ readText: async () => ({ content: '{not json', truncated: false, size: 9, mtime: 1, kind: 'text', encoding: 'utf-8' }) });
  await assert.rejects(
    () => jsonProvider.prepare(fileReq('/a.json'), ctx),
    (e: unknown) => e instanceof PreviewProviderError && e.code === 'parse_failed' && e.recoverable === true,
  );
});

test('code provider catches the same file (fallback target)', async () => {
  const ctx = makeContext({ readText: async () => ({ content: '{not json', truncated: false, size: 9, mtime: 1, kind: 'text', encoding: 'utf-8' }) });
  const model = await codeProvider.prepare(fileReq('/a.json'), ctx);
  assert.equal(model.kind, 'code');
});

test('valid JSON returns typed model with nodeCount', async () => {
  const ctx = makeContext();
  const model = await jsonProvider.prepare(fileReq('/a.json'), ctx);
  assert.equal(model.kind, 'json');
  if (model.kind === 'json') {
    assert.deepEqual(model.value, { a: 1 });
    assert.equal(model.nodeCount, 2);
    assert.equal(model.truncated, false);
  }
});

test('file source must be authorized before readText (json)', async () => {
  const ctx = makeContext();
  await jsonProvider.prepare(fileReq('/a.json'), ctx);
  assert.equal(ctx.__authorizeCalls(), 1);
  assert.equal(ctx.__readCalls(), 1);
});

test('fatal io error does NOT fall back to code', async () => {
  const ctx = makeContext({
    authorizeFile: async () => {
      throw new PreviewProviderError('permission_denied', 'blocked');
    },
  });
  await assert.rejects(
    () => jsonProvider.prepare(fileReq('/secret/a.json'), ctx),
    (e: unknown) => e instanceof PreviewProviderError && e.code === 'permission_denied' && e.recoverable === false,
  );
});

test('node budget bounds huge JSON as truncated', () => {
  const big = { list: Array.from({ length: JSON_NODE_BUDGET + 10 }, (_, i) => i) };
  const { nodeCount, truncated } = countJsonNodes(big);
  assert.equal(truncated, true);
  assert.ok(nodeCount > JSON_NODE_BUDGET);
});

test('depth budget bounds deep JSON', () => {
  let deep: unknown = 1;
  for (let i = 0; i < 1000; i++) deep = { next: deep };
  const { truncated } = countJsonNodes(deep);
  assert.equal(truncated, true);
});

test('memory source works without authorization', async () => {
  const ctx = makeContext();
  const model = await jsonProvider.prepare({ source: { type: 'memory', name: 'x.json', content: '{"b":2}' }, mode: 'preview', surface: 'assistant' }, ctx);
  assert.equal(model.kind, 'json');
  assert.equal(ctx.__authorizeCalls(), 0);
});
