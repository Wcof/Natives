import assert from 'node:assert/strict';
import test from 'node:test';
import { PreviewRegistry } from '../registry';
import { PreviewService } from '../service';
import { PreviewRequestController } from '../request-controller';
import { PreviewProviderError, fatalError, recoverableError } from '../errors';
import { classifyCapability } from '../classify';
import type { PreviewContext, PreviewModel, PreviewProvider, PreviewRequest } from '../contracts';
import { PreviewDiagnostics } from '../diagnostics';

// ── 测试上下文（fake host：可控错误注入）──

function fakeContext(overrides: Partial<PreviewContext> = {}): PreviewContext {
  return {
    authorizeFile: async (path) => ({
      path,
      name: path.split('/').pop() ?? path,
      kind: 'text',
      size: 10,
      mtime: 1,
    }),
    readText: async () => ({ content: 'x', truncated: false, size: 1, mtime: 1, kind: 'text', encoding: 'utf-8' }),
    toAssetUrl: () => 'asset://localhost/fake',
    prepareHtml: async () => ({ content: '<html/>', fsBase: '/', serverPort: 0 }),
    listArchive: async () => ({ entries: [], truncated: false, totalSize: 0 }),
    ...overrides,
  };
}

function provider(partial: Partial<PreviewProvider>): PreviewProvider {
  return {
    id: 'p',
    priority: 10,
    accepts: () => true,
    prepare: async () => ({ kind: 'code', source: '', language: 'text', truncated: false }),
    ...partial,
  };
}

function req(overrides: Partial<PreviewRequest> = {}): PreviewRequest {
  return { source: { type: 'memory', name: 'a.txt', content: '' }, mode: 'preview', surface: 'files', ...overrides };
}

// ── provider priority 确定性 ──

test('provider priority is deterministic (desc then id asc)', async () => {
  const reg = new PreviewRegistry();
  const order: string[] = [];
  const recoverable = () => recoverableError('not_applicable', 'decline');
  reg.register(provider({ id: 'low', priority: 1, prepare: async () => { order.push('low'); return { kind: 'code', source: '', language: 'text', truncated: false }; } }));
  reg.register(provider({ id: 'high', priority: 100, prepare: async () => { order.push('high'); throw recoverable(); } }));
  reg.register(provider({ id: 'same', priority: 100, prepare: async () => { order.push('same'); throw recoverable(); } }));
  await new PreviewService(reg, fakeContext()).prepare(req());
  assert.deepEqual(order, ['high', 'same', 'low']);
});

test('duplicate provider id is rejected', () => {
  const reg = new PreviewRegistry();
  reg.register(provider({ id: 'dup' }));
  assert.throws(() => reg.register(provider({ id: 'dup' })), /duplicate preview provider: dup/);
});

// ── recoverable fallback taxonomy ──

test('recoverable parse_failed falls through to next provider', async () => {
  const reg = new PreviewRegistry();
  const seen: string[] = [];
  reg.register(provider({
    id: 'json',
    priority: 90,
    prepare: async () => { seen.push('json'); throw recoverableError('parse_failed', 'bad json'); },
  }));
  reg.register(provider({
    id: 'code',
    priority: 10,
    prepare: async () => { seen.push('code'); return { kind: 'code', source: 'x', language: 'json', truncated: false }; },
  }));
  const model = await new PreviewService(reg, fakeContext()).prepare(req());
  assert.deepEqual(seen, ['json', 'code']);
  assert.equal(model.kind, 'code');
});

test('not_applicable / unsupported also fall through', async () => {
  const reg = new PreviewRegistry();
  reg.register(provider({ id: 'a', prepare: async () => { throw recoverableError('not_applicable', 'nope'); } }));
  reg.register(provider({ id: 'b', prepare: async () => { throw recoverableError('unsupported', 'nope'); } }));
  reg.register(provider({ id: 'c', prepare: async () => ({ kind: 'code', source: '', language: 'text', truncated: false }) }));
  const model = await new PreviewService(reg, fakeContext()).prepare(req());
  assert.equal(model.kind, 'code');
});

