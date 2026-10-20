import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { setupTestDomEnvironment } from './test-dom-mock.js';
import { createAppCenter } from './apps.js';
import { createAppShell, parseAppId, validateNotification, isValidViewToken, viewAllowedForModule } from './app.js';
import { appHostFor } from './native-app-client.js';
import { acquireCapabilityLeases } from './system-capability-leases.js';

setupTestDomEnvironment();
globalThis.chrome = { runtime: {
  getURL: (path) => 'chrome-extension://abcdefghijklmnopabcdefghijklmnop/' + path,
  getManifest: () => ({ version: '0.1.0' }),
} };
const messages = JSON.parse(readFileSync(new URL('./_locales/zh_CN/messages.json', import.meta.url)));
const t = (key, fallback) => messages[key]?.message || fallback || key;
const tick = () => new Promise((resolve) => setTimeout(resolve, 25));
const shells = [];
const makeShell = (options) => { const shell = createAppShell({ t, getPreference: async (key,fallback) => fallback, openSettings: async () => {}, ...options }); shells.push(shell); return shell; };

// Fixed built-in module projection as the Host returns it (plan §3.4): the
// definition comes from the product manifest, never from a catalog.
const FUND_MODULE = {
  appId: 'fund',
  name: { zh_CN: '基金', en: 'Fund' },
  description: { zh_CN: '个人基金记账、持仓与收益', en: 'Personal fund ledger, positions and returns' },
  entryRoute: 'app.html?app=fund',
  present: true, configured: false, enabled: true, showInSidebar: true, sidebarOrder: 0,
};

function clientFixture() {
  const calls = [], apps = [], modules = [JSON.parse(JSON.stringify(FUND_MODULE))];
  let lastClear = null;
  let product = { version: '1.0.0', generation: 1, configured: true, sourcePresent: true, sourceVersion: '1.0.0' };
  modules[0].configured = true;
  return {
    calls, apps, modules,
    get lastClear() { return lastClear; },
    get product() { return product; },
    disconnected: false,
    disconnect() { this.disconnected = true; },
    async call(method, params = {}) {
      calls.push({ method, params });
      if (method === 'apps:handshake') return { platform: 'darwin', arch: 'arm64', version: '0.1.0', appsProtocolVersion: 5 };
      if (method === 'apps:list') return {
        apps: [...apps], modules: JSON.parse(JSON.stringify(modules)),
        product: { ...product }, pendingDataResets: [],
        revision: calls.length,
      };
      if (method === 'apps:clear_data') {
        // Host contract (plan §4.3): confirmation + requestId + scope flags.
        assert.equal(params.confirmPurge, true);
        assert.ok(params.requestId?.startsWith(`${params.appId}:`), 'requestId is app-scoped and unique');
        lastClear = { ...params };
        const index = apps.findIndex((entry) => entry.app_id === params.appId);
        if (index >= 0) apps.splice(index, 1);
        return { receipt: { request_id: params.requestId, app_id: params.appId, state: 'completed', cleared: ['data', 'imports', 'cache', 'logs'], data_preserved: true }, revision: calls.length };
      }
      return {};
    },
  };
}
function makeDom() {
  document.body.replaceChildren();
  for (const [id, tag] of [['apps-list', 'div'], ['apps-toast', 'div'], ['nav-back', 'button']]) {
    const element = document.createElement(tag); element.id = id; document.body.append(element);
  }
  return { list: document.getElementById('apps-list'), toast: document.getElementById('apps-toast') };
}

