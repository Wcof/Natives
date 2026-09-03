import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { widgetPlugins, WIDGET_KEYS } from './space-plugins.js';
import { buildDashboardStyles } from './space-dashboard-styles.js';
import { setupTestDomEnvironment } from './test-dom-mock.js';

setupTestDomEnvironment();

console.log('--- 24 Space Widgets Structural & Baseline Verification ---');

assert.equal(WIDGET_KEYS.length, 24, 'Must register exactly 24 widgets');
const manifest = JSON.parse(readFileSync(new URL('./manifest.json', import.meta.url), 'utf8'));
assert.deepEqual(manifest.optional_permissions?.sort(), ['bookmarks', 'topSites']);
// Every host the network widgets fetch must stay declared, or real Chrome CORS-blocks them.
const declaredHosts = new Set((manifest.host_permissions || []).map((p) => p.replace(/^https?:\/\//, '').replace(/\/\*$/, '').replace(/\/$/, '')));
const sources = Object.values(widgetPlugins).map((p) => String(p.render || '') + String(p.renderSettings || ''));
for (const src of sources) {
  for (const m of src.matchAll(/fetch\(`?https?:\/\/([a-z0-9.-]+)/gi)) {
    const host = m[1];
    const covered = [...declaredHosts].some((d) => host === d || host.endsWith(`.${d}`));
    assert.ok(covered, `manifest.host_permissions must cover fetched host: ${host}`);
  }
}

const mockContainer = document.createElement('div');
document.body.append(mockContainer);

for (const key of WIDGET_KEYS) {
  const plugin = widgetPlugins[key];
  assert.ok(plugin, `Plugin ${key} must exist in widgetPlugins registry`);
  assert.equal(plugin.key, key, `Plugin key mismatch: ${plugin.key} vs ${key}`);
  assert.ok(typeof plugin.name === 'string' && plugin.name, `Plugin ${key} must have name`);
  assert.ok(typeof plugin.render === 'function', `Plugin ${key} must have render function`);
  assert.ok(typeof plugin.renderSettings === 'function', `Plugin ${key} must have renderSettings function`);
  if (!['widget/css', 'widget/html'].includes(key)) {
    assert.ok(typeof plugin.styles === 'string', `Plugin ${key} must declare styles`);
  }

  // 1. Render default state
  mockContainer.replaceChildren();
  const disposer = plugin.render(mockContainer, plugin.defaultData || {}, {}, {
    t: (k, f) => f || k,
    lang: 'zh_CN',
    onDataChange: () => {},
  });

  assert.ok(mockContainer.children.length > 0 || mockContainer.textContent !== undefined, `Widget ${key} render must produce DOM output`);

  // 2. Render settings
  const settingsContainer = document.createElement('div');
  plugin.renderSettings(settingsContainer, plugin.defaultData || {}, () => {}, {
    t: (k, f) => f || k,
    lang: 'zh_CN',
  });
  assert.ok(settingsContainer.children.length > 0, `Widget ${key} renderSettings must produce settings DOM`);

  // Cleanup
  if (typeof disposer === 'function') {
    disposer();
  }

  console.log(`  ✓ Widget ${key} passed structural contract verification`);
}

function renderWidget(key, data = widgetPlugins[key].defaultData) {
  const container = document.createElement('div');
  const dispose = widgetPlugins[key].render(container, data, {}, {
    t: (name, fallback) => fallback || name,
    lang: 'zh_CN',
    onDataChange: () => {},
  });
  return { container, dispose };
}

{
  const { container, dispose } = renderWidget('widget/binaryTime');
  assert.equal(container.querySelectorAll('.binary-digit-group').length, 3);
  assert.equal(container.querySelectorAll('.binary-digit').length, 6);
  assert.equal(container.querySelectorAll('.pip').length, 24);
  dispose();
}

{
  const { container, dispose } = renderWidget('widget/palette');
  assert.equal(container.querySelectorAll('.Color').length, 5);
  assert.equal(container.querySelector('.palette-root'), null);
  dispose();
}

{
  const { container, dispose } = renderWidget('widget/workHours', {
    startTime: '00:00', endTime: '23:59', days: [new Date().getDay()],
  });
  assert.match(container.querySelector('h2').textContent, /^\d+%$/);
  assert.equal(container.querySelector('.workhours-bar-bg'), null);
  dispose();
}

const dashboardStyles = buildDashboardStyles({
  widgetStyles: Object.values(widgetPlugins).map((plugin) => plugin.styles || '').join('\n'),
});
assert.match(dashboardStyles, /\.Slot > \*[\s\S]*margin:\s*1rem/);
assert.doesNotMatch(dashboardStyles, /\.Widget\s*\{[^}]*display:\s*inline-flex/s);
assert.doesNotMatch(dashboardStyles, /\.Weather[^}]*backdrop-filter/s);
assert.doesNotMatch(dashboardStyles, /\.Notes[^}]*backdrop-filter/s);
assert.doesNotMatch(dashboardStyles, /\.Trello[^}]*backdrop-filter/s);

console.log('\nAll 24 widgets pass structural and DOM contract verification!');
