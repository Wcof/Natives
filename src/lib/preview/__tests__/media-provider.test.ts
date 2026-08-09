import assert from 'node:assert/strict';
import test from 'node:test';
import type { PreviewContext, PreviewRequest } from '../contracts';
import { PreviewProviderError } from '../errors';
import { mediaProvider } from '../providers/media';
import { pdfProvider } from '../providers/pdf';

function makeContext(overrides: Partial<PreviewContext> = {}): PreviewContext & {
  __toAssetUrlCalls(): string[];
} {
  const assetUrls: string[] = [];
  const ctx: PreviewContext = {
    authorizeFile: async (path) => ({ path, name: path.split('/').pop() ?? path, kind: 'image', size: 10, mtime: 1 }),
    readText: async () => ({ content: '', truncated: false, size: 0, mtime: 1, kind: 'text', encoding: 'utf-8' }),
    toAssetUrl: (file) => {
      assetUrls.push(file.path);
      return `asset://localhost${encodeURI(file.path)}`;
    },
    prepareHtml: async () => ({ content: '', fsBase: '', serverPort: 0 }),
    listArchive: async () => ({ entries: [], truncated: false, totalSize: 0 }),
    ...overrides,
  };
  return Object.assign(ctx, { __toAssetUrlCalls: () => assetUrls }) as PreviewContext & { __toAssetUrlCalls(): string[] };
}

function fileReq(path: string, kind: 'image' | 'video' | 'audio' | 'pdf' | 'text'): PreviewRequest {
  return { source: { type: 'file', path, kind }, mode: 'preview', surface: 'files' };
}

test('file source must be authorized before toAssetUrl (image)', async () => {
  const ctx = makeContext();
  const model = await mediaProvider.prepare(fileReq('/a/photo.png', 'image'), ctx);
  assert.equal(model.kind, 'image');
  if (model.kind === 'image') {
    assert.ok(model.src.startsWith('asset://localhost'));
    assert.deepEqual(ctx.__toAssetUrlCalls(), ['/a/photo.png']);
  }
});

test('toAssetUrl only ever receives AuthorizedPreviewFile (no raw path)', async () => {
  // 若 provider 把 raw path 直接交给 toAssetUrl，URL 也会出现，但 authorizeFile 未调用 → 测试失败
  const ctx = makeContext();
  await mediaProvider.prepare(fileReq('/a/video.mp4', 'video'), ctx);
  assert.deepEqual(ctx.__toAssetUrlCalls(), ['/a/video.mp4']);
});

test('authorize permission_denied propagates fatal without fallback', async () => {
  const fatal = new PreviewProviderError('permission_denied', 'blocked');
  const ctx = makeContext({ authorizeFile: async () => { throw fatal; } });
  const err = await mediaProvider.prepare(fileReq('/secret/x.png', 'image'), ctx).then(
    () => null,
    (e: unknown) => e,
  );
  assert.equal(err, fatal);
  assert.equal((err as PreviewProviderError).recoverable, false);
});

test('memory source is not accepted for media', async () => {
  const ctx = makeContext();
  assert.equal(mediaProvider.accepts({ source: { type: 'memory', name: 'x.png', content: '' }, mode: 'preview', surface: 'assistant' }), false);
  await assert.rejects(
    () => mediaProvider.prepare({ source: { type: 'memory', name: 'x.png', content: '' }, mode: 'preview', surface: 'assistant' }, ctx),
    (e: unknown) => e instanceof PreviewProviderError && e.code === 'not_applicable',
  );
});

test('pdf provider requires file source and authorizes before asset url', async () => {
  const ctx = makeContext();
  const model = await pdfProvider.prepare(fileReq('/a/doc.pdf', 'pdf'), ctx);
  assert.equal(model.kind, 'pdf');
  if (model.kind === 'pdf') {
    assert.deepEqual(ctx.__toAssetUrlCalls(), ['/a/doc.pdf']);
  }
  assert.equal(pdfProvider.accepts({ source: { type: 'memory', name: 'x.pdf', content: '' }, mode: 'preview', surface: 'assistant' }), false);
});

test('heic text-kind file maps to image via ext', async () => {
  const ctx = makeContext();
  const model = await mediaProvider.prepare(fileReq('/a/photo.heic', 'text'), ctx);
  assert.equal(model.kind, 'image');
});

// ── PREV-004: kind 判断用真实 key 空间；Host kind 优先；未知扩展名不误判 image ──

test('unknown extension with Host kind video uses Host kind (PREV-004)', async () => {
  const ctx = makeContext();
  const model = await mediaProvider.prepare(fileReq('/a/clip.xyz', 'video'), ctx);
  assert.equal(model.kind, 'video');
  if (model.kind === 'video') {
    assert.deepEqual(ctx.__toAssetUrlCalls(), ['/a/clip.xyz']);
  }
});

test('Host kind wins over extension when they disagree (PREV-004)', async () => {
  const ctx = makeContext();
  const model = await mediaProvider.prepare(fileReq('/a/strange.mp3', 'image'), ctx);
  assert.equal(model.kind, 'image');
});

test('unknown extension with text kind is not misclassified as image (PREV-004)', async () => {
  const ctx = makeContext();
  assert.equal(mediaProvider.accepts(fileReq('/a/unknown.xyz', 'text')), false);
  const err = await mediaProvider.prepare(fileReq('/a/unknown.xyz', 'text'), ctx).then(
    () => null,
    (e: unknown) => e,
  );
  assert.ok(err instanceof PreviewProviderError);
  assert.equal((err as PreviewProviderError).code, 'not_applicable');
});