test('fatal permission_denied does NOT fall back', async () => {
  const reg = new PreviewRegistry();
  const code = provider({
    id: 'code',
    priority: 10,
    prepare: async () => ({ kind: 'code', source: '', language: 'text', truncated: false }),
  });
  reg.register(provider({ id: 'json', priority: 90, prepare: async () => { throw fatalError('permission_denied', 'blocked'); } }));
  reg.register(code);
  await assert.rejects(() => new PreviewService(reg, fakeContext()).prepare(req()), (e: unknown) => {
    assert.ok(e instanceof PreviewProviderError);
    assert.equal(e.code, 'permission_denied');
    assert.equal(e.recoverable, false);
    return true;
  });
});

test('fatal security_violation / io_error / host_error do NOT fall back', async () => {
  for (const code of ['security_violation', 'io_error', 'host_error'] as const) {
    const reg = new PreviewRegistry();
    reg.register(provider({ id: 'a', priority: 90, prepare: async () => { throw fatalError(code, 'stop'); } }));
    reg.register(provider({ id: 'b', priority: 10, prepare: async () => ({ kind: 'code', source: '', language: 'text', truncated: false }) }));
    await assert.rejects(() => new PreviewService(reg, fakeContext()).prepare(req()), (e: unknown) => {
      assert.ok(e instanceof PreviewProviderError);
      assert.equal(e.code, code);
      return true;
    });
  }
});

// ── stale / cancel 归属 Surface ──

test('same-surface new request discards old result via controller generation', async () => {
  const ctrl = new PreviewRequestController();
  const first = ctrl.next();
  const second = ctrl.next(); // 新请求 → 旧 generation 失效
  assert.equal(ctrl.isCurrent(first.generation), false);
  assert.equal(ctrl.isCurrent(second.generation), true);
});

test('cancelling surface A does not affect surface B', async () => {
  const a = new PreviewRequestController();
  const b = new PreviewRequestController();
  const tokenA = a.next();
  const tokenB = b.next();
  a.cancel();
  assert.equal(a.isCurrent(tokenA.generation), false);
  assert.equal(b.isCurrent(tokenB.generation), true);
});

test('aborted signal during prepare throws cancelled without noise', async () => {
  const reg = new PreviewRegistry();
  reg.register(provider({
    id: 'slow',
    priority: 10,
    prepare: async (_r, _ctx) => {
      await new Promise((resolve) => setTimeout(resolve, 50));
      return { kind: 'code', source: '', language: 'text', truncated: false };
    },
  }));
  const ctrl = new PreviewRequestController();
  const { signal } = ctrl.next();
  const pending = new PreviewService(reg, fakeContext()).prepare(req({ signal }));
  ctrl.cancel(); // abort signal
  await assert.rejects(() => pending, (e: unknown) => e instanceof PreviewProviderError && e.code === 'cancelled');
});

// ── 并发：Files + Assistant 互不 stale/cancel ──

test('simultaneous Files + Assistant requests stay independent', async () => {
  const reg = new PreviewRegistry();
  reg.register(provider({
    id: 'code',
    priority: 10,
    prepare: async () => ({ kind: 'code', source: 'x', language: 'text', truncated: false }),
  }));
  const filesCtrl = new PreviewRequestController();
  const assistantCtrl = new PreviewRequestController();
  const service = new PreviewService(reg, fakeContext());
  const filesReq = req({ surface: 'files', signal: filesCtrl.next().signal });
  const assistantReq = req({ surface: 'assistant', signal: assistantCtrl.next().signal });
  const [fm, am] = await Promise.all([service.prepare(filesReq), service.prepare(assistantReq)]);
  assert.equal(fm.kind, 'code');
  assert.equal(am.kind, 'code');
  // Files 再发起新请求不会取消 Assistant 的完成结果
  filesCtrl.next();
  assert.equal(filesCtrl.isCurrent(0), false);
  assert.equal(assistantCtrl.isCurrent(1), true);
});

// ── memory / file source 同一 service 契约 ──

test('memory and file sources flow through the same service', async () => {
  const reg = new PreviewRegistry();
  const fileSeen = reg.register(provider({
    id: 'md',
    priority: 100,
    accepts: (r) => r.source.type === 'file',
    prepare: async () => ({ kind: 'markdown', source: 'file-md', truncated: false, urlPolicy: 'authorized-file-assets' }),
  }));
  void fileSeen;
  reg.register(provider({
    id: 'mem',
    priority: 50,
    accepts: (r) => r.source.type === 'memory',
    prepare: async () => ({ kind: 'markdown', source: 'mem-md', truncated: false, urlPolicy: 'assistant-safe' }),
  }));
  const service = new PreviewService(reg, fakeContext());
  const fileModel = await service.prepare(req({ source: { type: 'file', path: '/tmp/a.md', kind: 'text' }, surface: 'files' }));
  const memModel = await service.prepare(req({ source: { type: 'memory', name: 'a.md', content: 'x' }, surface: 'assistant' }));
  assert.equal(fileModel.kind, 'markdown');
  assert.equal(memModel.kind, 'markdown');
  if (fileModel.kind === 'markdown') assert.equal(fileModel.urlPolicy, 'authorized-file-assets');
  if (memModel.kind === 'markdown') assert.equal(memModel.urlPolicy, 'assistant-safe');
});

