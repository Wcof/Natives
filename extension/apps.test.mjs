import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { setupTestDomEnvironment } from './test-dom-mock.js';
import { createAppCenter } from './apps.js';
import { createAppShell, parseAppId } from './app.js';
import { APP_UI_MODULES, isKnownUiModule } from './app-module-registry.js';
import { appHostFor } from './native-app-client.js';

setupTestDomEnvironment();

const t = (key, fallback) => fallback || key;

function fakeNativeClient(overrides = {}) {
  const calls = [];
  const apps = [];
  return {
    calls,
    apps,
    async call(method, params = {}) {
      calls.push({ method, params });
      if (overrides[method]) return overrides[method](params);
      switch (method) {
        case 'apps:list':
          return { apps: [...apps], revision: apps.length + 1 };
        case 'apps:install_begin': {
          const request = JSON.parse(new TextDecoder().decode(Uint8Array.from(atob(params.request), (c) => c.charCodeAt(0))));
          if (apps.some((a) => a.app_id === request.app.app_id)) {
            const error = new Error('app already installed');
            error.code = 'APP_CONFLICT';
            throw error;
          }
          return { install_id: 'tx-1' };
        }
        case 'apps:install_commit': {
          apps.push({
            app_id: 'com.natives.app.demo',
            kind: 'extension_app',
            name: 'Demo',
            version: '0.1.0',
            enabled: true,
            show_in_sidebar: true,
            sidebar_order: 0,
            runtime_spec_json: '{"host":"com.natives.app.demo"}',
            surface_json: '{"icon":"grid","route":"app.html?app=com.natives.app.demo"}',
            manifest_json: '{}',
            installed_at: 1,
            updated_at: 1,
            revision: 1,
          });
          return { app_id: 'com.natives.app.demo', revision: 2 };
        }
        case 'apps:uninstall': {
          const index = apps.findIndex((a) => a.app_id === params.appId);
          if (index < 0) {
            const error = new Error('not found');
            error.code = 'APP_NOT_FOUND';
            throw error;
          }
          apps.splice(index, 1);
          return { app_id: params.appId, revision: 3 };
        }
        default:
          return {};
      }
    },
  };
}

// each scenario gets a fresh body so getElementById binds to this scenario's DOM
function makeDom() {
  document.body.replaceChildren();
  const listEl = document.createElement('div');
  listEl.id = 'apps-list';
  const toastEl = document.createElement('div');
  toastEl.id = 'apps-toast';
  const backEl = document.createElement('button');
  backEl.id = 'nav-back';
  document.body.append(listEl, toastEl, backEl);
  return { listEl, toastEl };
}

const catalog = {
  apps: [
    {
      app_id: 'com.natives.app.demo',
      name: 'Demo',
      version: '0.1.0',
      description: '示例应用',
      icon: 'grid',
      permissions: [],
      wireSize: 0,
    },
    {
      app_id: 'fund',
      name: '基金',
      version: '0.1.0',
      description: '基金资产分析',
      icon: 'box',
      permissions: ['keychain:com.natives.app.fund'],
      wireSize: 0,
    },
  ],
};

const tick = () => new Promise((r) => setTimeout(r, 25));

// 1. Gate A4: catalog + empty registry → every entry renders as an AppCenterItem
const client1 = fakeNativeClient();
const dom1 = makeDom();
createAppCenter({ t, client: client1, catalogLoader: async () => catalog });
await tick();
assert.equal(dom1.listEl.querySelectorAll('.app-card').length, 2, '目录中的每个条目都显示');
assert.equal(dom1.listEl.querySelectorAll('.action.primary').length, 2, '未安装条目提供安装按钮');
assert.ok(dom1.listEl.textContent.includes('未安装'));

