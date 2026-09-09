import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { setupTestDomEnvironment } from './test-dom-mock.js';
import { createAppCenter } from './apps.js';
import { createAppShell, parseAppId } from './app.js';
import { APP_UI_MODULES, isKnownUiModule } from './app-module-registry.js';
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
const demo = {
  app_id: 'com.natives.app.demo', name: 'Demo', version: '1.0.0', icon: 'grid', permissions: [],
  runtime_spec: {}, surface: { icon: 'grid' },
  packages: [{ package_id: 'demo-data', kind: 'data', version: '1.0.0', platform: 'any', arch: 'any',
    wire_size: 10, payload_size: 10, artifact_sha256: 'a'.repeat(64), payload_sha256: 'b'.repeat(64), required: true,
    url: 'https://github.com/Wcof/Natives/releases/download/apps-demo-v1.0.0/demo.nap' }],
};
const catalog = { catalogVersion: 2, apps: [demo, { app_id: 'fund', name: '基金', version: '0.1.0', published: false, packages: [] }] };
function clientFixture({ failCommit = false } = {}) {
  const calls = [], apps = [];
  let request, staged = false;
  return { calls, apps, disconnected: false,
    disconnect() { this.disconnected = true; },
    async call(method, params = {}) {
      calls.push({ method, params });
      if (method === 'apps:handshake') return { platform: 'darwin', arch: 'arm64', version: '0.1.0', appsProtocolVersion: 3 };
      if (method === 'apps:list') return { apps: [...apps], revision: calls.length, retainedData: [] };
      if (method === 'apps:install_begin') {
        request = JSON.parse(Buffer.from(params.request, 'base64'));
        assert.equal(request.packages.length, 1);
        assert.equal(request.packages[0].url, undefined, 'Host receives metadata, never a download path');
        staged = false;
        return { install_id: 'tx-1' };
      }
      if (method === 'apps:install_package') { assert.equal(params.installId, 'tx-1'); assert.ok(params.data); staged = true; }
      if (method === 'apps:install_commit') {
        assert.ok(staged, 'commit must follow package transfer');
        if (failCommit) throw Object.assign(new Error('private backend detail'), { code: 'APP_INVALID_STATE' });
        const app = { ...request.app, host_registered: false, runtime_spec_json: JSON.stringify(request.app.runtime_spec) };
        const index = apps.findIndex((entry) => entry.app_id === app.app_id);
        if (index < 0) apps.push(app); else apps[index] = app;
        return app;
      }
      if (method === 'apps:uninstall') apps.splice(apps.findIndex((entry) => entry.app_id === params.appId), 1);
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
const transfer = async (_, { onProgress }) => { onProgress?.(10); return 'cGF5bG9hZA=='; };
{
  const dom = makeDom(), client = clientFixture();
  const call = client.call.bind(client);
  client.call = (method, params) => method === 'apps:handshake'
    ? Promise.resolve({ version: '0.1.0' }) : call(method, params);
  const center = createAppCenter({ t, client, catalogLoader: async () => catalog });
  await center.ready;
  assert.equal(center.state.error, 'appsNeedsUpdate', 'list refresh must preserve the incompatible Host error');
  assert.ok(dom.list.textContent.includes(t('appsNeedsUpdate')));
  center.dispose();
}
{
  makeDom();
  globalThis.window = new EventTarget();
  const client = clientFixture();
  let loads = 0;
  const center = createAppCenter({ t, client, catalogLoader: async () => { loads++; return catalog; } });
  await center.ready;
  window.dispatchEvent(new Event('pagehide'));
  const before = client.calls.length;
  await center.refresh();
  assert.equal(client.calls.length, before, 'hidden cached page must not reconnect');
  const restored = new Event('pageshow');
  Object.defineProperty(restored, 'persisted', { value: true });
  window.dispatchEvent(restored);
  await tick();
  assert.equal(loads, 2, 'restored page must reload its verified catalog');
  assert.ok(client.calls.length > before, 'restored page must refresh the Host registry');
  center.dispose();
  delete globalThis.window;
}
{
  const dom = makeDom(), client = clientFixture();
  const center = createAppCenter({ t, client, catalogLoader: async () => catalog, packageTransfer: transfer });
  await center.ready;
  assert.equal(dom.list.querySelectorAll('.app-card').length, 2);
  assert.equal(dom.list.querySelectorAll('.action.primary').length, 1, 'unreleased Fund must not be installable');
  await center.install(demo);
  assert.deepEqual(client.calls.filter((call) => call.method.startsWith('apps:install_')).map((call) => call.method),
    ['apps:install_begin', 'apps:install_package', 'apps:install_commit']);
  assert.equal(client.apps.length, 1);
  const next = { ...demo, version: '1.1.0', packages: demo.packages.map((pkg) => ({ ...pkg, version: '1.1.0' })) };
  await center.install(next);
  assert.equal(client.apps[0].version, '1.1.0');
  assert.ok(!client.calls.some((call) => call.method === 'apps:uninstall'), 'update must never uninstall the old version first');
  await center.uninstall(demo.app_id, { purgeData: true, confirmPurge: true });
  const uninstall = client.calls.find((call) => call.method === 'apps:uninstall');
  assert.deepEqual(uninstall.params, { appId: demo.app_id, purgeData: true, confirmPurge: true });
  center.dispose();
  assert.ok(client.disconnected);
}
{
  const dom = makeDom(), client = clientFixture({ failCommit: true });
  const center = createAppCenter({ t, client, catalogLoader: async () => catalog, packageTransfer: transfer });
  await center.ready;
  await assert.rejects(center.install(demo));
  assert.ok(client.calls.some((call) => call.method === 'apps:install_abort'));
  assert.ok(dom.list.textContent.includes(t('appsOperationFailed')));
  assert.ok(!dom.list.textContent.includes('private backend detail'));
  assert.equal(client.apps.length, 0);
  center.dispose();
}
{
  const dom = makeDom(), client = clientFixture();
  let retries = 0;
  const center = createAppCenter({ t, client, catalogLoader: async () => {
    if (!retries++) throw Object.assign(new Error('offline'), { code: 'APP_NETWORK' });
    return catalog;
  } });
  await center.ready;
  assert.ok(dom.list.textContent.includes(t('appsNetworkError')));
  assert.ok(dom.list.textContent.includes(t('retry')));
  await center.reloadCatalog();
  assert.equal(center.state.catalogError, null);
  assert.equal(dom.list.querySelectorAll('.app-card').length, 2);
  center.dispose();
}
{
  const dom = makeDom(), client = clientFixture();
  client.apps.push({ ...demo, enabled: true, show_in_sidebar: true, host_registered: true });
  const center = createAppCenter({ t, client, catalogLoader: async () => catalog });
  await center.ready;
  const pending = dom.list.querySelector('.app-card .action.danger').onclick();
  await tick();
  let dialog = document.querySelector('.apps-dialog');
  const checkbox = dialog.querySelector('input');
  assert.ok(checkbox && !checkbox.checked, 'uninstall preserves data by default');
  checkbox.checked = true; checkbox.onchange();
  dialog.querySelector('[data-role="ok"]').onclick();
  await tick();
  assert.ok(!client.calls.some((call) => call.method === 'apps:uninstall'), 'first confirmation cannot purge');
  dialog = document.querySelector('.apps-dialog');
  dialog.querySelector('[data-role="cancel"]').onclick();
  await pending;
  assert.equal(client.apps.length, 1, 'second confirmation cancellation preserves the app');
  center.dispose();
}

// ─── Phase A6: App Surface (ADR-0025 D50) ───────────────────────────────────

const DEMO_DETAIL = {
  app: {
    app_id: 'com.natives.app.demo',
    kind: 'extension_app',
    name: 'Demo',
    version: '2.0.0',
    enabled: true,
    host_registered: false,
    show_in_sidebar: true,
    sidebar_order: 0,
    runtime_spec_json: '{"version":"2.0.0"}',
    surface_json: '{"icon":"grid","route":"app.html?app=com.natives.app.demo"}',
    manifest_json: '{}',
    installed_at: 1,
    updated_at: 1,
    revision: 1,
  },
  packages: [{ package_id: 'demo-data' }, { package_id: 'demo-image' }],
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
      if (method === 'apps:read_resource') {
        const bytes = params.packageId === 'demo-image'
          ? Buffer.from('89504e470d0a1a0a', 'hex')
          : Buffer.from('{"demo":"中文"}');
        return {
          ok: true,
          app_id: params.appId,
          package_id: params.packageId,
          version: '2.0.0',
          format: params.packageId === 'demo-image' ? 'png' : 'json',
          total_size: bytes.length,
          offset: params.offset,
          length: bytes.length,
          data: bytes.toString('base64'),
        };
      }
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
assert.equal(appHostFor(DEMO_DETAIL.app), 'com.natives.file_manager', 'apps share native-file-host');
assert.equal(appHostFor({ app_id: 'fund' }), 'com.natives.file_manager', 'apps share native-file-host');

// A6.2 installed + enabled → demo-ui mounts and reads resource via shared host
{
  const domA = makeSurfaceDom();
  const hostA = fakeHostClient();
  const shellA = makeShell({
    appId: 'com.natives.app.demo',
    stage: domA.stage,
    getNativeClient: () => surfaceClient(DEMO_DETAIL),
    createHostClient: () => hostA,
  });
  await shellA.open();
  await tick();
  assert.equal(domA.title.textContent, 'Demo', 'Surface 标题来自权威 App 记录');
  const badge = domA.stage.querySelectorAll('#demo-host-status');
  assert.equal(badge.length, 1, 'demo UI 已挂载');
  assert.ok(badge[0].classList.contains('ok'), 'host 在线徽标');
  assert.equal(badge[0].textContent, '在线');
  assert.ok(hostA.calls.some((c) => c.method === 'apps:read_resource'), 'demo-ui 调用 apps:read_resource');
  await shellA.open(); // retry path: idempotent remount must not throw
}

// A6.3 unknown UI module → "需要更新 Natives" + no host port is opened
{
  const domB = makeSurfaceDom();
  let hostCreated = 0;
  const detailB = { ...DEMO_DETAIL, app: { ...DEMO_DETAIL.app, app_id: 'com.example.mystery' } };
  const shellB = makeShell({
    appId: 'com.example.mystery',
    stage: domB.stage,
    getNativeClient: () => surfaceClient(detailB),
    createHostClient: () => { hostCreated += 1; return fakeHostClient(); },
  });
  await shellB.open();
  assert.ok(domB.stage.textContent.includes('需要更新 Natives'), '未知模块必须提示更新 Natives');
  assert.equal(hostCreated, 0, '未知模块不得打开任何 app host');
  assert.ok(domB.stage.textContent.includes('打开应用中心'), '提供打开应用中心入口');
}

// A6.4 disabled app → 已停用 state, no host port
{
  const domC = makeSurfaceDom();
  let hostCreated = 0;
  const detailC = { ...DEMO_DETAIL, app: { ...DEMO_DETAIL.app, enabled: false } };
  const shellC = makeShell({
    appId: 'com.natives.app.demo',
    stage: domC.stage,
    getNativeClient: () => surfaceClient(detailC),
    createHostClient: () => { hostCreated += 1; return fakeHostClient(); },
  });
  await shellC.open();
  assert.ok(domC.stage.textContent.includes('应用已停用'), '停用应用必须显示停用状态');
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
  assert.ok(domD.stage.textContent.includes('应用无法打开'), '未安装应用必须显示错误状态');
  assert.ok(domD.toast.textContent.length > 0, '错误必须进 toast');
}

// A6.6 fund placeholder: mounted, zero host traffic
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
  assert.ok(domE.stage.textContent.includes('基金应用已安装'), 'explicit zero-package apps mount without external resources');
  assert.equal(hostCreated, 0, 'zero-package fund app must not open a Host port');
}

// A6.7 D2: registry is build-time only — no remote code URL anywhere
{
  assert.ok(isKnownUiModule('com.natives.app.demo'), 'demo 模块必须 build-time 存在');
  assert.ok(!isKnownUiModule('com.example.mystery'), '目录之外的 appId 必须未知');
  const appJs = readFileSync(new URL('./app.js', import.meta.url), 'utf8');
  for (const file of Object.values(APP_UI_MODULES)) {
    const src = readFileSync(new URL(`./${file}`, import.meta.url), 'utf8');
    assert.ok(src.includes('export function mountApp'), `${file} 必须导出 mountApp`);
  }
  // D2: every module path is a build-time RELATIVE path — no remote code URL.
  for (const [appId, path] of Object.entries(APP_UI_MODULES)) {
    assert.ok(path.startsWith('./apps/'), `${appId} 的模块路径必须是包内相对路径`);
    assert.ok(!path.includes('http'), `${appId} 的模块路径不得是远程 URL（D2）`);
  }
  assert.ok(!appJs.includes('com.natives.app.demo'), 'app.js 不得点名 demo host（D50）');
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

console.log('apps: Gate A4+A6 checks passed (catalog items, install cycle, D42, conflict, uninstall confirm, settings entry, locales, App Surface, demo host, unknown module)');

for (const shell of shells) shell.dispose();