// Direction check 1 (plan §5): the center only handshakes and reads the
// fixed module list is returned by the Host, and a failed handshake stops
// every dependent call.
{
  const dom = makeDom(), client = clientFixture();
  const call = client.call.bind(client);
  client.call = (method, params) => method === 'apps:handshake'
    ? Promise.resolve({ version: '0.1.0' }) : call(method, params);
  const center = createAppCenter({ t, client, openSettings: async () => {} });
  await center.ready;
  assert.equal(center.state.error, 'appsNeedsUpdate', 'incompatible Host error must be preserved');
  assert.ok(dom.list.textContent.includes(t('appsNeedsUpdate')));
  assert.ok(!client.calls.some((c) => c.method === 'apps:list'), 'failed handshake must stop dependent calls');
  center.dispose();
}
// bfcache: hidden page keeps no connection; restore re-handshakes and reads.
{
  makeDom();
  globalThis.window = new EventTarget();
  const client = clientFixture();
  const center = createAppCenter({ t, client, openSettings: async () => {} });
  await center.ready;
  window.dispatchEvent(new Event('pagehide'));
  const before = client.calls.length;
  await center.refresh();
  assert.equal(client.calls.length, before, 'hidden cached page must not reconnect');
  const restored = new Event('pageshow');
  Object.defineProperty(restored, 'persisted', { value: true });
  window.dispatchEvent(restored);
  await tick();
  assert.ok(client.calls.length > before, 'restored page must re-handshake and refresh');
  assert.ok(client.calls.slice(before).some((c) => c.method === 'apps:handshake'));
  center.dispose();
  delete globalThis.window;
}
// Direction check 2 (plan §5): an empty preference table still shows the
// built-in fund module, with no module distribution semantics.
{
  const dom = makeDom(), client = clientFixture();
  const center = createAppCenter({ t, client, openSettings: async () => {} });
  await center.ready;
  assert.equal(dom.list.querySelectorAll('.app-card').length, 1, 'empty preference table still shows fund');
  assert.ok(dom.list.textContent.includes('基金'));
  assert.ok(dom.list.textContent.includes('个人基金记账、持仓与收益'));
  // Product configuration is completed by the first verified Host handshake;
  // the center contains no setup/install action.
  assert.ok(dom.list.textContent.includes(t('appsReady')));
  center.dispose();
  assert.ok(client.disconnected);
}
// Data management (plan §4.3): double confirmation, credentials opt-in,
// apps:clear_data with requestId.
{
  const dom = makeDom(), client = clientFixture();
  client.apps.push({ app_id: 'fund', name: '基金', version: '0.1.0', enabled: true, show_in_sidebar: true, host_registered: true });
  const center = createAppCenter({ t, client, openSettings: async () => {} });
  await center.ready;
  const pending = dom.list.querySelector('.app-card .action.danger').onclick();
  await tick();
  let dialog = document.querySelector('.apps-dialog');
  assert.ok(dialog.textContent.includes(t('appsClearDataScope')), 'confirmation lists the actual scope');
  const credentials = dialog.querySelector('input');
  assert.ok(credentials && !credentials.checked, 'credentials are kept by default');
  credentials.checked = true; credentials.onchange();
  dialog.querySelector('[data-role="ok"]').onclick();
  await tick();
  assert.ok(!client.calls.some((c) => c.method === 'apps:clear_data'), 'first confirmation alone cannot purge');
  dialog = document.querySelector('.apps-dialog');
  dialog.querySelector('[data-role="cancel"]').onclick();
  await pending;
  assert.equal(client.apps.length, 1, 'second confirmation cancellation preserves the module');

  const confirmed = dom.list.querySelector('.app-card .action.danger').onclick();
  await tick();
  dialog = document.querySelector('.apps-dialog');
  dialog.querySelector('[data-role="ok"]').onclick();
  await tick();
  dialog = document.querySelector('.apps-dialog');
  dialog.querySelector('[data-role="ok"]').onclick();
  await confirmed;
  assert.ok(client.lastClear, 'confirmed flow reaches apps:clear_data');
  assert.equal(client.lastClear.appId, 'fund');
  assert.equal(client.lastClear.deleteCredentials, false, 'credentials kept unless separately confirmed');
  assert.equal(client.apps.length, 0);
  center.dispose();
}

// ─── Built-in module owner surface ──────────────────────────────────────────

const MODULE_DETAIL = {
  runtime_host: 'com.natives.local.app_runtime',
  app: {
    app_id: 'sample',
    name: 'Sample Module',
    version: '2.0.0',
    enabled: true,
    show_in_sidebar: true,
    sidebar_order: 0,
    capabilities: [],
    requires: [],
    product_generation: 1,
    updated_at: 1,
    revision: 1,
  },
};

function makeSurfaceDom() {
  document.body.replaceChildren();
  const stage = document.createElement('main');
  stage.id = 'app-stage';
  const title = document.createElement('h1');
  title.id = 'app-title';
  const sub = document.createElement('span');
  sub.id = 'app-sub';
  const toast = document.createElement('div');
  toast.id = 'app-toast';
  const back = document.createElement('button');
  back.id = 'app-back';
  document.body.append(stage, title, sub, toast, back);
  return { stage, title, sub, toast, back };
}

function surfaceClient(detail) {
  const calls = [];
  let disconnected = 0;
  return {
    calls,
    get disconnected() { return disconnected; },
    async call(method, params = {}) {
      calls.push({ method, params });
      if (method === 'apps:get') {
        assert.equal(typeof params.appId, 'string', 'apps:get uses the Host appId contract');
        if (!detail || detail.app.app_id !== params.appId) return { app: null };
        return detail;
      }
      return {};
    },
    disconnect() { disconnected += 1; },
  };
}