// 2. install flow: begin → commit → card flips to installed with 打开 button
const installBtn = dom1.listEl.querySelector('.app-card[data-app-id="com.natives.app.demo"] .action.primary');
installBtn.onclick(new Event('click', { bubbles: true }));
await tick();
assert.ok(client1.calls.some((c) => c.method === 'apps:install_begin'), 'install 必须调用 apps:install_begin');
assert.ok(client1.calls.some((c) => c.method === 'apps:install_commit'), 'install 必须调用 apps:install_commit');
const installedCard = dom1.listEl.querySelector('.app-card[data-app-id="com.natives.app.demo"]');
assert.equal(installedCard.querySelector('.status')?.textContent, '已安装');
assert.ok(installedCard.textContent.includes('打开'), '已安装条目提供打开按钮');
assert.equal(client1.apps.length, 1, 'install 后注册表恰好新增 1 个 App');

// 3. D42: catalog entries do NOT become Apps — only the installed demo is
//    in the native registry, fund is still catalog-only
assert.equal(client1.apps[0].app_id, 'com.natives.app.demo');
assert.ok(!client1.apps.some((a) => a.app_id === 'fund'), '未安装的目录条目不得进入注册表');

// 4. APP_CONFLICT: installing an already-installed app surfaces the error
const client2 = fakeNativeClient();
client2.apps.push({ app_id: 'com.natives.app.demo', name: 'Demo', version: '0.1.0', enabled: true, show_in_sidebar: true });
const dom2 = makeDom();
const center2 = createAppCenter({ t, client: client2, catalogLoader: async () => catalog });
await tick();
// registry-wins merge: the card shows the installed state, not a fresh install
assert.equal(dom2.listEl.querySelector('.app-card[data-app-id="com.natives.app.demo"] .status')?.textContent, '已安装');
const conflict = await center2.install(catalog.apps[0]).catch((error) => error);
assert.equal(conflict.code, 'APP_CONFLICT', '重复安装必须暴露 APP_CONFLICT（由 UI 按钮层显示 toast）');
assert.equal(client2.apps.length, 1, '冲突后注册表不新增条目');
// button path surfaces a failing commit in the toast:
const client5 = fakeNativeClient({
  'apps:install_commit': async () => {
    const error = new Error('commit rejected');
    error.code = 'INTERNAL';
    throw error;
  },
});
const dom5 = makeDom();
createAppCenter({ t, client: client5, catalogLoader: async () => catalog });
await tick();
const demoInstall = dom5.listEl.querySelector('.app-card[data-app-id="com.natives.app.demo"] .action.primary');
demoInstall.onclick(new Event('click', { bubbles: true }));
await tick();
assert.ok(dom5.toastEl.textContent.includes('commit rejected'), '按钮路径错误必须显示在 toast');
assert.equal(client5.apps.length, 0, 'commit 失败不得留下注册表条目');

// 5. uninstall: confirm dialog → registry empties → card back to 未安装
const uninstallBtn = dom1.listEl.querySelector('.app-card[data-app-id="com.natives.app.demo"] .action.danger');
assert.ok(uninstallBtn, '已安装条目提供卸载按钮');
uninstallBtn.onclick(new Event('click', { bubbles: true }));
await tick();
const okButton = document.querySelector('.apps-dialog [data-role="ok"]');
assert.ok(okButton, '卸载需要确认对话框');
okButton.onclick(new Event('click', { bubbles: true }));
await tick();
assert.equal(client1.apps.length, 0, '卸载后注册表为空');
assert.equal(dom1.listEl.querySelector('.app-card[data-app-id="com.natives.app.demo"] .status')?.textContent, '未安装');
assert.ok(dom1.toastEl.textContent.includes('应用已卸载'), '卸载成功 toast');

// 6. structural: settings menu exposes 应用中心 from both surfaces
for (const page of ['files.js', 'space.js']) {
  const src = readFileSync(new URL(`./${page}`, import.meta.url), 'utf8');
  assert.ok(src.includes('onAppsCenter'), `${page} 必须接线 onAppsCenter`);
  assert.ok(src.includes('openAppsCenter'), `${page} 必须定义 openAppsCenter`);
}
const settingsMenu = readFileSync(new URL('./settings-menu.js', import.meta.url), 'utf8');
assert.ok(settingsMenu.includes('appsRow') && settingsMenu.includes('appsCenter'), '设置菜单必须含应用中心行');

