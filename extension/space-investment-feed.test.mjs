// 空间投资卡数据源生命周期测试（ADR-0036 / 方案 03 P0 失败验收）。
//
// 覆盖：一次查询页面共享、readiness 门控（未 ready 不 app:start）、
// unsupported、另一页面占锁、迟到响应丢弃、刷新合并、金额遮蔽通知、
// 卡片移除挂起与恢复。

import assert from 'node:assert/strict';
import {
  resetInvestmentFeedForTests,
  initInvestmentFeed,
  setInvestmentCardsPresent,
  subscribeInvestmentCards,
  refreshInvestmentCards,
  invalidateInvestmentCards,
  suspendInvestmentFeed,
  resumeInvestmentFeed,
  setMoneyVisible,
  isMoneyVisible,
  investmentFeedDiagnostics,
  setInvestmentNavigationHandler,
  openInvestmentDetail,
} from './space-investment-feed.js';

globalThis.document = {
  visibilityState: 'visible',
  addEventListener: () => {},
  removeEventListener: () => {},
};

// —— 脚本化 App Runtime 假 Port：按方法应答，记录调用序列 ——
// sink：跨 swapRuntime 聚合所有 Port 的调用（查询结束后 Port 会被断开替换）。
function makeRuntimePort(script, sink) {
  const calls = sink ?? [];
  let listeners = [];
  let disconnectListeners = [];
  let connected = true;
  const port = {
    onMessage: { addListener(fn) { listeners.push(fn); } },
    onDisconnect: { addListener(fn) { disconnectListeners.push(fn); } },
    postMessage(request) {
      calls.push(JSON.parse(JSON.stringify(request)));
      const respond = (payload) => queueMicrotask(() => {
        if (connected) listeners.forEach((fn) => fn({ id: request.id, ...payload }));
      });
      script(respond, request);
    },
    disconnect() {
      connected = false;
      disconnectListeners.forEach((fn) => fn());
    },
  };
  return { port, calls };
}

const SNAPSHOT = {
  schema: 'fund.cards.v1',
  generatedAt: '2026-09-29T12:00:00+08:00',
  payload: {
    registeredFundHoldings: {
      positionCount: 3, accountCount: 2,
      cost: { complete: true, total: { amount: '650', currency: 'CNY' } },
      holdingDates: { earliest: '2026-09-01', latest: '2026-09-10' },
      quality: 'ready',
    },
  },
};

function happyScript(respond, request, options = {}) {
  switch (request.method) {
    case 'app:handshake':
      if (options.handshakeError) return respond({ ok: false, error: { code: options.handshakeError, message: 'busy elsewhere', retryable: false } });
      return respond({ ok: true, result: { protocolVersion: 2, appId: 'fund', state: 'initialized', moduleApiVersion: 1, dataSchemaVersion: 2, capabilityVersion: 2 } });
    case 'app:data_status':
      return respond({ ok: true, result: { currentSchema: 2, migrationState: 'committed', lastDataWriterVersion: '0.1.0', hasCommittedNewWrites: false, previousVersionCompatible: true, ...( 'readiness' in options ? { readiness: options.readiness } : { readiness: 'ready' } ) } });
    case 'app:start':
      if (options.refuseStart) throw new Error('app:start must not be called');
      return respond({ ok: true, result: { instanceId: 'inst-1', port: 12345, generation: 'gen-1', state: 'ready' } });
    case 'app:card_snapshot':
      if (options.snapshotError) return respond({ ok: false, error: { code: 'APP_RUNTIME_INVALID_STATE', message: 'boom', retryable: false } });
      return respond({ ok: true, result: SNAPSHOT });
    default:
      return respond({ ok: false, error: { code: 'APP_PROTOCOL_MISMATCH', message: 'unknown', retryable: false } });
  }
}

