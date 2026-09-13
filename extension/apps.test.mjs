import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { setupTestDomEnvironment } from './test-dom-mock.js';
import { createAppCenter } from './apps.js';
import { createAppShell, parseAppId } from './app.js';
import { appHostFor } from './native-app-client.js';

setupTestDomEnvironment();
globalThis.chrome = { runtime: {
  getURL: (path) => 'chrome-extension://abcdefghijklmnopabcdefghijklmnop/' + path,
  getManifest: () => ({ version: '0.1.0' }),
} };
const messages = JSON.parse(readFileSync(new URL('./_locales/zh_CN/messages.json', import.meta.url)));
const t = (key, fallback) => messages[key]?.message || fallback || key;
const tick = () => new Promise((resolve) => setTimeout(resolve, 25));
const shells = [];
const makeShell = (options) => { const shell = createAppShell({ t, ...options }); shells.push(shell); return shell; };

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
  let product = { version: '', generation: 0, configured: false, sourcePresent: true, sourceVersion: '1.0.0' };
  return {
    calls, apps, modules,
    get lastClear() { return lastClear; },
    get product() { return product; },
    disconnected: false,
    disconnect() { this.disconnected = true; },
    async call(method, params = {}) {
      calls.push({ method, params });
      if (method === 'apps:handshake') return { platform: 'darwin', arch: 'arm64', version: '0.1.0', appsProtocolVersion: 4 };
      if (method === 'apps:list') return {
        apps: [...apps], modules: JSON.parse(JSON.stringify(modules)),
        product: { ...product }, pendingDataResets: [],
        revision: calls.length, retainedData: [],
      };
      if (method === 'apps:product_configure') {
        // Host contract (plan §3.3): one transaction prepares every fixed
        // module for this user and binds one productGeneration.
        product = { version: '1.0.0', generation: 1, configured: true, sourcePresent: true, sourceVersion: '1.0.0' };
        modules[0].configured = true;
        if (!apps.some((entry) => entry.app_id === 'fund')) {
          apps.push({ app_id: 'fund', name: '基金', version: '1.0.0', enabled: true, show_in_sidebar: true, host_registered: true });
        }
        return { ...product };
      }
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
// fixed module list — catalog loads and suite_prepare calls are zero, and
// a failed handshake stops every dependent call.
{
  const dom = makeDom(), client = clientFixture();
  const call = client.call.bind(client);
  client.call = (method, params) => method === 'apps:handshake'
    ? Promise.resolve({ version: '0.1.0' }) : call(method, params);
  const center = createAppCenter({ t, client });
  await center.ready;
  assert.equal(center.state.error, 'appsNeedsUpdate', 'incompatible Host error must be preserved');
  assert.ok(dom.list.textContent.includes(t('appsNeedsUpdate')));
  assert.ok(!client.calls.some((c) => c.method === 'apps:list'), 'failed handshake must stop dependent calls');
  assert.ok(!client.calls.some((c) => c.method === 'apps:suite_prepare'));
  center.dispose();
}
// bfcache: hidden page keeps no connection; restore re-handshakes and reads.
{
  makeDom();
  globalThis.window = new EventTarget();
  const client = clientFixture();
  const center = createAppCenter({ t, client });
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
// Direction check 2 (plan §5): an empty install table still shows the
// built-in fund module, with zero install/uninstall semantics.
{
  const dom = makeDom(), client = clientFixture();
  const center = createAppCenter({ t, client });
  await center.ready;
  assert.equal(dom.list.querySelectorAll('.app-card').length, 1, 'empty install table still shows fund');
  assert.ok(dom.list.textContent.includes('基金'));
  assert.ok(dom.list.textContent.includes('个人基金记账、持仓与收益'));
  assert.ok(!dom.list.textContent.includes(t('appsInstall')));
  assert.ok(!dom.list.textContent.includes(t('appsUninstall')));
  assert.ok(!dom.list.textContent.includes(t('appsUpdate')));
  assert.equal(client.calls.filter((c) => c.method === 'apps:suite_prepare').length, 0);
  assert.ok(!client.calls.some((c) => c.method.startsWith('apps:install_')));
  // The explicit product setup bar is shown; configuration never happens
  // as an implicit side effect of loading or opening (plan §3.3).
  assert.ok(dom.list.textContent.includes(t('appsProductConfigure')));
  assert.ok(dom.list.textContent.includes(t('appsProductConfigRequired')));
  assert.equal(client.calls.filter((c) => c.method === 'apps:product_configure').length, 0);
  center.dispose();
  assert.ok(client.disconnected);
}
// Product setup (plan §3.3): the explicit action configures every fixed
// module once; afterwards the center shows the ready module without it.
{
  const dom = makeDom(), client = clientFixture();
  const center = createAppCenter({ t, client });
  await center.ready;
  const setupButton = [...dom.list.querySelectorAll('.action.primary')]
    .find((button) => button.textContent === t('appsProductConfigure'));
  assert.ok(setupButton, 'product setup action is offered');
  setupButton.onclick();
  await tick();
  await tick();
  assert.equal(client.calls.filter((c) => c.method === 'apps:product_configure').length, 1);
  assert.equal(center.state.product.configured, true);
  assert.ok(!dom.list.textContent.includes(t('appsProductConfigure')), 'setup bar disappears once configured');
  assert.ok(dom.list.textContent.includes(t('appsReady')), 'fund card is ready after configuration');
  center.dispose();
}
// Data management (plan §4.3): double confirmation, credentials opt-in,
// apps:clear_data with requestId — and no uninstall method anywhere.
{
  const dom = makeDom(), client = clientFixture();
  client.apps.push({ app_id: 'fund', name: '基金', version: '0.1.0', enabled: true, show_in_sidebar: true, host_registered: true });
  const center = createAppCenter({ t, client });
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
  assert.ok(!client.calls.some((c) => c.method === 'apps:uninstall'), 'uninstall is retired, not an alias');
  center.dispose();
}

// ─── Phase A6: App Surface (ADR-0025 D50) ───────────────────────────────────

const DEMO_DETAIL = {
  app: {
    app_id: 'sample',
    kind: 'managed_local',
    name: 'Demo',
    version: '2.0.0',
    enabled: true,
    host_registered: true,
    runtime_host: 'com.natives.app.hash',
    show_in_sidebar: true,
    sidebar_order: 0,
    runtime_spec_json: '{"version":"2.0.0"}',
    surface_json: '{"icon":"grid","route":"app.html?app=sample"}',
    manifest_json: '{}',
    installed_at: 1,
    updated_at: 1,
    revision: 1,
  },
  packages: [{ package_id: 'app-exec' }],
  permissions: [],
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
      if (method === 'app:handshake') return { protocolVersion: 1, appId: 'sample', appVersion: '2.0.0' };
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
assert.equal(appHostFor(DEMO_DETAIL.app), 'com.natives.app.hash', 'managed app uses its registered host');
assert.equal(appHostFor({ app_id: 'fund' }), 'com.natives.file_manager', 'apps share native-file-host');

// A6.2 installed + enabled → demo-ui mounts and reads resource via shared host
{
  const domA = makeSurfaceDom();
  const hostA = fakeHostClient();
  const shellA = makeShell({
    appId: 'sample',
    stage: domA.stage,
    getNativeClient: () => surfaceClient(DEMO_DETAIL),
    createHostClient: () => hostA,
  });
  await shellA.open();
  await tick();
  assert.equal(domA.title.textContent, 'Demo', 'Surface 标题来自权威 App 记录');
  const iframe = domA.stage.querySelector('iframe');
  assert.ok(iframe, '通用壳创建应用 sandbox');
  assert.equal(iframe.getAttribute('sandbox'), 'allow-scripts allow-forms');
  assert.equal(iframe.src, 'http://127.0.0.1:49152/');
  assert.ok(hostA.calls.some((c) => c.method === 'app:start'), '通用壳直属 App Host');
  await shellA.open(); // retry path: idempotent remount must not throw
}

// A6.3 a new appId opens without an extension registry entry
{
  const domB = makeSurfaceDom();
  let hostCreated = 0;
  const detailB = { ...DEMO_DETAIL, app: { ...DEMO_DETAIL.app, app_id: 'com.example.mystery' } };
  const shellB = makeShell({
    appId: 'com.example.mystery',
    stage: domB.stage,
    getNativeClient: () => surfaceClient(detailB),
    createHostClient: () => { hostCreated += 1; const client = fakeHostClient();
      const call = client.call.bind(client); client.call = (method, params) => method === 'app:handshake'
        ? Promise.resolve({ protocolVersion: 1, appId: 'com.example.mystery', appVersion: '2.0.0' }) : call(method, params); return client; },
  });
  await shellB.open();
  assert.equal(hostCreated, 1, '新 appId 使用同一通用承载链路');
  assert.ok(domB.stage.querySelector('iframe'), '新 appId 无需扩展内置 UI');
}

// A6.4 disabled app → 已停用 state, no host port
{
  const domC = makeSurfaceDom();
  let hostCreated = 0;
  const detailC = { ...DEMO_DETAIL, app: { ...DEMO_DETAIL.app, enabled: false } };
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

// A6.5 not installed / unknown app id → apps:get empty → 应用不存在
{
  const domD = makeSurfaceDom();
  const shellD = makeShell({
    appId: 'com.natives.app.gone',
    stage: domD.stage,
    getNativeClient: () => surfaceClient(null),
    createHostClient: () => fakeHostClient(),
  });
  await shellD.open();
  assert.ok(domD.stage.textContent.includes('无法打开'), '未安装应用必须显示错误状态');
  assert.ok(domD.toast.textContent.length > 0, '错误必须进 toast');
}

// A6.6 legacy extension_app placeholders are not a production fallback
{
  const domE = makeSurfaceDom();
  let hostCreated = 0;
  const fundDetail = {
    app: {
      app_id: 'fund', kind: 'extension_app', name: '基金', version: '0.1.0',
      enabled: true, runtime_spec_json: '{"version":"0.1.0"}',
    },
    packages: [],
    permissions: [],
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
  assert.equal(hostCreated, 0, 'legacy placeholder must not open a Host port');
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
  assert.ok(appHtml.includes('app.js'), 'app.html 必须加载 app.js');
}

// A6.9 locales: A6 keys exist in both languages (sync requirement)
{
  const keys = [
    'appSurfaceNotFound', 'appSurfaceOpenFailed', 'appSurfaceNeedsUpdate',
    'appSurfaceNeedsUpdateBody', 'appSurfaceDisabled', 'appSurfaceDisabledBody',
    'appSurfaceHostOffline', 'appSurfaceHostOfflineBody', 'appSurfaceOpenCenter',
    'appSurfaceOpenCenterHint', 'demoHostOk', 'demoHostLost', 'demoConnecting',
    'demoVersion', 'demoHost', 'demoPingTitle', 'fundEmpty',
  ];
  for (const loc of ['zh_CN', 'en']) {
    const messages = JSON.parse(readFileSync(new URL(`./_locales/${loc}/messages.json`, import.meta.url), 'utf8'));
    for (const key of keys) {
      assert.equal(typeof messages[key]?.message, 'string', `${loc} 缺少 ${key}`);
    }
  }
}

console.log('apps: built-in module projection, handshake-stop, data management and App Surface checks passed');

for (const shell of shells) shell.dispose();
