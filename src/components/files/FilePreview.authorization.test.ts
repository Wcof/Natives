import assert from 'node:assert/strict';
import test from 'node:test';
import type { PreviewContext } from '@/lib/preview/contracts';
import { authorizeImageEditAsset } from './FilePreview';

function context(calls: string[]): PreviewContext {
  return {
    authorizeFile: async (path) => {
      calls.push(`authorize:${path}`);
      return { path, name: path.split('/').pop() ?? path, kind: 'image', size: 1, mtime: 1 };
    },
    toAssetUrl: (file) => {
      calls.push(`asset:${file.path}`);
      return `asset://localhost${file.path}`;
    },
    readText: async () => { throw new Error('unused'); },
    prepareHtml: async () => { throw new Error('unused'); },
    listArchive: async () => { throw new Error('unused'); },
  };
}

test('image editor authorizes a regular image before creating its asset URL', async () => {
  const calls: string[] = [];
  const url = await authorizeImageEditAsset('/images/a.png', context(calls));
  assert.equal(url, 'asset://localhost/images/a.png');
  assert.deepEqual(calls, ['authorize:/images/a.png', 'asset:/images/a.png']);
});

test('HEIC editor authorizes both source and converted output before creating URL', async () => {
  const calls: string[] = [];
  const url = await authorizeImageEditAsset(
    '/images/a.heic',
    context(calls),
    async (path) => {
      calls.push(`convert:${path}`);
      return { ok: true, jpegPath: '/cache/a.jpg' };
    },
  );

  assert.equal(url, 'asset://localhost/cache/a.jpg');
  assert.deepEqual(calls, [
    'authorize:/images/a.heic',
    'convert:/images/a.heic',
    'authorize:/cache/a.jpg',
    'asset:/cache/a.jpg',
  ]);
});

test('denied image authorization never creates an asset URL', async () => {
  let assetCalls = 0;
  const ctx = context([]);
  ctx.authorizeFile = async () => { throw new Error('denied'); };
  ctx.toAssetUrl = () => {
    assetCalls++;
    return 'asset://localhost/forbidden';
  };

  await assert.rejects(() => authorizeImageEditAsset('/secret/a.png', ctx), /denied/);
  assert.equal(assetCalls, 0);
});

test('HEIC conversion failure does not fall back to the raw source asset', async () => {
  const calls: string[] = [];
  const ctx = context(calls);

  await assert.rejects(
    () => authorizeImageEditAsset('/images/a.heic', ctx, async (path) => {
      calls.push(`convert:${path}`);
      throw new Error('sips failed');
    }),
    /image conversion failed/,
  );

  assert.deepEqual(calls, [
    'authorize:/images/a.heic',
    'convert:/images/a.heic',
  ]);
});

test('denied converted output never creates an asset URL or falls back to source', async () => {
  const calls: string[] = [];
  const ctx = context(calls);
  const authorizeFile = ctx.authorizeFile;
  ctx.authorizeFile = async (path) => {
    if (path === '/cache/a.jpg') {
      calls.push(`authorize:${path}`);
      throw new Error('converted output denied');
    }
    return authorizeFile(path);
  };

  await assert.rejects(
    () => authorizeImageEditAsset('/images/a.heic', ctx, async (path) => {
      calls.push(`convert:${path}`);
      return { ok: true, jpegPath: '/cache/a.jpg' };
    }),
    /converted output denied/,
  );

  assert.deepEqual(calls, [
    'authorize:/images/a.heic',
    'convert:/images/a.heic',
    'authorize:/cache/a.jpg',
  ]);
});

test('cancelled image edit request does not authorize, convert, or create an asset URL', async () => {
  const calls: string[] = [];
  const controller = new AbortController();
  controller.abort();

  await assert.rejects(
    () => authorizeImageEditAsset(
      '/images/a.heic',
      context(calls),
      async (path) => {
        calls.push(`convert:${path}`);
        return { ok: true, jpegPath: '/cache/a.jpg' };
      },
      controller.signal,
    ),
    (error: unknown) => error instanceof Error && error.message === 'request cancelled',
  );

  assert.deepEqual(calls, []);
});