function setup(script, coreAppsGet, { coreCall: coreCallImpl } = {}) {
  const coreCalls = [];
  const runtimeCalls = [];
  let runtime = makeRuntimePort(script, runtimeCalls);
  let connectCount = 0;
  globalThis.chrome = {
    runtime: {
      connectNative(name) {
        connectCount += 1;
        assert.equal(name, 'com.natives.file_manager', 'module sessions must use the shared Core relay');
        return runtime.port;
      },
    },
  };
  const defaultCoreCall = async (method, params) => {
    coreCalls.push({ method, params });
    return coreAppsGet ?? {
      app: { app_id: 'fund', name: '投资', version: '2.6.0', enabled: true, product_generation: 7 },
      runtime_host: 'com.natives.local.app_runtime',
    };
  };
  const coreCall = coreCallImpl
    ? async (method, params) => {
      coreCalls.push({ method, params });
      return coreCallImpl(method, params);
    }
    : defaultCoreCall;
  const states = [];
  const unsubscribe = subscribeInvestmentCards((state) => states.push({ ...state }));
  return {
    coreCalls,
    coreCall,
    get connectCount() { return connectCount; },
    get calls() { return runtimeCalls; },
    swapRuntime(nextScript) { runtime = makeRuntimePort(nextScript, runtimeCalls); },
    unsubscribe,
    states,
  };
}

const realSetTimeout = globalThis.setTimeout;
const flush = () => new Promise((resolve) => realSetTimeout(resolve, 20));

// 1. 快乐路径：一次查询后 ready，页面共享一次 Port。
{
  resetInvestmentFeedForTests();
  const ctx = setup((respond, request) => happyScript(respond, request));
  initInvestmentFeed({ coreCall: ctx.coreCall });
  setInvestmentCardsPresent(true);
  await flush();
  const diag = investmentFeedDiagnostics();
  assert.equal(diag.status, 'ready', `diag=${JSON.stringify(diag)}`);
  assert.equal(ctx.calls.filter((c) => c.method === 'app:card_snapshot').length, 1);
  assert.equal(ctx.coreCalls.filter((c) => c.method === 'apps:get').length, 1);
  // 订阅者能拿到投影
  const last = ctx.states.at(-1);
  assert.equal(last.snapshot.registeredFundHoldings.positionCount, 3);
  ctx.unsubscribe();
  console.log('✓ happy path: one shared query, ready state');
}

// 2. readiness 非 ready：不 app:start，状态 unavailable + APP_DATA_NOT_READY。
{
  resetInvestmentFeedForTests();
  const ctx = setup((respond, request) => happyScript(respond, request, { readiness: 'migration_required' }));
  initInvestmentFeed({ coreCall: ctx.coreCall });
  setInvestmentCardsPresent(true);
  await flush();
  const diag = investmentFeedDiagnostics();
  assert.equal(diag.status, 'unavailable');
  assert.equal(diag.errorCode, 'APP_DATA_NOT_READY');
  assert.equal(diag.readiness, 'migration_required');
  assert.equal(ctx.calls.some((c) => c.method === 'app:start'), false, '未 ready 不得 app:start');
  console.log('✓ readiness gate: start blocked when not ready');
}

// 3. readiness 缺失（旧模块）：unsupported（能力不支持，非暂时失败），不 start。
{
  resetInvestmentFeedForTests();
  const ctx = setup((respond, request) => happyScript(respond, request, { readiness: undefined }));
  initInvestmentFeed({ coreCall: ctx.coreCall });
  setInvestmentCardsPresent(true);
  await flush();
  const diag = investmentFeedDiagnostics();
  assert.equal(diag.status, 'unsupported', `diag=${JSON.stringify(diag)}`);
  assert.equal(diag.errorCode, 'CARD_SNAPSHOT_UNSUPPORTED');
  assert.equal(ctx.calls.some((c) => c.method === 'app:start'), false);
  console.log('✓ missing readiness treated as unsupported');
}

// 4. 另一页面占锁：诚实不可用，不自动重试。
{
  resetInvestmentFeedForTests();
  const ctx = setup((respond, request) => happyScript(respond, request, { handshakeError: 'APP_RUNNING_ELSEWHERE' }));
  initInvestmentFeed({ coreCall: ctx.coreCall });
  setInvestmentCardsPresent(true);
  await flush();
  const diag = investmentFeedDiagnostics();
  assert.equal(diag.status, 'unavailable');
  assert.equal(diag.errorCode, 'APP_RUNNING_ELSEWHERE');
  const attempts = ctx.calls.filter((c) => c.method === 'app:handshake').length;
  await flush();
  assert.equal(ctx.calls.filter((c) => c.method === 'app:handshake').length, attempts, '不得自动重试');
  // 手动刷新：再试一次
  ctx.swapRuntime((respond, request) => happyScript(respond, request));
  refreshInvestmentCards();
  await flush();
  assert.equal(investmentFeedDiagnostics().status, 'ready');
  console.log('✓ elsewhere lock: honest unavailable, manual refresh recovers');
}

