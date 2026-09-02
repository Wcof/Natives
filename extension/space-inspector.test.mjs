import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createSpaceInspector } from './space-inspector.js';
import { createSpaceToolbar } from './space-toolbar.js';

console.log('--- Space Inspector & Toolbar State Machine Test ---');

const [inspectorSource, spaceHtml] = await Promise.all([
  readFile(new URL('./space-inspector.js', import.meta.url), 'utf8'),
  readFile(new URL('./space.html', import.meta.url), 'utf8'),
]);

assert.doesNotMatch(inspectorSource, /\bprompt\s*\(/, 'workspace reset must never require a typed command');
assert.match(spaceHtml, /id="ws-reset-modal"/, 'workspace reset must use a selectable modal');
assert.doesNotMatch(spaceHtml, /id="toggle-sidebar"/, 'personal space must not expose the legacy sidebar toggle');
assert.equal((spaceHtml.match(/id="space-toggle-widgets-btn"/g) || []).length, 1, 'personal space must expose one widget visibility toggle');

// Mock DOM elements
function createMockEl(id = '') {
  return {
    id,
    tagName: 'DIV',
    hidden: true,
    style: {},
    dataset: {},
    children: [],
    attributes: [],
    classList: {
      toggle(cls, state) {
        if (state) this.classes.add(cls);
        else this.classes.delete(cls);
      },
      add(cls) { this.classes.add(cls); },
      remove(cls) { this.classes.delete(cls); },
      classes: new Set(),
    },
    setAttribute(k, v) { this[k] = v; },
    getAttribute(k) { return this[k]; },
    append(...nodes) { this.children.push(...nodes); },
    replaceChildren(...nodes) { this.children = [...nodes]; },
    querySelector(sel) {
      if (sel.startsWith('#')) {
        const targetId = sel.slice(1);
        return this.children.find((c) => c.id === targetId) || createMockEl(targetId);
      }
      return this.children[0] || createMockEl();
    },
    querySelectorAll() { return this.children; },
    focus() { this.focused = true; },
  };
}

globalThis.document = {
  createElement(tag) {
    return createMockEl(tag);
  },
};

const inspectorEl = createMockEl('inspector');
const backdropEl = createMockEl('inspector-backdrop');
const inspectorBody = createMockEl('inspector-body');
const settingsBtn = createMockEl('space-settings-btn');
const toggleWidgetsBtn = createMockEl('space-toggle-widgets-btn');
const emptyStateEl = createMockEl('space-empty-state');
const emptyAddBtn = createMockEl('space-empty-add-btn');

const elementMap = {
  'inspector': inspectorEl,
  'inspector-backdrop': backdropEl,
  'inspector-body': inspectorBody,
  'space-settings-btn': settingsBtn,
  'space-toggle-widgets-btn': toggleWidgetsBtn,
  'space-empty-state': emptyStateEl,
  'space-empty-add-btn': emptyAddBtn,
};

const $ = (id) => elementMap[id] || createMockEl(id);
const t = (k, f) => f || k;

let lastCall = null;
let currentSnapshot = {
  name: 'Main Space',
  backgroundJson: { key: 'background/colour', display: { colour: '#101010' } },
  widgets: [
    { id: 'w-time-1', key: 'widget/time', order: 0, enabled: true, configJson: {}, displayJson: { position: 'middleCentre' } },
  ],
  revision: 1,
};

const mockNativeCall = async (method, params) => {
  lastCall = { method, params };
  if (method === 'workspace_widget_upsert') {
    const updated = {
      ...currentSnapshot,
      widgets: [...currentSnapshot.widgets, { ...params.widget, id: 'w-new-1' }],
      revision: currentSnapshot.revision + 1,
    };
    return updated;
  }
  if (method === 'workspace_reset') {
    return {
      ...currentSnapshot,
      widgets: [],
      revision: currentSnapshot.revision + 1,
    };
  }
  return currentSnapshot;
};

// 1. Initialize Inspector
let closedFocusCalled = false;
const inspector = createSpaceInspector({
  $,
  t,
  language: 'zh_CN',
  nativeCall: mockNativeCall,
  broadcastRevision: () => {},
  updateSnapshot: (snap) => { currentSnapshot = snap; },
  onCloseFocusAnchor: () => { closedFocusCalled = true; },
});

assert.equal(inspector.isOpen, false);
assert.equal(inspector.state, 'closed');
console.log('✓ Initial closed state verified');

// 2. Open to overview
inspector.sync(currentSnapshot, 'ws-1');
inspector.open('overview');
assert.equal(inspector.isOpen, true);
assert.equal(inspector.state, 'overview');
assert.equal(inspectorEl.hidden, false);
assert.equal(backdropEl.hidden, false);
console.log('✓ Overview route verified');

// 3. Route to catalog
inspector.open('catalog');
assert.equal(inspector.state, 'catalog');
console.log('✓ Catalog route verified');

// 4. Route to specific widget
inspector.open({ widgetId: 'w-time-1' });
assert.equal(inspector.state, 'widget');
console.log('✓ Widget settings route verified');

// 5. Route to background
inspector.open('background');
assert.equal(inspector.state, 'background');
console.log('✓ Background settings route verified');

// 6. Close Inspector & focus anchor callback
inspector.close();
assert.equal(inspector.isOpen, false);
assert.equal(inspector.state, 'closed');
assert.equal(inspectorEl.hidden, true);
assert.equal(closedFocusCalled, true);
console.log('✓ Close and focus anchor restoration verified');

// 7. Toolbar shortcuts & state sync
let toolbarSettingsToggled = false;
let toolbarWidgetsToggled = false;
let toolbarSidebarToggled = false;
let toolbarCatalogOpened = false;

const toggleSidebarBtn = createMockEl('space-toggle-sidebar-btn');
elementMap['space-toggle-sidebar-btn'] = toggleSidebarBtn;

const toolbar = createSpaceToolbar({
  $,
  t,
  onToggleSettings: () => { toolbarSettingsToggled = true; },
  onToggleWidgets: () => { toolbarWidgetsToggled = true; },
  onToggleSidebar: () => { toolbarSidebarToggled = true; },
  onOpenCatalog: () => { toolbarCatalogOpened = true; },
});

toolbar.sync(currentSnapshot);
assert.equal(emptyStateEl.hidden, true, 'Space with widgets must not show empty state');

toolbar.sync({ ...currentSnapshot, widgets: [] });
assert.equal(emptyStateEl.hidden, false, 'Empty workspace must show empty state CTA');

// Test toolbar button clicks
settingsBtn.onclick();
assert.equal(toolbarSettingsToggled, true);
toggleSidebarBtn.onclick();
assert.equal(toolbarSidebarToggled, true);
toggleWidgetsBtn.onclick();
assert.equal(toolbarWidgetsToggled, true);
emptyAddBtn.onclick();
assert.equal(toolbarCatalogOpened, true);
// 8. Visual & Layout Contracts Verification
console.log('--- Inspector Compact Controls & Plugins Structure Contract ---');
const [spaceCss, filesCss] = await Promise.all([
  readFile(new URL('./space.css', import.meta.url), 'utf8'),
  readFile(new URL('./files.css', import.meta.url), 'utf8'),
]);

// A. Check search input shell contract equivalence
assert.match(filesCss, /\.command-search\s*\{[^}]*min-height:34px;[^}]*padding:0 11px;/, 'Left search must declare 34px compact shell');
assert.match(spaceCss, /\.catalog-search-wrap\s*\{[^}]*min-height:34px;[^}]*padding:0 11px;/, 'Catalog search must match left command-search compact dimensions');

