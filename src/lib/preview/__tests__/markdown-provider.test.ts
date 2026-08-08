import assert from 'node:assert/strict';
import test from 'node:test';
import type { PreviewContext, PreviewRequest } from '../contracts';
import { markdownProvider } from '../providers/markdown';
import {
  buildMarkdownRenderOptions,
  isAuthorizedAssetUrl,
  transformAuthorizedFileUrl,
} from '../providers/markdown-policy';

function makeContext(overrides: Partial<PreviewContext> = {}): PreviewContext {
  let authorizeCalls = 0;
  let readCalls = 0;
  const ctx: PreviewContext = {
    authorizeFile: async (path) => {
      authorizeCalls++;
      return { path, name: path.split('/').pop() ?? path, kind: 'text', size: 10, mtime: 1 };
    },
    readText: async () => {
      readCalls++;
      return { content: '# hi', truncated: false, size: 4, mtime: 1, kind: 'text', encoding: 'utf-8' };
    },
    toAssetUrl: () => 'asset://localhost/x',
    prepareHtml: async () => ({ content: '', fsBase: '', serverPort: 0 }),
    listArchive: async () => ({ entries: [], truncated: false, totalSize: 0 }),
    ...overrides,
  };
  return Object.assign(ctx, { __authorizeCalls: () => authorizeCalls, __readCalls: () => readCalls }) as PreviewContext & {
    __authorizeCalls(): number;
    __readCalls(): number;
  };
}

function fileReq(path: string, kind: 'text' | 'image' = 'text'): PreviewRequest {
  return { source: { type: 'file', path, kind }, mode: 'preview', surface: 'files' };
}

function memoryReq(name: string): PreviewRequest {
  return { source: { type: 'memory', name, content: '# mem' }, mode: 'preview', surface: 'assistant' };
}

test('file source must be authorized before readText', async () => {
  const ctx = makeContext();
  const model = await markdownProvider.prepare(fileReq('/docs/a.md'), ctx);
  assert.equal(model.kind, 'markdown');
  const c = ctx as unknown as { __authorizeCalls(): number; __readCalls(): number };
  assert.equal(c.__authorizeCalls(), 1);
  assert.equal(c.__readCalls(), 1);
  if (model.kind === 'markdown') {
    assert.equal(model.urlPolicy, 'authorized-file-assets');
    assert.equal(model.baseDir, '/docs');
  }
});

test('memory source does not call authorizeFile and uses assistant-safe policy', async () => {
  const ctx = makeContext();
  const model = await markdownProvider.prepare(memoryReq('a.md'), ctx);
  assert.equal(model.kind, 'markdown');
  const c = ctx as unknown as { __authorizeCalls(): number };
  assert.equal(c.__authorizeCalls(), 0);
  if (model.kind === 'markdown') {
    assert.equal(model.urlPolicy, 'assistant-safe');
    assert.equal(model.baseDir, undefined);
  }
});

test('non-text file kinds are not accepted', () => {
  assert.equal(markdownProvider.accepts(fileReq('/a.png', 'image')), false);
  assert.equal(markdownProvider.accepts(fileReq('/a.md', 'text')), true);
  assert.equal(markdownProvider.accepts(memoryReq('a.md')), true);
  assert.equal(markdownProvider.accepts(memoryReq('a.txt')), false);
});

test('authorize failure propagates as fatal without fallback', async () => {
  const ctx = makeContext({
    authorizeFile: async () => {
      throw new Error('blocked');
    },
  });
  // context 会在真实实现里把 stat 失败包装为 io_error；此处验证 provider 不透传降级
  await assert.rejects(() => markdownProvider.prepare(fileReq('/secret/a.md'), ctx));
});

test('authorized asset URL is not filtered by file markdown policy', () => {
  assert.equal(isAuthorizedAssetUrl('asset://localhost/var/foo.png'), true);
  assert.equal(isAuthorizedAssetUrl('http://asset.localhost/var/foo.png'), true);
  assert.equal(isAuthorizedAssetUrl('https://evil.com/x.png'), false);
  assert.equal(
    transformAuthorizedFileUrl('asset://localhost/var/foo.png', 'src', { tagName: 'img' }),
    'asset://localhost/var/foo.png',
  );
  // 危险 scheme 仍被拒绝
  assert.equal(transformAuthorizedFileUrl('javascript:alert(1)', 'href', { tagName: 'a' }), '');
});

test('buildMarkdownRenderOptions rewrites local images only for authorized-file-assets', () => {
  const fileOpts = buildMarkdownRenderOptions('authorized-file-assets', '/docs');
  const out = fileOpts.rewrite!('![a](./img/x.png)');
  assert.ok(out.includes('asset://localhost'), out);
  const safeOpts = buildMarkdownRenderOptions('assistant-safe');
  assert.equal(safeOpts.rewrite, undefined);
});