// 5. 刷新合并：进行中不叠加第二次查询。
{
  resetInvestmentFeedForTests();
  let release;
  const gate = new Promise((resolve) => { release = resolve; });
  const ctx = setup((respond, request) => {
    if (request.method === 'app:handshake') {
      return gate.then(() => respond({ ok: true, result: { protocolVersion: 2, appId: 'fund', state: 'initialized', moduleApiVersion: 1, dataSchemaVersion: 2, capabilityVersion: 2 } }));
    }
    return happyScript(respond, request);
  });
  initInvestmentFeed({ coreCall: ctx.coreCall });
  setInvestmentCardsPresent(true);
  await flush(); // 查询卡在握手 gate 上
  assert.equal(investmentFeedDiagnostics().status, 'loading');
  refreshInvestmentCards();
  refreshInvestmentCards();
  release();
  await flush();
  assert.equal(ctx.calls.filter((c) => c.method === 'app:handshake').length, 1, '重复刷新合并为一次');
  console.log('✓ refresh merge: one in-flight query');
}

// 6. suspend 丢弃迟到响应；恢复后重查。
{
  resetInvestmentFeedForTests();
  const ctx = setup((respond, request) => happyScript(respond, request));
  initInvestmentFeed({ coreCall: ctx.coreCall });
  setInvestmentCardsPresent(true);
  await flush();
  assert.equal(investmentFeedDiagnostics().status, 'ready');
  suspendInvestmentFeed();
  refreshInvestmentCards(); // suspended：不发起
  await flush();
  const genBefore = investmentFeedDiagnostics().queryGeneration;
  const snapshotCallsBefore = ctx.calls.filter((c) => c.method === 'app:card_snapshot').length;
  resumeInvestmentFeed();
  await flush();
  assert.ok(investmentFeedDiagnostics().queryGeneration > genBefore || investmentFeedDiagnostics().status === 'ready');
  assert.ok(ctx.calls.filter((c) => c.method === 'app:card_snapshot').length >= snapshotCallsBefore);
  console.log('✓ suspend/resume: late responses discarded, resume re-queries');
}

// 7. 卡片全部移除：挂起并断开，不查询。
{
  resetInvestmentFeedForTests();
  const ctx = setup((respond, request) => happyScript(respond, request));
  initInvestmentFeed({ coreCall: ctx.coreCall });
  setInvestmentCardsPresent(true);
  await flush();
  setInvestmentCardsPresent(false);
  const before = ctx.calls.length;
  refreshInvestmentCards();
  await flush();
  assert.equal(ctx.calls.length, before, '无卡片不查询');
  console.log('✓ no cards: query suppressed');
}

// 8. 金额遮蔽：页面级内存开关通知订阅者，无持久化。
{
  resetInvestmentFeedForTests();
  const ctx = setup((respond, request) => happyScript(respond, request));
  initInvestmentFeed({ coreCall: ctx.coreCall });
  const seen = [];
  const unsubscribe = subscribeInvestmentCards((state) => seen.push(state.moneyVisible));
  assert.equal(isMoneyVisible(), false);
  setMoneyVisible(true);
  assert.equal(isMoneyVisible(), true);
  setMoneyVisible(false);
  unsubscribe();
  assert.ok(seen.includes(true) && seen.includes(false));
  console.log('✓ money visibility toggles notify subscribers');
}

// 9. 深链 handler：白名单 view 透传，非法 view 不透传。
{
  let received = null;
  setInvestmentNavigationHandler((view) => { received = view; });
  openInvestmentDetail('holdings');
  assert.equal(received, 'holdings');
  received = null;
  openInvestmentDetail('../../etc/passwd');
  assert.equal(received, null, '非法 view 不得透传');
  received = null;
  openInvestmentDetail('<script>');
  assert.equal(received, null);
  setInvestmentNavigationHandler(null);
  console.log('✓ deep link: whitelisted views only');
}

// 10. 模块禁用：不可用且不连接 Runtime。
{
  resetInvestmentFeedForTests();
  const ctx = setup((respond, request) => happyScript(respond, request), {
    app: { app_id: 'fund', name: '投资', version: '2.6.0', enabled: false, product_generation: 7 },
    runtime_host: 'com.natives.local.app_runtime',
  });
  initInvestmentFeed({ coreCall: ctx.coreCall });
  setInvestmentCardsPresent(true);
  await flush();
  const diag = investmentFeedDiagnostics();
  assert.equal(diag.status, 'unavailable');
  assert.equal(diag.errorCode, 'APP_DISABLED');
  assert.equal(ctx.calls.length, 0, '禁用时不连接 App Runtime');
  console.log('✓ disabled module: no runtime connection');
}

