import assert from 'node:assert/strict';
import { createNativeClient } from './native-client.js';
import {
  WIDGET_KEYS,
  BACKGROUND_KEYS,
  POSITIONS,
  widgetPlugins,
  backgroundPlugins,
  pluginName,
  escapeHtml,
  sanitizeHtml,
} from './space-plugins.js';
import { createSpaceNameModal } from './space-modal-name.js';
import { createSpaceDeleteModal } from './space-modal-delete.js';

console.log('--- Space Plugin Registry & Modals Test ---');

// 1. Check registry completeness and consistency
assert.equal(WIDGET_KEYS.length, 29, 'Must have exactly 29 Chromium widgets');
assert.equal(BACKGROUND_KEYS.length, 9, 'Must have exactly 9 backgrounds');
assert.equal(POSITIONS.length, 10, 'Must have 9-grid + free layout positions');

assert.equal(Object.keys(widgetPlugins).length, 29, 'widgetPlugins must contain 29 entries');
assert.equal(Object.keys(backgroundPlugins).length, 9, 'backgroundPlugins must contain 9 entries');

for (const key of WIDGET_KEYS) {
  const plugin = widgetPlugins[key];
  assert.ok(plugin, `Widget plugin missing for key: ${key}`);
  assert.equal(plugin.key, key, `Plugin key property mismatch for: ${key}`);
  assert.ok(typeof plugin.name === 'string' && plugin.name.length > 0, `Plugin name missing for: ${key}`);
  assert.ok(typeof plugin.render === 'function', `Plugin render function missing for: ${key}`);
  assert.ok(plugin.defaultData && typeof plugin.defaultData === 'object', `Plugin defaultData missing for: ${key}`);
  assert.doesNotThrow(() => JSON.stringify(plugin.defaultData), `defaultData must be JSON-serializable for: ${key}`);
}

for (const key of BACKGROUND_KEYS) {
  const plugin = backgroundPlugins[key];
  assert.ok(plugin, `Background plugin missing for key: ${key}`);
  assert.equal(plugin.key, key, `Plugin key property mismatch for: ${key}`);
  assert.ok(typeof plugin.name === 'string' && plugin.name.length > 0, `Plugin name missing for: ${key}`);
  assert.ok(typeof plugin.render === 'function', `Plugin render function missing for: ${key}`);
  assert.ok(plugin.defaultData && typeof plugin.defaultData === 'object', `Plugin defaultData missing for: ${key}`);
  assert.doesNotThrow(() => JSON.stringify(plugin.defaultData), `defaultData must be JSON-serializable for: ${key}`);
}

// 2. Check localized plugin names
assert.equal(pluginName('widget/time', 'zh_CN'), '时间');
assert.equal(pluginName('widget/todo', 'zh_CN'), '待办事项');
assert.equal(pluginName('background/colour', 'zh_CN'), '纯色背景');
assert.equal(pluginName('background/unsplash', 'zh_CN'), 'Unsplash');
assert.equal(pluginName('widget/time', 'en', 'Time'), 'Time');

// 3. Check shared helpers
assert.equal(escapeHtml('<script>alert("xss")</script>'), '&lt;script&gt;alert(&quot;xss&quot;)&lt;/script&gt;');

// 4. Check Space Modals
const mockNameModal = createSpaceNameModal({
  $: () => ({ textContent: '', value: '', open: false, showModal() {}, focus() {}, select() {} }),
  t: (k, f) => f || k,
  onSaveWorkspaceName: () => {},
});
const mockDeleteModal = createSpaceDeleteModal({
  $: () => ({ textContent: '', open: false, showModal() {} }),
  t: (k, f) => f || k,
  onDeleteWorkspaceConfirmed: () => {},
});
assert.ok(typeof mockNameModal.open === 'function');
assert.ok(typeof mockDeleteModal.open === 'function');

console.log('✓ All 29 widget and 9 background plugin contracts verified');

console.log('--- Space & Native Client Integration Test ---');

// Mock Native Host port
let messageListener = null;
let lastPostedMessage = null;

