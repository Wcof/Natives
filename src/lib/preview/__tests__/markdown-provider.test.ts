import assert from 'node:assert/strict';
import test from 'node:test';
import type { PreviewContext, PreviewRequest } from '../contracts';
import { fatalError } from '../errors';
import { markdownProvider } from '../providers/markdown';
import {
  buildMarkdownRenderOptions,
  isAuthorizedAssetUrl,
  rewriteAuthorizedLocalImages,
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

test('buildMarkdownRenderOptions never produces asset URLs itself (SEC-001)', () => {
  // SEC-001: renderer 侧不得自行把相对路径改写为 asset:// URL——没有 ctx，
  // 无法逐资源授权；改写必须发生在 provider（rewriteAuthorizedLocalImages）。
  const fileOpts = buildMarkdownRenderOptions('authorized-file-assets', '/docs');
  assert.equal(fileOpts.rewrite, undefined);
  const safeOpts = buildMarkdownRenderOptions('assistant-safe');
  assert.equal(safeOpts.rewrite, undefined);
});

test('authorized embedded local images are rewritten and listed in authorizedAssets (SEC-001 positive)', async () => {
  const ctx = makeContext({
    readText: async () => ({
      content: '![a](./img/x.png)\n\n![](/docs/img/y.png)',
      truncated: false,
      size: 60,
      mtime: 1,
      kind: 'text',
      encoding: 'utf-8',
    }),
  });
  const model = await markdownProvider.prepare(fileReq('/docs/a.md'), ctx);
  assert.equal(model.kind, 'markdown');
  if (model.kind === 'markdown') {
    assert.ok(model.source.includes('asset://localhost/docs/img/x.png'), model.source);
    assert.ok(model.source.includes('asset://localhost/docs/img/y.png'), model.source);
    assert.ok(model.authorizedAssets?.includes('/docs/img/x.png'));
    assert.ok(model.authorizedAssets?.includes('/docs/img/y.png'));
  }
});

test('unauthorized embedded image produces no asset URL and is authorized per-resource (SEC-001 negative)', async () => {
  const authorizedPaths: string[] = [];
  const ctx = makeContext({
    readText: async () => ({
      content: '![a](./img/secret.png)',
      truncated: false,
      size: 30,
      mtime: 1,
      kind: 'text',
      encoding: 'utf-8',
    }),
    authorizeFile: async (path) => {
      authorizedPaths.push(path);
      // 主文档授权成功；文档内图片引用授权失败（无访问权）→ 不产出 asset URL
      if (path === '/docs/a.md') return { path, name: 'a.md', kind: 'text', size: 30, mtime: 1 };
      throw fatalError('permission_denied', 'blocked');
    },
  });
  const model = await markdownProvider.prepare(fileReq('/docs/a.md'), ctx);
  assert.equal(model.kind, 'markdown');
  if (model.kind === 'markdown') {
    // 未授权：原始引用保留，不产出可访问的 asset:// URL
    assert.ok(!model.source.includes('asset://localhost'), model.source);
    assert.ok(model.source.includes('./img/secret.png'), model.source);
    assert.deepEqual(model.authorizedAssets, []);
  }
  // 主文档与文档内引用都经同一 authorizeFile 通道（非 baseDir 字符串判断），
  // 图片引用确实被逐资源授权且被拒
  assert.deepEqual(authorizedPaths, ['/docs/a.md', '/docs/img/secret.png']);
});

test('markdown ref outside baseDir is never authorized and produces no asset URL (SEC-001)', async () => {
  const ctx = makeContext({
    readText: async () => ({
      content: '![x](../../etc/passwd)',
      truncated: false,
      size: 30,
      mtime: 1,
      kind: 'text',
      encoding: 'utf-8',
    }),
    authorizeFile: async (path) => {
      // 越根引用绝不被授权：authorizeFile 只应收到主文档授权请求
      assert.equal(path, '/docs/a.md', `authorizeFile must not be called for out-of-base refs, got ${path}`);
      return { path, name: 'a.md', kind: 'text', size: 30, mtime: 1 };
    },
  });
  const model = await markdownProvider.prepare(fileReq('/docs/a.md'), ctx);
  assert.equal(model.kind, 'markdown');
  if (model.kind === 'markdown') {
    assert.ok(!model.source.includes('asset://localhost'), model.source);
    assert.deepEqual(model.authorizedAssets, []);
  }
});

test('rewriteAuthorizedLocalImages authorizes each unique ref through the same channel (SEC-001)', async () => {
  const calls: string[] = [];
  const authorizeFile: PreviewContext['authorizeFile'] = async (path) => {
    calls.push(path);
    return { path, name: path.split('/').pop() ?? path, kind: 'image', size: 4, mtime: 1 };
  };
  const { text, authorizedAssets } = await rewriteAuthorizedLocalImages(
    '![a](./img/a.png) ![b](./img/a.png) ![c](./img/b.png)',
    '/docs',
    authorizeFile,
  );
  // 同一引用只授权一次；每篇图片都产 asset URL
  assert.deepEqual(calls, ['/docs/img/a.png', '/docs/img/b.png']);
  assert.ok(text.includes('asset://localhost/docs/img/a.png'), text);
  assert.ok(text.includes('asset://localhost/docs/img/b.png'), text);
  assert.deepEqual(authorizedAssets.sort(), ['/docs/img/a.png', '/docs/img/b.png']);
});