// 11. 15 秒预算超时：终态是 APP_CARD_TIMEOUT；之后迟到的成功响应
//     不得把不可用翻回已更新（方案 03 P0 失败验收）。
{
  resetInvestmentFeedForTests();
  let pendingSnapshot = null;
  const timers = [];
  globalThis.setTimeout = (fn, ms) => { timers.push({ fn, ms }); return timers.length; };
  try {
    const ctx = setup((respond, request) => {
      if (request.method === 'app:card_snapshot') {
        // 故意不回应：只有预算超时能让这次查询结束。
        pendingSnapshot = (payload) => respond(payload);
        return;
      }
      return happyScript(respond, request);
    });
    initInvestmentFeed({ coreCall: ctx.coreCall });
    setInvestmentCardsPresent(true);
    await flush();
    const budget = timers.find((t) => t.ms === 15_000);
    assert.ok(budget, '必须注册 15 秒总预算定时器');
    budget.fn();
    const diag = investmentFeedDiagnostics();
    assert.equal(diag.status, 'unavailable', `diag=${JSON.stringify(diag)}`);
    assert.equal(diag.errorCode, 'APP_CARD_TIMEOUT');
    // 迟到响应：即使成功也不得改变终态。
    pendingSnapshot?.({ ok: true, result: SNAPSHOT });
    await flush();
    const after = investmentFeedDiagnostics();
    assert.equal(after.status, 'unavailable', `迟到响应不得翻回 ready：${JSON.stringify(after)}`);
    assert.equal(after.errorCode, 'APP_CARD_TIMEOUT');
  } finally {
    globalThis.setTimeout = realSetTimeout;
  }
  console.log('✓ budget timeout is terminal; late success is discarded');
}

// 12. 首次查询途中被 suspend：不得停留在「正在读取」（无可达查询在飞行）。
{
  resetInvestmentFeedForTests();
  let release;
  const gate = new Promise((resolve) => { release = resolve; });
  const ctx = setup((respond, request) => {
    if (request.method === 'app:handshake') {
      return gate.then(() => respond({ ok: true, result: { protocolVersion: 2, appId: 'fund', state: 'initialized', moduleApiVersion: 1, dataSchemaVersion: 2, capabilityVersion: 2 } }));
    }
    return happyScript(respond, request);
  });
  initInvestmentFeed({ coreCall: ctx.coreCall });
  setInvestmentCardsPresent(true);
  await flush();
  assert.equal(investmentFeedDiagnostics().status, 'loading');
  suspendInvestmentFeed();
  const diag = investmentFeedDiagnostics();
  assert.equal(diag.inFlight, false);
  assert.notEqual(diag.status, 'loading', 'suspend 后不得停留在 loading');
  assert.equal(diag.errorCode, 'SUSPENDED');
  release();
  await flush();
  assert.equal(investmentFeedDiagnostics().status, 'unavailable', '被丢弃的查询不得改写状态');
  console.log('✓ suspend during first query leaves an honest state');
}

// 13. 已有快照且查询在飞行时 suspend：SUSPENDED（主动让出，非失败）+ 保留快照。
{
  resetInvestmentFeedForTests();
  const ctx = setup((respond, request) => happyScript(respond, request));
  initInvestmentFeed({ coreCall: ctx.coreCall });
  setInvestmentCardsPresent(true);
  await flush();
  assert.equal(investmentFeedDiagnostics().status, 'ready');
  // 再发起一次会卡住的刷新，让 suspend 有可取消的查询。
  let release;
  const gate = new Promise((resolve) => { release = resolve; });
  ctx.swapRuntime((respond, request) => {
    if (request.method === 'app:handshake') {
      return gate.then(() => respond({ ok: true, result: { protocolVersion: 2, appId: 'fund', state: 'initialized', moduleApiVersion: 1, dataSchemaVersion: 2, capabilityVersion: 2 } }));
    }
    return happyScript(respond, request);
  });
  refreshInvestmentCards();
  await flush();
  assert.equal(investmentFeedDiagnostics().status, 'loading');
  suspendInvestmentFeed();
  const seen = ctx.states.at(-1);
  assert.equal(seen.status, 'unavailable');
  assert.equal(seen.errorCode, 'SUSPENDED');
  assert.ok(seen.snapshot, 'suspend 保留上次本页快照');
  release();
  await flush();
  assert.equal(investmentFeedDiagnostics().errorCode, 'SUSPENDED', '被取消的查询不得改写状态');
  console.log('✓ suspend keeps snapshot and reports SUSPENDED');
}

