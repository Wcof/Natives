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
let toolbarCatalogOpened = false;
const toolbar = createSpaceToolbar({
  $,
  t,
  onToggleSettings: () => { toolbarSettingsToggled = true; },
  onToggleWidgets: () => {},
  onOpenCatalog: () => { toolbarCatalogOpened = true; },
});

toolbar.sync(currentSnapshot);
assert.equal(emptyStateEl.hidden, true, 'Space with widgets must not show empty state');

toolbar.sync({ ...currentSnapshot, widgets: [] });
assert.equal(emptyStateEl.hidden, false, 'Empty workspace must show empty state CTA');

// Test toolbar button clicks
settingsBtn.onclick();
assert.equal(toolbarSettingsToggled, true);
emptyAddBtn.onclick();
assert.equal(toolbarCatalogOpened, true);
console.log('✓ Toolbar actions and empty state sync verified');

console.log('All space-inspector and toolbar tests passed!\n');