// ── 未授权 file source 必须拒绝 ──

test('unauthorized file source produces fatal explicit error', async () => {
  const ctx = fakeContext({
    authorizeFile: async () => { throw fatalError('permission_denied', 'blocked: /secret'); },
  });
  const reg = new PreviewRegistry();
  reg.register(provider({
    id: 'media',
    priority: 100,
    accepts: (r) => r.source.type === 'file',
    prepare: async (_r, c) => {
      const file = await c.authorizeFile(_r.source.type === 'file' ? _r.source.path : '');
      return { kind: 'image', src: c.toAssetUrl(file), name: file.name };
    },
  }));
  await assert.rejects(
    () => new PreviewService(reg, ctx).prepare(req({ source: { type: 'file', path: '/secret/x.png', kind: 'image' } })),
    (e: unknown) => e instanceof PreviewProviderError && e.code === 'permission_denied' && e.recoverable === false,
  );
});

// ── 无候选 provider → unsupported ──

test('no matching provider yields unsupported model', async () => {
  const reg = new PreviewRegistry();
  reg.register(provider({ id: 'only', accepts: () => false }));
  const model = await new PreviewService(reg, fakeContext()).prepare(req());
  assert.deepEqual(model, { kind: 'unsupported', reason: 'no preview provider matched' });
});

// ── diagnostics 有界 ──

test('diagnostics buffer is bounded', async () => {
  const reg = new PreviewRegistry();
  reg.register(provider({ id: 'code', priority: 10, prepare: async () => ({ kind: 'code', source: '', language: 'text', truncated: false }) }));
  const diag = new PreviewDiagnostics(4);
  const service = new PreviewService(reg, fakeContext(), diag);
  for (let i = 0; i < 10; i++) await service.prepare(req());
  assert.ok(diag.snapshot().length <= 4);
});

// ── classify 边界 ──

test('classify uses Host kind authority before ext', () => {
  assert.equal(classifyCapability({ type: 'file', path: '/a.png', kind: 'image' }), 'image');
  assert.equal(classifyCapability({ type: 'file', path: '/a.md', kind: 'text' }), 'markdown');
  assert.equal(classifyCapability({ type: 'file', path: '/a.json', kind: 'text' }), 'json');
  assert.equal(classifyCapability({ type: 'file', path: '/a.html', kind: 'text' }), 'html');
  assert.equal(classifyCapability({ type: 'file', path: '/a.csv', kind: 'text' }), 'csv');
  assert.equal(classifyCapability({ type: 'file', path: '/a.ts', kind: 'text' }), 'code');
  assert.equal(classifyCapability({ type: 'memory', name: 'x.json', content: '' }), 'json');
  assert.equal(classifyCapability({ type: 'memory', name: 'x.ts', content: '' }), 'code');
});

// ── 模型类型检查 ──

test('PreviewModel is a typed discriminated union', () => {
  const models: PreviewModel[] = [
    { kind: 'markdown', source: '', truncated: false, urlPolicy: 'assistant-safe' },
    { kind: 'html', revision: 'r', sandbox: 'allow-scripts' },
    { kind: 'json', value: null, formatted: 'null', nodeCount: 1, truncated: false },
    { kind: 'code', source: '', language: 'text', truncated: false },
    { kind: 'image', src: 'asset://localhost/x', name: 'x' },
    { kind: 'video', src: 'asset://localhost/x', name: 'x' },
    { kind: 'audio', src: 'asset://localhost/x', name: 'x' },
    { kind: 'pdf', src: 'asset://localhost/x', name: 'x' },
    { kind: 'csv', headers: [], rows: [], truncated: false },
    { kind: 'archive', entries: [], truncated: false },
    { kind: 'unsupported', reason: 'r' },
  ];
  for (const m of models) assert.ok('kind' in m);
});