function fakeHostClient() {
  const calls = [];
  return {
    calls,
    async call(method, params = {}) {
      calls.push({ method, params });
      if (method === 'app:handshake') return { protocolVersion: 2, appId: 'sample', appVersion: '2.0.0' };
      if (method === 'app:start') return { state: 'ready', port: 49152, instanceId: 'i'.repeat(22), generation: 'g'.repeat(22) };
      if (method === 'app:session' && params.op === 'rotate') return { newGeneration: 'n'.repeat(22) };
      if (method === 'app:session' && params.op === 'issue') return { generation: 'n'.repeat(22), token: 't'.repeat(43), expiresAt: 900 };
      if (method === 'app:stop') return { stopped: true };
      return {};
    },
    disconnect() {},
  };
}

// A6.1 parseAppId + host binding is host-agnostic (app.js never names a host)
assert.equal(parseAppId('?app=com.natives.app.demo'), 'com.natives.app.demo');
assert.equal(parseAppId('app=fund'), 'fund');
assert.equal(parseAppId('?other=x'), '');
assert.equal(parseAppId(''), '');
assert.equal(appHostFor(MODULE_DETAIL.runtime_host), 'com.natives.local.app_runtime', 'module uses the product runtime Host');
assert.throws(() => appHostFor(), /runtime host is missing/, 'missing product Host has no fallback');
assert.deepEqual(validateNotification({ title: 'T', body: 'B', tag: 'tag', badge: { text: '1', tone: 'info' } }).badge, { text: '1', tone: 'info' });
assert.equal(validateNotification({ title: '<b>', body: '', tag: '', badge: { text: '', tone: 'info' } }), null, 'notification rejects markup');
assert.throws(() => acquireCapabilityLeases(['unknown.required']), /unsupported required capability/);

// A6.2 configured + enabled → the generic owner opens the module Host
{
  const domA = makeSurfaceDom();
  const hostA = fakeHostClient();
  const shellA = makeShell({
    appId: 'sample',
    stage: domA.stage,
    getNativeClient: () => surfaceClient(MODULE_DETAIL),
    createHostClient: () => hostA,
  });
  await shellA.open();
  await tick();
  assert.equal(domA.title.textContent, 'Sample Module', 'Surface 标题来自权威模块投影');
  const iframe = domA.stage.querySelector('iframe');
  assert.ok(iframe, '通用壳创建应用 sandbox');
  assert.equal(iframe.getAttribute('sandbox'), 'allow-scripts allow-forms');
  assert.equal(iframe.src, 'http://127.0.0.1:49152/?theme=archive', '建帧 URL 携带首帧主题直传参数');
  assert.ok(hostA.calls.some((c) => c.method === 'app:start'), '通用壳直属 App Host');
  await shellA.open(); // retry path: idempotent remount must not throw
}

// A6.3 another product-declared module opens without an Extension registry entry
{
  const domB = makeSurfaceDom();
  let hostCreated = 0;
  const detailB = { ...MODULE_DETAIL, app: { ...MODULE_DETAIL.app, app_id: 'portfolio' } };
  const shellB = makeShell({
    appId: 'portfolio',
    stage: domB.stage,
    getNativeClient: () => surfaceClient(detailB),
    createHostClient: () => { hostCreated += 1; const client = fakeHostClient();
      const call = client.call.bind(client); client.call = (method, params) => method === 'app:handshake'
        ? Promise.resolve({ protocolVersion: 2, appId: 'portfolio', appVersion: '2.0.0' }) : call(method, params); return client; },
  });
  await shellB.open();
  assert.equal(hostCreated, 1, '另一内置模块使用同一通用承载链路');
  assert.ok(domB.stage.querySelector('iframe'), '另一内置模块无需 Extension 内置业务 UI');
}

// A6.4 disabled app → 已停用 state, no host port
{
  const domC = makeSurfaceDom();
  let hostCreated = 0;
  const detailC = { ...MODULE_DETAIL, app: { ...MODULE_DETAIL.app, enabled: false } };
  const shellC = makeShell({
    appId: 'sample',
    stage: domC.stage,
    getNativeClient: () => surfaceClient(detailC),
    createHostClient: () => { hostCreated += 1; return fakeHostClient(); },
  });
  await shellC.open();
  assert.ok(domC.stage.textContent.includes('已停用'), '停用应用必须显示停用状态');
  assert.equal(hostCreated, 0, '停用应用不得打开 app host');
}

