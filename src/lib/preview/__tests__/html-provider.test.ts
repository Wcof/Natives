// HTML Preview provider 测试（PREV-001：垂直链）
//
// file source 必须先 authorizeFile 再 prepareHtml；fatal 原样透传（不降级）。
// memory source 不获得 FS 能力。sandbox 红线：allow-scripts allow-forms，
// 无 allow-same-origin / allow-top-navigation / allow-popups。

import assert from 'node:assert/strict';
import test from 'node:test';
import type { PreviewContext, PreviewRequest } from '../contracts';
import { fatalError } from '../errors';
import { HTML_PREVIEW_SANDBOX, htmlProvider } from '../providers/html';

function makeContext(overrides: Partial<PreviewContext> = {}): PreviewContext {
  let authorizeCalls = 0;
  let prepareCalls = 0;
  const ctx: PreviewContext = {
    authorizeFile: async (path) => {
      authorizeCalls++;
      return { path, name: path.split('/').pop() ?? path, kind: 'text', size: 10, mtime: 1 };
    },
    readText: async () => ({ content: '', truncated: false, size: 0, mtime: 1, kind: 'text', encoding: 'utf-8' }),
    toAssetUrl: () => 'asset://localhost/x',
    prepareHtml: async (file) => {
      prepareCalls++;
      return {
        content: `<img src="http://localhost:4321/fs/tok/${encodeURI(file.path.split('/').pop() ?? '')}">`,
        fsBase: '/',
        serverPort: 4321,
      };
    },
    listArchive: async () => ({ entries: [], truncated: false, totalSize: 0 }),
    ...overrides,
  };
  return Object.assign(ctx, {
    __authorizeCalls: () => authorizeCalls,
    __prepareCalls: () => prepareCalls,
  }) as PreviewContext & {
    __authorizeCalls(): number;
    __prepareCalls(): number;
  };
}

function fileReq(path: string, kind: 'text' | 'image' = 'text'): PreviewRequest {
  return { source: { type: 'file', path, kind }, mode: 'preview', surface: 'files' };
}

function memoryReq(name: string): PreviewRequest {
  return { source: { type: 'memory', name, content: '<div>mem</div>' }, mode: 'preview', surface: 'assistant' };
}

test('accepts .html / .htm text sources and rejects non-text kinds', () => {
  assert.equal(htmlProvider.accepts(fileReq('/docs/a.html')), true);
  assert.equal(htmlProvider.accepts(fileReq('/docs/a.htm')), true);
  assert.equal(htmlProvider.accepts(fileReq('/docs/a.html', 'image')), false);
  assert.equal(htmlProvider.accepts(fileReq('/docs/a.md')), false);
  assert.equal(htmlProvider.accepts(memoryReq('a.html')), true);
});

test('file source must be authorized then prepared (PREV-001 vertical chain)', async () => {
  const ctx = makeContext();
  const model = await htmlProvider.prepare(fileReq('/docs/a.html'), ctx);
  assert.equal(model.kind, 'html');
  const c = ctx as unknown as { __authorizeCalls(): number; __prepareCalls(): number };
  assert.equal(c.__authorizeCalls(), 1);
  assert.equal(c.__prepareCalls(), 1);
  if (model.kind === 'html') {
    assert.equal(model.sandbox, HTML_PREVIEW_SANDBOX);
    // sandbox 红线（R-S2）：允许脚本+表单；禁止同源提升/顶层导航/popup
    assert.ok(!model.sandbox.includes('allow-same-origin'));
    assert.ok(!model.sandbox.includes('allow-top-navigation'));
    assert.ok(!model.sandbox.includes('allow-popups'));
    // Host 已把本地引用改写为 /fs/{token}/；renderer 以 srcDoc 呈现即可
    assert.ok(model.html?.includes('http://localhost:4321/fs/tok/'));
    assert.ok(model.revision.includes('/docs/a.html'));
  }
});

test('memory source renders sandboxed html without any FS capability', async () => {
  const ctx = makeContext();
  const model = await htmlProvider.prepare(memoryReq('a.html'), ctx);
  assert.equal(model.kind, 'html');
  const c = ctx as unknown as { __authorizeCalls(): number; __prepareCalls(): number };
  assert.equal(c.__authorizeCalls(), 0);
  assert.equal(c.__prepareCalls(), 0);
  if (model.kind === 'html') {
    assert.equal(model.html, '<div>mem</div>');
    assert.equal(model.sandbox, HTML_PREVIEW_SANDBOX);
  }
});

test('authorize failure propagates as fatal without fallback', async () => {
  const ctx = makeContext({
    authorizeFile: async () => {
      throw fatalError('permission_denied', 'blocked');
    },
  });
  await assert.rejects(
    () => htmlProvider.prepare(fileReq('/secret/a.html'), ctx),
    (e: unknown) => e instanceof Error && e.message === 'blocked',
  );
});

test('prepareHtml host failure propagates as fatal', async () => {
  const ctx = makeContext({
    prepareHtml: async () => {
      throw fatalError('host_error', 'html prepare failed');
    },
  });
  await assert.rejects(
    () => htmlProvider.prepare(fileReq('/docs/a.html'), ctx),
    (e: unknown) => e instanceof Error && e.message === 'html prepare failed',
  );
});
