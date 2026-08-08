import assert from 'node:assert/strict';
import test from 'node:test';
import { PreviewRegistry } from '../registry';
import { PreviewService } from '../service';
import { PreviewRequestController } from '../request-controller';
import { fatalError, PreviewProviderError } from '../errors';
import { createBuiltinRegistry, BUILTIN_PROVIDERS } from '../composition';
import type { PreviewContext, PreviewModel, PreviewRequest } from '../contracts';

function fakeContext(): PreviewContext {
  return {
    authorizeFile: async (path) => ({ path, name: path.split('/').pop() ?? path, kind: 'text', size: 1, mtime: 1 }),
    readText: async () => ({ content: 'hello', truncated: false, size: 5, mtime: 1, kind: 'text', encoding: 'utf-8' }),
    toAssetUrl: () => 'asset://localhost/x',
    prepareHtml: async () => ({ content: '', fsBase: '', serverPort: 0 }),
    listArchive: async () => ({ entries: [], truncated: false, totalSize: 0 }),
  };
}

test('builtin registry contains C0 leaf + T16/T17 providers (no HTML while H0 BLOCKED)', () => {
  const ids = BUILTIN_PROVIDERS.map((p) => p.id).sort();
  assert.deepEqual(ids, ['archive', 'code', 'csv', 'json', 'markdown', 'media', 'pdf']);
  const reg = createBuiltinRegistry();
  assert.equal(reg.size, 7);
});

test('markdown file source routes through registry → service → typed model', async () => {
  const reg = createBuiltinRegistry();
  const service = new PreviewService(reg, fakeContext());
  const model = await service.prepare({ source: { type: 'file', path: '/a.md', kind: 'text' }, mode: 'preview', surface: 'files' });
  assert.equal(model.kind, 'markdown');
});

test('json parse failure falls back to code provider', async () => {
  const ctx = fakeContext();
  ctx.readText = async () => ({ content: '{bad', truncated: false, size: 4, mtime: 1, kind: 'text', encoding: 'utf-8' });
  const service = new PreviewService(createBuiltinRegistry(), ctx);
  const model = await service.prepare({ source: { type: 'file', path: '/a.json', kind: 'text' }, mode: 'preview', surface: 'files' });
  assert.equal(model.kind, 'code');
});

test('fatal error stops without falling back to code', async () => {
  const ctx = fakeContext();
  ctx.authorizeFile = async () => {
    throw fatalError('permission_denied', 'blocked');
  };
  const service = new PreviewService(createBuiltinRegistry(), ctx);
  await assert.rejects(
    () => service.prepare({ source: { type: 'file', path: '/secret/a.json', kind: 'text' }, mode: 'preview', surface: 'files' }),
    (e: unknown) => e instanceof Error && (e as PreviewProviderError).code === 'permission_denied',
  );
});

test('unknown text file falls to code provider', async () => {
  const service = new PreviewService(createBuiltinRegistry(), fakeContext());
  const model = await service.prepare({ source: { type: 'file', path: '/a.xyz', kind: 'text' }, mode: 'preview', surface: 'files' });
  assert.equal(model.kind, 'code');
});

test('memory json source works without FS access', async () => {
  const service = new PreviewService(createBuiltinRegistry(), fakeContext());
  const model = await service.prepare({ source: { type: 'memory', name: 'x.json', content: '{"a":1}' }, mode: 'preview', surface: 'assistant' });
  assert.equal(model.kind, 'json');
  if (model.kind === 'json') assert.deepEqual(model.value, { a: 1 });
});

test('Surface-local controllers keep Files and Assistant independent', async () => {
  const files = new PreviewRequestController();
  const assistant = new PreviewRequestController();
  const f1 = files.next();
  const a1 = assistant.next();
  files.cancel();
  assert.equal(files.isCurrent(f1.generation), false);
  assert.equal(assistant.isCurrent(a1.generation), true);
});