// B. Check top-label contract and container queries
assert.match(spaceCss, /\.inspector-field\s*\{[^}]*display:grid;[^}]*gap:6px;/, 'Inspector fields must place labels on top with 6px gap');
assert.match(spaceCss, /container-type:inline-size/, 'Inspector must declare container queries');
assert.match(spaceCss, /\.inspector-field-range-header/, 'Range fields must display label and value in header row');

// C. Verify all 29 widget and 9 background renderSettings without inline style attributes
const { widgetPlugins, backgroundPlugins } = await import('./space-plugins.js');
const testHost = createMockEl('plugin-test-host');

for (const [key, plugin] of Object.entries(widgetPlugins)) {
  if (plugin.renderSettings) {
    testHost.children = [];
    testHost.innerHTML = '';
    plugin.renderSettings(testHost, plugin.defaultData || {}, () => {}, { t });
    const content = testHost.innerHTML || '';
    assert.doesNotMatch(content, /style="[^"]*display\s*:\s*flex[^"]*"/i, `Widget ${key} renderSettings must not use inline flex layout`);
    assert.doesNotMatch(content, /style="[^"]*width\s*:\s*100%[^"]*"/i, `Widget ${key} renderSettings must not use inline width style`);
  }
}

for (const [key, plugin] of Object.entries(backgroundPlugins)) {
  if (plugin.renderSettings) {
    testHost.children = [];
    testHost.innerHTML = '';
    plugin.renderSettings(testHost, plugin.defaultData || {}, () => {}, { t });
    const content = testHost.innerHTML || '';
    assert.doesNotMatch(content, /style="[^"]*display\s*:\s*flex[^"]*"/i, `Background ${key} renderSettings must not use inline flex layout`);
    assert.doesNotMatch(content, /style="[^"]*width\s*:\s*100%[^"]*"/i, `Background ${key} renderSettings must not use inline width style`);
  }
}
console.log('✓ Compact controls and plugin settings contracts verified');

console.log('All space-inspector and toolbar tests passed!\n');