// 7. locales
for (const loc of ['zh_CN', 'en']) {
  const messages = JSON.parse(readFileSync(new URL(`./_locales/${loc}/messages.json`, import.meta.url), 'utf8'));
  for (const key of ['appsCenter', 'appsInstall', 'appsUninstallBody', 'navApps']) {
    assert.equal(typeof messages[key]?.message, 'string', `${loc} 缺少 ${key}`);
  }
}

// 8. D44: build-time fixed catalog inside the package
const parsed = JSON.parse(readFileSync(new URL('./apps/catalog-v1.json', import.meta.url), 'utf8'));
assert.ok(Array.isArray(parsed.apps) && parsed.apps.length >= 1, 'build-time fake catalog 必须存在');

// 9. apps.html declares the required containers
const appsHtml = readFileSync(new URL('./apps.html', import.meta.url), 'utf8');
assert.ok(appsHtml.includes('id="apps-list"'), 'apps.html 必须声明 #apps-list');
assert.ok(appsHtml.includes('apps.js'), 'apps.html 必须加载 apps.js');

// ─── Phase A6: App Surface (ADR-0025 D50) ───────────────────────────────────

const DEMO_DETAIL = {
  app: {
    app_id: 'com.natives.app.demo',
    kind: 'extension_app',
    name: 'Demo',
    version: '0.1.0',
    enabled: true,
    show_in_sidebar: true,
    sidebar_order: 0,
    runtime_spec_json: '{"host":"com.natives.app.demo"}',
    surface_json: '{"icon":"grid","route":"app.html?app=com.natives.app.demo"}',
    manifest_json: '{}',
    installed_at: 1,
    updated_at: 1,
    revision: 1,
  },
  packages: [],
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
        if (!detail || detail.app.app_id !== params.app_id) return { app: null };
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
      if (method === 'ping') return { pong: true };
      if (method === 'version') return { host: 'com.natives.app.demo', version: '0.1.0' };
      if (method === 'health') return { ok: true };
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
assert.equal(appHostFor(DEMO_DETAIL.app), 'com.natives.app.demo', 'host 来自 runtime_spec.host');
assert.equal(appHostFor({ app_id: 'fund' }), 'com.natives.app.fund', '缺省回退 com.natives.app.<appId>');

// A6.2 installed + enabled → demo-ui mounts and talks to the app host
{
  const domA = makeSurfaceDom();
  const hostA = fakeHostClient();
  const shellA = createAppShell({
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
  assert.ok(hostA.calls.some((c) => c.method === 'ping'), 'demo-ui 调用 ping');
  assert.ok(hostA.calls.some((c) => c.method === 'version'), 'demo-ui 调用 version');
  assert.ok(hostA.calls.some((c) => c.method === 'health'), 'demo-ui 调用 health');
  await shellA.open(); // retry path: idempotent remount must not throw
}

// A6.3 unknown UI module → "需要更新 Natives" + no host port is opened
{
  const domB = makeSurfaceDom();
  let hostCreated = 0;
  const detailB = { ...DEMO_DETAIL, app: { ...DEMO_DETAIL.app, app_id: 'com.example.mystery' } };
  const shellB = createAppShell({
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
  const shellC = createAppShell({
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
  const shellD = createAppShell({
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
      enabled: true, runtime_spec_json: '{"host":"com.natives.app.fund"}',
    },
    packages: [],
    permissions: [],
  };
  const shellE = createAppShell({
    appId: 'fund',
    stage: domE.stage,
    getNativeClient: () => surfaceClient(fundDetail),
    createHostClient: () => { hostCreated += 1; return fakeHostClient(); },
  });
  await shellE.open();
  await tick();
  assert.ok(domE.stage.textContent.includes('基金'), 'fund 占位 UI 已挂载');
  assert.equal(hostCreated, 0, 'fund 占位不得打开 host（fund-host 尚未实现）');
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