// A6.5 unknown built-in module id → apps:get empty → 模块不存在
{
  const domD = makeSurfaceDom();
  const shellD = makeShell({
    appId: 'com.natives.app.gone',
    stage: domD.stage,
    getNativeClient: () => surfaceClient(null),
    createHostClient: () => fakeHostClient(),
  });
  await shellD.open();
  assert.ok(domD.stage.textContent.includes('无法打开'), '未知模块必须显示错误状态');
  assert.ok(domD.toast.textContent.length > 0, '错误必须进 toast');
}

// A6.6 missing product runtime host is not a production fallback
{
  const domE = makeSurfaceDom();
  let hostCreated = 0;
  const fundDetail = {
    app: {
      app_id: 'fund', name: '基金', version: '0.1.0', enabled: true,
      capabilities: [], requires: [], product_generation: 1,
    },
  };
  const shellE = makeShell({
    appId: 'fund',
    stage: domE.stage,
    getNativeClient: () => surfaceClient(fundDetail),
    createHostClient: () => { hostCreated += 1; return fakeHostClient(); },
  });
  await shellE.open();
  await tick();
  assert.ok(domE.stage.textContent.includes('无法打开'));
  assert.equal(hostCreated, 0, 'missing product Host must not open a Native Port');
}

// A6.7 static business registry and modules are gone
{
  const appJs = readFileSync(new URL('./app.js', import.meta.url), 'utf8');
  assert.ok(!appJs.includes('app-module-registry'));
  assert.ok(!appJs.includes('demo-ui'));
  assert.ok(!appJs.includes('fund-ui'));
}

// A6.8 app.html declares the Surface containers
{
  const appHtml = readFileSync(new URL('./app.html', import.meta.url), 'utf8');
  for (const id of ['app-stage', 'app-title', 'app-toast', 'app-back']) {
    assert.ok(appHtml.includes(`id="${id}"`), `app.html 必须声明 #${id}`);
  }
  // mode/T06：页面装配入口为 app-boot.js（注入 openSettings），其导入 app.js。
  assert.ok(appHtml.includes('app-boot.js'), 'app.html 必须加载装配入口 app-boot.js');
}

// A6.9 locales: A6 keys exist in both languages (sync requirement)
{
  const keys = [
    'appSurfaceOpenFailed', 'appSurfaceDisabled', 'appSurfaceDisabledBody',
    'appSurfaceHostOffline', 'fundEmpty',
  ];
  for (const loc of ['zh_CN', 'en']) {
    const messages = JSON.parse(readFileSync(new URL(`./_locales/${loc}/messages.json`, import.meta.url), 'utf8'));
    for (const key of keys) {
      assert.equal(typeof messages[key]?.message, 'string', `${loc} 缺少 ${key}`);
    }
  }
}

// A6.10 ADR-0038 深链视图白名单：声明非空 views 的模块严格放行；
// 未声明的旧模块回退 ADR-0036 短 token 正则；非法 token 无论声明与否拒绝。
{
  assert.equal(isValidViewToken('positions'), true);
  assert.equal(isValidViewToken('Positions'), false);
  assert.equal(isValidViewToken(''), false);
  assert.equal(isValidViewToken('a'.repeat(33)), false);

  const fundViews = ['home', 'positions', 'assets', 'trends', 'settings'];
  assert.equal(viewAllowedForModule('positions', fundViews), true);
  assert.equal(viewAllowedForModule('settings', fundViews), true);
  assert.equal(viewAllowedForModule('watchlist', fundViews), false, '未声明视图必须拒绝');
  assert.equal(viewAllowedForModule('../../etc', fundViews), false, '非法 token 即使在白名单语法外也拒绝');
  // 旧模块（无声明）回退正则：任意短 token 通过，形状外拒绝。
  assert.equal(viewAllowedForModule('anything', []), true);
  assert.equal(viewAllowedForModule('anything', undefined), true);
  assert.equal(viewAllowedForModule('Bad Token', []), false);

  // 壳接线：apps:get 投影的 views 进入白名单，且视图消费点不再直接用裸正则。
  const appJs = readFileSync(new URL('./app.js', import.meta.url), 'utf8');
  assert.ok(appJs.includes('app.views'), '壳必须读取 apps:get 的 views 投影');
  assert.ok(!appJs.includes('isValidViewToken(initialView)') && !appJs.includes('view: isValidViewToken'),
    'welcome/navigate 必须走白名单校验，不再直接用裸正则');
  assert.ok(appJs.includes('viewAllowedForModule(value, declaredViews)'), '白名单由壳统一执行');
}

console.log('apps: built-in module projection, handshake-stop, data management and App Surface checks passed');

for (const shell of shells) shell.dispose();