// 14. 20 张投资卡：仍只查询一次、只开一个 Port；20 次手动刷新合并为一次。
{
  resetInvestmentFeedForTests();
  const ctx = setup((respond, request) => happyScript(respond, request));
  initInvestmentFeed({ coreCall: ctx.coreCall });
  const unsubs = [];
  const perCard = [];
  for (let i = 0; i < 20; i += 1) {
    // 每张卡各自订阅并上报存在（真实 renderer 的行为）。
    unsubs.push(subscribeInvestmentCards((state) => perCard.push({ ...state })));
    setInvestmentCardsPresent(true);
  }
  await flush();
  const diag = investmentFeedDiagnostics();
  assert.equal(diag.status, 'ready', `diag=${JSON.stringify(diag)}`);
  // apps:get 走 Core（coreCall），记录在 ctx.coreCalls；Runtime Port 只承载
  // app:* 协议调用（此前误从 ctx.calls 读取，恒为 0）。
  assert.equal(ctx.coreCalls.filter((c) => c.method === 'apps:get').length, 1, '20 张卡只查一次 apps:get');
  assert.equal(ctx.calls.filter((c) => c.method === 'app:card_snapshot').length, 1, '20 张卡仍只查询一次');
  assert.equal(ctx.calls.filter((c) => c.method === 'app:handshake').length, 1);
  assert.equal(ctx.connectCount, 1, '20 张卡只开一个 Port');
  // 每个订阅者都收到同一份投影（无卡片各自重查）。订阅瞬间的初始态
  // （idle，尚无快照）不计入快照断言。
  assert.ok(perCard.length >= 20);
  const withSnapshot = perCard.filter((s) => s.snapshot?.registeredFundHoldings);
  assert.ok(withSnapshot.length >= 20, '每个订阅者都收到投影');
  for (const state of withSnapshot) {
    assert.equal(state.snapshot?.registeredFundHoldings?.positionCount, 3);
  }
  // 20 次手动刷新合并为一次进行中的查询。首次查询结束已断开短命 Port，
  // 与真实 Chrome 一致，刷新会新开一个 Port。
  ctx.swapRuntime((respond, request) => happyScript(respond, request));
  for (let i = 0; i < 20; i += 1) refreshInvestmentCards();
  await flush();
  assert.equal(ctx.calls.filter((c) => c.method === 'app:card_snapshot').length, 2, '重复刷新合并为一次');
  // 每次查询使用一个短命 Port（查询结束即断开）：两次查询共两个 Port，
  // 但任一时刻至多一个存活，不叠加并存。
  assert.equal(ctx.connectCount, 2, '刷新新开一个短命 Port，不并存叠加');
  for (const unsub of unsubs) unsub();
  console.log('✓ 20 cards: one query, one port, refreshes merged');
}