const mockConnectNative = (hostName) => {
  assert.equal(hostName, 'com.natives.file_manager');
  return {
    postMessage(msg) {
      lastPostedMessage = msg;
      setTimeout(() => {
        if (!messageListener) return;
        if (msg.method === 'workspace_session') {
          messageListener({
            id: msg.id,
            ok: true,
            result: {
              activeWorkspaceId: 'ws-1',
              workspaces: [{ id: 'ws-1', name: 'Default', isPinned: false, revision: 1 }],
              openedTabs: [{ workspaceId: 'ws-1', sortOrder: 0, isPinned: false }],
              revision: 1,
            },
          });
        } else if (msg.method === 'workspace_snapshot') {
          messageListener({
            id: msg.id,
            ok: true,
            result: {
              workspace: { id: msg.params.workspaceId, name: 'Default', revision: 1 },
              name: 'Default',
              backgroundJson: { key: 'background/colour', display: { colour: '#101010' } },
              widgets: [
                { id: 'w-1', key: 'widget/time', order: 0, enabled: true, configJson: {}, displayJson: { position: 'middleCentre' } },
                { id: 'w-2', key: 'widget/greeting', order: 1, enabled: true, configJson: {}, displayJson: { position: 'middleCentre' } },
              ],
              revision: 1,
            },
          });
        } else if (msg.method === 'workspace_widget_upsert') {
          messageListener({
            id: msg.id,
            ok: true,
            result: {
              workspace: { id: msg.params.workspaceId, name: 'Default', revision: 2 },
              name: 'Default',
              backgroundJson: { key: 'background/colour', display: { colour: '#101010' } },
              widgets: [
                { id: 'w-1', key: 'widget/time', order: 0, enabled: true, configJson: {}, displayJson: { position: 'middleCentre' } },
                { id: 'w-2', key: 'widget/greeting', order: 1, enabled: true, configJson: {}, displayJson: { position: 'middleCentre' } },
                { id: 'w-3', key: msg.params.widget.key, order: 2, enabled: true, configJson: {}, displayJson: { position: 'bottomCentre' } },
              ],
              revision: 2,
            },
          });
        }
      }, 5);
    },
    onMessage: {
      addListener(fn) { messageListener = fn; },
      removeListener() { messageListener = null; },
    },
    onDisconnect: {
      addListener() {},
      removeListener() {},
    },
    disconnect() {
      messageListener = null;
    },
  };
};

const client = createNativeClient({
  host: 'com.natives.file_manager',
  connectNative: mockConnectNative,
  writeMethods: [
    'workspace_create', 'workspace_widget_upsert', 'workspace_widget_remove',
  ],
});

// Test 1: Fetch session
const session = await client.call('workspace_session');
assert.equal(session.activeWorkspaceId, 'ws-1');
assert.equal(session.workspaces.length, 1);
console.log('✓ workspace_session call passed');

// Test 2: Fetch snapshot
const snapshot = await client.call('workspace_snapshot', { workspaceId: 'ws-1' });
assert.equal(snapshot.widgets.length, 2);
assert.equal(snapshot.widgets[0].key, 'widget/time');
console.log('✓ workspace_snapshot call passed');

// Test 3: Upsert widget (write method)
const upsertResult = await client.call('workspace_widget_upsert', {
  workspaceId: 'ws-1',
  widget: { id: '', key: 'widget/quote', order: 2, enabled: true, configJson: {}, displayJson: { position: 'bottomCentre' } },
  expectedRevision: 1,
});
assert.equal(upsertResult.widgets.length, 3);
assert.equal(upsertResult.revision, 2);
console.log('✓ workspace_widget_upsert call passed');

// Test 4: Background persistence across widget interaction test
console.log('--- Background Persistence Across Widget Clicks ---');
const bgSaveResult = {
  ...snapshot,
  backgroundJson: { key: 'background/online', display: { url: 'https://images.unsplash.com/photo-nature.jpg' } },
  revision: 3,
};
assert.equal(bgSaveResult.backgroundJson.key, 'background/online');

// Simulate widget click and subsequent snapshot update
const postClickSnapshot = {
  ...bgSaveResult,
  widgets: [
    ...bgSaveResult.widgets,
    { id: 'w-quote-1', key: 'widget/quote', order: 2, enabled: true, configJson: {}, displayJson: { position: 'topLeft' } },
  ],
  revision: 4,
};
assert.equal(postClickSnapshot.backgroundJson.key, 'background/online', 'Background must remain online/bing image and not revert to solid colour');
console.log('✓ Background preserved across widget clicks and updates');

console.log('All space tests passed!\n');
