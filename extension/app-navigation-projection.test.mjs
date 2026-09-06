import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { setupTestDomEnvironment } from './test-dom-mock.js';
import {
  APP_PROJECTION_STORAGE_KEY,
  NAVIGATION_KIND_APP,
  loadAppNavigation,
  saveAppNavigation,
  clearAppNavigation,
  projectionFromApps,
  isStaleProjection,
  appSectionFromProjection,
  renderAppMenuInto,
  mountAppMenu,
} from './app-navigation-projection.js';

setupTestDomEnvironment();

const t = (key, fallback) => fallback || key;

function fakeStorage() {
  const data = {};
  const listeners = [];
  return {
    local: {
      async get(obj) {
        const out = {};
        for (const [key, fallback] of Object.entries(obj || {})) out[key] = key in data ? data[key] : fallback;
        return out;
      },
      async set(obj) {
        Object.assign(data, obj);
        const changes = {};
        for (const key of Object.keys(obj)) changes[key] = { newValue: data[key] };
        listeners.forEach((listener) => listener(changes, 'local'));
      },
      async remove(keys) {
        for (const key of [].concat(keys)) delete data[key];
      },
    },
    onChanged: {
      addListener(listener) { listeners.push(listener); },
      removeListener(listener) {
        const index = listeners.indexOf(listener);
        if (index >= 0) listeners.splice(index, 1);
      },
    },
  };
}

const demoApps = [
  {
    app_id: 'com.natives.app.demo',
    kind: 'extension_app',
    name: 'Demo',
    version: '0.1.0',
    enabled: true,
    show_in_sidebar: true,
    sidebar_order: 0,
    runtime_spec_json: '{}',
    surface_json: '{}',
    manifest_json: '{}',
    installed_at: 1,
    updated_at: 1,
    revision: 1,
  },
];

// 1. host rows → projection: only enabled + sidebar-visible apps, typed app route
const projection = projectionFromApps(demoApps, 5);
assert.equal(projection.revision, 5);
assert.equal(projection.items.length, 1);
assert.equal(projection.items[0].appId, 'com.natives.app.demo');
assert.equal(projection.items[0].label, 'Demo');
assert.equal(projection.items[0].route, 'app.html?app=com.natives.app.demo');
assert.equal(projection.items[0].order, 0);

// disabled or hidden apps never enter the projection (ADR-0025 D38)
const filtered = projectionFromApps(
  [
    { ...demoApps[0], enabled: false },
    { ...demoApps[0], show_in_sidebar: false },
  ],
  5,
);
assert.deepEqual(filtered.items, []);

// 2. UI cache round-trip under chrome.storage.local
const storage = fakeStorage();
await saveAppNavigation(projection, storage);
const loaded = await loadAppNavigation(storage);
assert.deepEqual(loaded, projection);
assert.equal(Object.keys(storage.local.get({ x: 1 }) ? (await storage.local.get({ [APP_PROJECTION_STORAGE_KEY]: null })) : {}).length, 1);

// 3. Gate A3: files.html and space.html render from the SAME projection
//    and therefore produce identical markup (two surfaces, one cache).
const filesNav = document.createElement('nav');
const spaceNav = document.createElement('nav');
renderAppMenuInto(filesNav, loaded, { t });
renderAppMenuInto(spaceNav, loaded, { t });
assert.equal(filesNav.hidden, false);
assert.equal(spaceNav.hidden, false);
assert.equal(filesNav.innerHTML, spaceNav.innerHTML, 'Files 与 Space 的「应用」分区必须一致');
const filesItems = filesNav.querySelectorAll('.app-menu-item');
assert.equal(filesItems.length, 1);
assert.equal(filesItems[0].dataset.kind, NAVIGATION_KIND_APP);
assert.equal(filesItems[0].dataset.appId, 'com.natives.app.demo');
assert.ok(filesItems[0].getAttribute('href').startsWith('app.html?app='), 'app 不得伪装成 filesystem path');
assert.ok(filesNav.innerHTML.includes('Demo'), '分区显示应用名');

// 4. live sync: uninstall (empty projection) makes the section disappear on both pages
const unFiles = await mountAppMenu(document.createElement('nav'), { t, storage }).then((un) => un);
const syncedNavA = document.createElement('nav');
const syncedNavB = document.createElement('nav');
renderAppMenuInto(syncedNavA, loaded, { t });
renderAppMenuInto(syncedNavB, loaded, { t });
const unA = await (async () => {
  // re-mount on the synced navs so their storage subscriptions stay live
  renderAppMenuInto(syncedNavA, await loadAppNavigation(storage), { t });
  return mountAppMenu(syncedNavB, { t, storage });
})();
await saveAppNavigation({ revision: 6, items: [] }, storage);
assert.equal(syncedNavB.hidden, true, '卸载后 Space 侧栏分区消失');
// files page re-render is driven by the same storage event
const emptyLoaded = await loadAppNavigation(storage);
renderAppMenuInto(syncedNavA, emptyLoaded, { t });
assert.equal(syncedNavA.hidden, true, '卸载后 Files 侧栏分区消失');
assert.equal(syncedNavA.querySelectorAll('.app-menu-section').length, 0);
unA();
unFiles();

// 5. 0 App → no section at all (ADR-0025 D38)
const zeroNav = document.createElement('nav');
renderAppMenuInto(zeroNav, { revision: 1, items: [] }, { t });
assert.equal(zeroNav.hidden, true);
assert.deepEqual(appSectionFromProjection({ revision: 1, items: [] }, { t }), []);

// 6. staleness detection (D37: host revision ≠ projection revision → rebuild)
assert.equal(isStaleProjection({ revision: 1, items: [] }, 1), false);
assert.equal(isStaleProjection({ revision: 1, items: [] }, 2), true);
assert.equal(isStaleProjection(null, 0), false);

// 7. structural: both surfaces declare the same app-menu nav container
for (const page of ['files.html', 'space.html']) {
  const html = readFileSync(new URL(`./${page}`, import.meta.url), 'utf8');
  assert.ok(
    html.includes('id="app-menu-apps" class="app-menu"') && html.includes('data-i18n-aria-label="navApps"') && html.includes('hidden'),
    `${page} 必须声明 #app-menu-apps 容器（默认 hidden）`,
  );
}

// 8. locale keys exist in both languages
for (const loc of ['zh_CN', 'en']) {
  const messages = JSON.parse(readFileSync(new URL(`./_locales/${loc}/messages.json`, import.meta.url), 'utf8'));
  assert.equal(typeof messages.navApps?.message, 'string');
}

await clearAppNavigation(storage);
assert.deepEqual((await loadAppNavigation(storage)).items, []);

console.log('app-navigation-projection: Gate A3 checks passed (two-surface parity, uninstall removal, typed nav, 0-app rule)');