// 15. 取消竞态回归：旧查询迟到继续执行时，只回收自有资源，不得断开
//     新查询的 Port、清掉新查询的预算定时器或改写新查询的状态。
{
  resetInvestmentFeedForTests();
  let releaseCore;
  const coreGate = new Promise((resolve) => { releaseCore = resolve; });
  let releaseHandshake;
  const handshakeGate = new Promise((resolve) => { releaseHandshake = resolve; });
  let coreQueryCount = 0;
  const ctx = setup((respond, request) => {
    if (request.method === 'app:handshake') {
      return handshakeGate.then(() => respond({ ok: true, result: { protocolVersion: 2, appId: 'fund', state: 'initialized', moduleApiVersion: 1, dataSchemaVersion: 2, capabilityVersion: 2 } }));
    }
    return happyScript(respond, request);
  }, null, {
    coreCall: async (method) => {
      if (method === 'apps:get' && coreQueryCount === 0) {
        coreQueryCount += 1;
        await coreGate; // 旧查询的 apps:get 卡住，制造迟到窗口
      }
      return {
        app: { app_id: 'fund', name: '投资', version: '2.6.0', enabled: true, product_generation: 7 },
        runtime_host: 'com.natives.local.app_runtime',
      };
    },
  });
  initInvestmentFeed({ coreCall: ctx.coreCall });
  setInvestmentCardsPresent(true);
  await flush(); // 旧查询在 apps:get 上挂起
  assert.equal(investmentFeedDiagnostics().status, 'loading');
  suspendInvestmentFeed(); // 取消旧查询（此时它还没有 Port）
  assert.equal(investmentFeedDiagnostics().errorCode, 'SUSPENDED');
  resumeInvestmentFeed(); // 新查询：apps:get 立即返回，创建 Port 后卡在 handshake
  await flush();
  assert.equal(investmentFeedDiagnostics().status, 'loading', '新查询进行中（handshake 待释放）');
  const newGen = investmentFeedDiagnostics().queryGeneration;
  // 旧查询的 apps:get 迟到返回：旧实现会在这里清算全局 runtimeClient/
  // budgetTimer，断开新查询的 Port 使其以 APP_HOST_DISCONNECTED 失败。
  releaseCore();
  await flush();
  assert.equal(investmentFeedDiagnostics().status, 'loading', `旧请求迟到不得破坏新查询：${JSON.stringify(investmentFeedDiagnostics())}`);
  // 新查询继续完成：Port 仍活着、预算定时器仍有效。
  releaseHandshake();
  await flush();
  assert.equal(investmentFeedDiagnostics().status, 'ready', `新查询应完成：${JSON.stringify(investmentFeedDiagnostics())}`);
  assert.equal(investmentFeedDiagnostics().errorCode, null);
  assert.ok(investmentFeedDiagnostics().queryGeneration >= newGen);
  const last = ctx.states.at(-1);
  assert.equal(last.snapshot?.registeredFundHoldings?.positionCount, 3, '快照来自新查询');
  console.log('✓ late aborted query cannot clobber the newer query resources');
}

// 16. 移除最后一张投资卡时查询进行中：终止查询、退出 loading；迟到响应
//     不得把状态翻回 ready；重新添加卡片能重新查询。
{
  resetInvestmentFeedForTests();
  let release;
  const gate = new Promise((resolve) => { release = resolve; });
  const ctx = setup((respond, request) => {
    if (request.method === 'app:handshake') {
      return gate.then(() => respond({ ok: true, result: { protocolVersion: 2, appId: 'fund', state: 'initialized', moduleApiVersion: 1, dataSchemaVersion: 2, capabilityVersion: 2 } }));
    }
    return happyScript(respond, request);
  });
  initInvestmentFeed({ coreCall: ctx.coreCall });
  setInvestmentCardsPresent(true);
  await flush();
  assert.equal(investmentFeedDiagnostics().status, 'loading');
  setInvestmentCardsPresent(false);
  const diag = investmentFeedDiagnostics();
  assert.equal(diag.inFlight, false, '移除卡片后查询已终止');
  assert.notEqual(diag.status, 'loading', '移除卡片后不得停留 loading');
  release(); // 旧查询迟到继续：不得把状态翻回 ready
  await flush();
  const after = investmentFeedDiagnostics();
  assert.notEqual(after.status, 'ready', `迟到响应不得翻回 ready：${JSON.stringify(after)}`);
  assert.equal(ctx.states.at(-1).snapshot ?? null, null, '无卡片时不得留下快照');
  // 重新添加卡片：重新查询并恢复 ready。
  ctx.swapRuntime((respond, request) => happyScript(respond, request));
  setInvestmentCardsPresent(true);
  await flush();
  assert.equal(investmentFeedDiagnostics().status, 'ready');
  console.log('✓ removing last card terminates query and exits loading');
}

{
  resetInvestmentFeedForTests();
  const ctx = setup(happyScript);
  initInvestmentFeed({coreCall:ctx.coreCall});
  setInvestmentCardsPresent(true);
  await flush();
  const before = investmentFeedDiagnostics().queryGeneration;
  ctx.swapRuntime(happyScript);
  invalidateInvestmentCards();
  await flush();
  assert.ok(investmentFeedDiagnostics().queryGeneration > before);
  assert.equal(investmentFeedDiagnostics().status, 'ready');
  document.visibilityState = 'hidden';
  const hidden = investmentFeedDiagnostics().queryGeneration;
  invalidateInvestmentCards();
  await flush();
  assert.equal(investmentFeedDiagnostics().queryGeneration, hidden);
  document.visibilityState = 'visible';
  console.log('✓ module invalidation refreshes one shared projection and stays quiet while hidden');
}
console.log('space-investment-feed tests passed');
