import assert from 'node:assert/strict';
import { WIDGET_KEYS, BACKGROUND_KEYS, widgetPlugins, backgroundPlugins } from './space-plugins.js';
import { MockElement, setupTestDomEnvironment } from './test-dom-mock.js';

console.log('--- Space Plugins Behavior Smoke Tests ---');

setupTestDomEnvironment();

globalThis.fetch = async (url) => {
  const urlStr = String(url);
  if (urlStr.includes('open-meteo')) return { json: async () => ({ current: { temperature_2m: 22, weather_code: 0 } }) };
  if (urlStr.includes('er-api.com')) return { json: async () => ({ result: 'success', rates: { CNY: 7.23, USD: 1 } }) };
  if (urlStr.includes('coingecko')) return { json: async () => ({ bitcoin: { usd: 65000, cny: 460000 } }) };
  if (urlStr.includes('ipify')) return { json: async () => ({ ip: '114.114.114.114' }) };
  return { json: async () => ({}) };
};

globalThis.chrome = {
  bookmarks: {
    getRecent: (count, cb) => cb([{ id: '1', title: 'Example', url: 'https://example.com' }]),
    getChildren: (id, cb) => cb([{ id: '1', title: 'Example', url: 'https://example.com' }]),
  },
  topSites: { get: (cb) => cb([{ title: 'GitHub', url: 'https://github.com' }]) },
};

const mockContext = {
  t: (k, fallback) => fallback || k,
  lang: 'zh_CN',
  shadowRoot: new MockElement('shadow-root'),
  onDataChange: () => {},
};

// 1. Test All 29 Widgets
console.log(`\nTesting ${WIDGET_KEYS.length} Widgets:`);
for (const key of WIDGET_KEYS) {
  const plugin = widgetPlugins[key];
  assert.ok(plugin, `Widget plugin missing for key: ${key}`);
  assert.equal(plugin.key, key, `Plugin key mismatch for: ${key}`);
  assert.ok(typeof plugin.name === 'string' && plugin.name.length > 0, `Plugin name missing: ${key}`);
  assert.ok(plugin.defaultData && typeof plugin.defaultData === 'object', `defaultData missing: ${key}`);
  assert.doesNotThrow(() => JSON.stringify(plugin.defaultData), `defaultData not serializable: ${key}`);
  assert.ok(typeof plugin.render === 'function', `render function missing: ${key}`);

  const container = new MockElement('div');
  let disposer = null;
  assert.doesNotThrow(() => {
    disposer = plugin.render(container, plugin.defaultData, { position: 'middleCentre' }, mockContext);
  }, `Widget render failed: ${key}`);

  if (typeof disposer === 'function') {
    disposer();
  }

  if (plugin.renderSettings) {
    assert.ok(typeof plugin.renderSettings === 'function', `renderSettings must be function: ${key}`);
    const settingsContainer = new MockElement('div');
    assert.doesNotThrow(() => {
      plugin.renderSettings(settingsContainer, plugin.defaultData, () => {}, mockContext);
    }, `Widget renderSettings failed: ${key}`);
  }

  if (plugin.styles !== undefined) {
    assert.equal(typeof plugin.styles, 'string', `styles must be string: ${key}`);
  }

  console.log(`  ✓ Widget ${key} smoke passed`);
}

// 2. Test All 9 Backgrounds
console.log(`\nTesting ${BACKGROUND_KEYS.length} Backgrounds:`);
for (const key of BACKGROUND_KEYS) {
  const plugin = backgroundPlugins[key];
  assert.ok(plugin, `Background plugin missing for key: ${key}`);
  assert.equal(plugin.key, key, `Plugin key mismatch for: ${key}`);
  assert.ok(typeof plugin.name === 'string' && plugin.name.length > 0, `Plugin name missing: ${key}`);
  assert.ok(plugin.defaultData && typeof plugin.defaultData === 'object', `defaultData missing: ${key}`);
  assert.doesNotThrow(() => JSON.stringify(plugin.defaultData), `defaultData not serializable: ${key}`);
  assert.ok(typeof plugin.render === 'function', `render function missing: ${key}`);

  const container = new MockElement('div');
  let disposer = null;
  assert.doesNotThrow(() => {
    disposer = plugin.render(container, plugin.defaultData, mockContext);
  }, `Background render failed: ${key}`);

  if (typeof disposer === 'function') {
    disposer();
  }

  if (plugin.renderSettings) {
    assert.ok(typeof plugin.renderSettings === 'function', `renderSettings must be function: ${key}`);
    const settingsContainer = new MockElement('div');
    assert.doesNotThrow(() => {
      plugin.renderSettings(settingsContainer, plugin.defaultData, () => {}, mockContext);
    }, `Background renderSettings failed: ${key}`);
  }

  if (plugin.styles !== undefined) {
    assert.equal(typeof plugin.styles, 'string', `styles must be string: ${key}`);
  }

  console.log(`  ✓ Background ${key} smoke passed`);
}

console.log('\nAll 29 widgets and 9 backgrounds passed behavioral smoke testing!\n');
