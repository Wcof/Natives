import assert from 'node:assert/strict';
import test from 'node:test';
import type { PreviewContext, PreviewRequest } from '../contracts';
import { PreviewProviderError } from '../errors';
import { codeProvider, CODE_MAX_LINES, sliceCodeBounded } from '../providers/code';
import {
  jsonProvider,
  JSON_NODE_BUDGET,
  JSON_MAX_PARSE_BYTES,
  countJsonNodes,
  parseJsonBounded,
  closeJsonPrefix,
  sliceUtf8,
  utf8ByteLength,
} from '../providers/json';

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

// ── P2-02：JSON 字节预算（主线程不 parse 无限 JSON；大文件 partial model）──

test('utf8ByteLength approximates utf-8 byte size', () => {
  assert.equal(utf8ByteLength('abc'), 3);
  assert.equal(utf8ByteLength('你好'), 6);
  assert.equal(utf8ByteLength('a😀b'), 6); // 代理对计 4 字节
});

test('sliceUtf8 cuts at byte boundary without splitting surrogate pairs', () => {
  assert.equal(sliceUtf8('abcd', 3), 'abc');
  assert.equal(sliceUtf8('abcd', 10), 'abcd');
  assert.equal(sliceUtf8('你好', 3), '你');
  assert.equal(sliceUtf8('a😀b', 3), 'a');
  assert.equal(sliceUtf8('a😀b', 5), 'a😀');
});

test('closeJsonPrefix closes arrays cut mid-way', () => {
  assert.equal(closeJsonPrefix('[1,2,3,'), '[1,2,3]');
  assert.equal(closeJsonPrefix('[1,2,'), '[1,2]');
  assert.equal(closeJsonPrefix('[true,false'), '[true,false]');
});

test('closeJsonPrefix closes objects cut at value/key boundaries', () => {
  assert.equal(closeJsonPrefix('{"a":1,"b":'), '{"a":1}');
  assert.equal(closeJsonPrefix('{"a":1,"b'), '{"a":1}');
  assert.equal(closeJsonPrefix('{"a":"hi"'), '{"a":"hi"}');
  assert.equal(closeJsonPrefix('{"a":[1,2'), '{"a":[1,2]}');
});

test('closeJsonPrefix handles nesting and escaped quotes', () => {
  assert.equal(closeJsonPrefix('[1,[2,[3]]'), '[1,[2,[3]]]');
  assert.equal(closeJsonPrefix('{"a":"x\\"y"'), '{"a":"x\\"y"}');
  assert.deepEqual(JSON.parse(closeJsonPrefix('{"a":"x\\"y"')!), { a: 'x"y' });
});

test('closeJsonPrefix returns null for uncloseable prefixes', () => {
  assert.equal(closeJsonPrefix('"unfinished'), null);
  assert.equal(closeJsonPrefix(''), null);
  assert.equal(closeJsonPrefix('{"a":'), null);
});

test('parseJsonBounded parses small JSON fully without truncation', () => {
  const { value, truncated } = parseJsonBounded('{"a":1}');
  assert.equal(truncated, false);
  assert.deepEqual(value, { a: 1 });
});

test('parseJsonBounded caps over-budget JSON to a bounded partial model', () => {
  const big = '[' + Array.from({ length: 200_000 }, (_, i) => i).join(',') + ']';
  assert.ok(utf8ByteLength(big) > JSON_MAX_PARSE_BYTES);
  const { value, truncated } = parseJsonBounded(big);
  assert.equal(truncated, true);
  assert.ok(Array.isArray(value));
  // 主线程解析的输入绝不超过字节预算
  assert.ok(utf8ByteLength(JSON.stringify(value)) <= JSON_MAX_PARSE_BYTES * 2);
});

test('over-budget JSON whose prefix cannot close falls back recoverable parse_failed', () => {
  const big = '"' + 'x'.repeat(JSON_MAX_PARSE_BYTES + 1000) + '"';
  assert.throws(
    () => parseJsonBounded(big),
    (e: unknown) => e instanceof PreviewProviderError && e.code === 'parse_failed' && e.recoverable === true,
  );
});

test('json provider marks truncated when source exceeds byte budget', async () => {
  const big = '[' + Array.from({ length: 200_000 }, (_, i) => i).join(',') + ']';
  const ctx = makeContext({ readText: async () => ({ content: big, truncated: false, size: big.length, mtime: 1, kind: 'text', encoding: 'utf-8' }) });
  const model = await jsonProvider.prepare(fileReq('/huge.json'), ctx);
  assert.equal(model.kind, 'json');
  if (model.kind === 'json') assert.equal(model.truncated, true);
});

// ── P2-02：Code provider 行/字节预算（read 阶段有界，不把完整文件塞进模型）──

test('sliceCodeBounded enforces line budget', () => {
  const text = Array.from({ length: CODE_MAX_LINES + 100 }, (_, i) => `line ${i}`).join('\n');
  const { source, truncated } = sliceCodeBounded(text);
  assert.equal(truncated, true);
  assert.equal(source.split('\n').length, CODE_MAX_LINES);
});

test('sliceCodeBounded enforces char budget (memory sources have no host truncation)', () => {
  const text = 'x'.repeat(300 * 1024); // 300KB 无换行
  const { source, truncated } = sliceCodeBounded(text);
  assert.equal(truncated, true);
  assert.ok(source.length < 300 * 1024);
});

test('code provider truncates oversized files and flags truncated', async () => {
  const big = Array.from({ length: CODE_MAX_LINES + 50 }, (_, i) => `line ${i}`).join('\n');
  const ctx = makeContext({ readText: async () => ({ content: big, truncated: false, size: big.length, mtime: 1, kind: 'text', encoding: 'utf-8' }) });
  const model = await codeProvider.prepare(fileReq('/big.ts'), ctx);
  assert.equal(model.kind, 'code');
  if (model.kind === 'code') {
    assert.equal(model.truncated, true);
    assert.equal(model.source.split('\n').length, CODE_MAX_LINES);
  }
});

test('code provider bounds memory sources too', async () => {
  const big = Array.from({ length: CODE_MAX_LINES + 50 }, (_, i) => `line ${i}`).join('\n');
  const ctx = makeContext();
  const model = await codeProvider.prepare({ source: { type: 'memory', name: 'big.ts', content: big }, mode: 'preview', surface: 'assistant' }, ctx);
  assert.equal(model.kind, 'code');
  if (model.kind === 'code') {
    assert.equal(model.truncated, true);
    assert.equal(model.source.split('\n').length, CODE_MAX_LINES);
  }
});
