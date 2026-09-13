import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createSpaceInspector } from './space-inspector.js';
import { createSpaceToolbar } from './space-toolbar.js';

console.log('--- Space Inspector & Toolbar State Machine Test ---');

const [inspectorSource, spaceHtml, spaceBootstrap] = await Promise.all([
  readFile(new URL('./space-inspector.js', import.meta.url), 'utf8'),
  readFile(new URL('./space.html', import.meta.url), 'utf8'),
  readFile(new URL('./space-bootstrap.js', import.meta.url), 'utf8'),
]);

assert.doesNotMatch(inspectorSource, /\bprompt\s*\(/, 'workspace reset must never require a typed command');
assert.match(spaceHtml, /id="ws-reset-modal"/, 'workspace reset must use a selectable modal');
assert.doesNotMatch(spaceHtml, /id="toggle-sidebar"/, 'personal space must not expose the legacy sidebar toggle');
assert.equal((spaceHtml.match(/id="space-toggle-widgets-btn"/g) || []).length, 1, 'personal space must expose one widget visibility toggle');
assert.match(spaceHtml, /src="space-bootstrap\.js"/, 'space page must bootstrap per-tab UI restore before the module paints');
assert.match(spaceBootstrap, /natives-space-ui/, 'space bootstrap must keep per-tab UI state so refresh restores the current view');
assert.match(spaceBootstrap, /navType === 'reload'/, 'space bootstrap must restore UI state only on reload, keeping fresh new tabs on the initial layout');

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
    parentElement: null,
    get nextSibling() {
      if (!this.parentElement) return null;
      const idx = this.parentElement.children.indexOf(this);
      if (idx === -1 || idx === this.parentElement.children.length - 1) return null;
      return this.parentElement.children[idx + 1];
    },
    classList: {
      toggle(cls, state) {
        if (state) this.classes.add(cls);
        else this.classes.delete(cls);
      },
      add(cls) { this.classes.add(cls); },
      remove(cls) { this.classes.delete(cls); },
      contains(cls) { return this.classes.has(cls); },
      classes: new Set(),
    },
    setAttribute(k, v) { this[k] = v; },
    getAttribute(k) { return this[k]; },
    set innerHTML(html) {
      this._innerHTML = html;
      const orderMatch = html.match(/class="inspector-row-order"[^>]*>([^<]+)<\/span>/);
      if (orderMatch) {
        const orderEl = createMockEl();
        orderEl.className = 'inspector-row-order';
        orderEl.textContent = orderMatch[1];
        this.append(orderEl);
      }
    },
    get innerHTML() {
      return this._innerHTML || '';
    },
    append(...nodes) {
      for (const n of nodes) {
        if (n && typeof n === 'object') {
          n.parentElement = this;
          const idx = this.children.indexOf(n);
          if (idx !== -1) this.children.splice(idx, 1);
        }
      }
      this.children.push(...nodes);
    },
    replaceChildren(...nodes) {
      for (const n of nodes) {
        if (n && typeof n === 'object') n.parentElement = this;
      }
      this.children = [...nodes];
    },
    insertBefore(newNode, refNode) {
      if (newNode && typeof newNode === 'object') {
        newNode.parentElement = this;
        const existingIdx = this.children.indexOf(newNode);
        if (existingIdx !== -1) this.children.splice(existingIdx, 1);
      }
      if (!refNode) {
        this.children.push(newNode);
        return newNode;
      }
      const refIdx = this.children.indexOf(refNode);
      if (refIdx === -1) {
        this.children.push(newNode);
      } else {
        this.children.splice(refIdx, 0, newNode);
      }
      return newNode;
    },
    querySelector(sel) {
      if (sel.startsWith('#')) {
        const targetId = sel.slice(1);
        const found = this.children.find((c) => c.id === targetId);
        if (found) return found;
        const created = createMockEl(targetId);
        this.append(created);
        return created;
      }
      if (sel.startsWith('.')) {
        const cls = sel.slice(1);
        const found = this.children.find((c) => c.classList?.classes?.has(cls) || (c.className && c.className.includes(cls)));
        if (found) return found;
        const created = createMockEl();
        created.className = cls;
        this.append(created);
        return created;
      }
      const created = this.children[0] || createMockEl();
      if (!this.children.includes(created)) this.append(created);
      return created;
    },
    querySelectorAll(sel) {
      const results = [];
      function collect(node) {
        for (const c of node.children || []) {
          if (!sel) {
            results.push(c);
          } else if (sel.startsWith('.')) {
            const cls = sel.slice(1);
            if (c.classList?.classes?.has(cls) || (c.className && c.className.split(/\s+/).includes(cls))) {
              results.push(c);
            }
          } else {
            results.push(c);
          }
          collect(c);
        }
      }
      collect(this);
      return results;
    },
    setPointerCapture() {},
    releasePointerCapture() {},
    focus() { this.focused = true; },
  };
}

const windowListeners = new Map();
globalThis.window = {
  addEventListener(event, fn) {
    if (!windowListeners.has(event)) windowListeners.set(event, []);
    windowListeners.get(event).push(fn);
  },
  removeEventListener(event, fn) {
    const list = windowListeners.get(event) || [];
    windowListeners.set(event, list.filter((f) => f !== fn));
  },
  trigger(event, data) {
    const list = windowListeners.get(event) || [];
    list.forEach((fn) => fn(data));
  },
};

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
  if (method === 'workspace_widget_reorder') {
    const nextWidgets = params.orderedIds.map((id) => currentSnapshot.widgets.find((w) => w.id === id)).filter(Boolean);
    const updated = {
      ...currentSnapshot,
      widgets: nextWidgets,
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
  queueWorkspaceMutation: async (wsId, fn) => {
    const res = await fn(currentSnapshot);
    if (res) currentSnapshot = res;
    return res;
  },
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

// 6b. Clicking outside the sidebar closes it without requiring the header button
inspector.open('overview');
backdropEl.onclick();
assert.equal(inspector.isOpen, false);
console.log('✓ Backdrop click closes the inspector');

// 6c. Live pointer drag reordering (pull down / pull up without releasing mouse, sequence updates dynamically)
console.log('--- Inspector Live Pointer Drag Reorder Test ---');
const multiWidgetSnapshot = {
  name: 'Multi Widget Space',
  backgroundJson: { key: 'background/colour', display: { colour: '#101010' } },
  widgets: [
    { id: 'w-time-1', key: 'widget/time', order: 0, enabled: true, configJson: {}, displayJson: { position: 'middleCentre' } },
    { id: 'w-todo-2', key: 'widget/todo', order: 1, enabled: true, configJson: {}, displayJson: { position: 'middleCentre' } },
    { id: 'w-notes-3', key: 'widget/notes', order: 2, enabled: true, configJson: {}, displayJson: { position: 'middleCentre' } },
  ],
  revision: 1,
};
currentSnapshot = multiWidgetSnapshot;
inspector.sync(multiWidgetSnapshot, 'ws-1');
inspector.open('overview');

// Locate the rendered widget rows
const widgetRows = inspectorBody.querySelectorAll('.inspector-row');
assert.equal(widgetRows.length, 3, 'Must render 3 widget rows');

// Mock getBoundingClientRect for mid-point calculations
widgetRows[0].getBoundingClientRect = () => ({ top: 100, height: 40 }); // mid 120
widgetRows[1].getBoundingClientRect = () => ({ top: 140, height: 40 }); // mid 160
widgetRows[2].getBoundingClientRect = () => ({ top: 180, height: 40 }); // mid 200

// Verify initial order badges
assert.equal(widgetRows[0].querySelector('.inspector-row-order').textContent, '1');
assert.equal(widgetRows[1].querySelector('.inspector-row-order').textContent, '2');
assert.equal(widgetRows[2].querySelector('.inspector-row-order').textContent, '3');

// 1) Pointer down on first row
widgetRows[0].onpointerdown({ button: 0, clientY: 100, pointerId: 1, target: widgetRows[0] });

// 2) Drag downwards past row 1's midpoint (clientY 170 > 160, < 200) without releasing
globalThis.window.trigger('pointermove', { clientY: 170 });
assert.ok(widgetRows[0].classList.contains('dragging'), 'Dragging row must have dragging class');

// Check dynamic real-time DOM position: row 0 is now between row 1 and row 2
const currentOrder1 = inspectorBody.querySelectorAll('.inspector-row').map((r) => r.dataset.widgetId);
assert.deepEqual(currentOrder1, ['w-todo-2', 'w-time-1', 'w-notes-3'], 'Row 0 must dynamically move between row 1 and 2');

// Check dynamic badge updates in real time
assert.equal(widgetRows[1].querySelector('.inspector-row-order').textContent, '1');
assert.equal(widgetRows[0].querySelector('.inspector-row-order').textContent, '2');
assert.equal(widgetRows[2].querySelector('.inspector-row-order').textContent, '3');

// 3) Drag further downwards past row 2's midpoint (clientY 210 > 200) without releasing
globalThis.window.trigger('pointermove', { clientY: 210 });
const currentOrder2 = inspectorBody.querySelectorAll('.inspector-row').map((r) => r.dataset.widgetId);
assert.deepEqual(currentOrder2, ['w-todo-2', 'w-notes-3', 'w-time-1'], 'Row 0 must dynamically move to the bottom');

// 4) Drag upwards ("上拉调整顺序") back above row 2's midpoint (clientY 150 < 160) without releasing
globalThis.window.trigger('pointermove', { clientY: 150 });
const currentOrder3 = inspectorBody.querySelectorAll('.inspector-row').map((r) => r.dataset.widgetId);
assert.deepEqual(currentOrder3, ['w-time-1', 'w-todo-2', 'w-notes-3'], 'Row 0 must dynamically move back to top when pulled up');

// 5) Drag down between row 1 and row 2, and release mouse ("松手")
globalThis.window.trigger('pointermove', { clientY: 170 });
globalThis.window.trigger('pointerup', { clientY: 170, pointerId: 1 });
assert.equal(widgetRows[0].classList.contains('dragging'), false, 'Dragging class must be removed on pointerup');

// Verify workspace_widget_reorder was queued and committed
assert.equal(lastCall.method, 'workspace_widget_reorder');
assert.deepEqual(lastCall.params.orderedIds, ['w-todo-2', 'w-time-1', 'w-notes-3']);
console.log('✓ Inspector live pointer drag and reorder passed');

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

// B2. Verify Inspector sidebar low-noise design contracts
assert.match(spaceCss, /\.inspector\s*\{[^}]*box-shadow:none;/, 'Inspector on desktop must not have heavy box shadow');
assert.match(spaceCss, /\.inspector-row\s*\{[^}]*background:transparent;/, 'Inspector rows must have transparent low-noise background');
assert.match(spaceCss, /\.inspector-card-clickable\s*\{[^}]*background:transparent;/, 'Inspector background card must have transparent low-noise background');
assert.match(spaceCss, /\.catalog-card\s*\{[^}]*background:transparent;/, 'Catalog cards must have transparent background');
assert.match(spaceCss, /\.catalog-add-btn\s*\{[^}]*background:transparent;/, 'Catalog add button must be a neutral button by default');
assert.match(spaceCss, /\.inspector-row\s*\.row-action-toggle\[aria-pressed="true"\]\s*\{[^}]*color:var\(--accent\);/, 'Row toggle must reflect aria-pressed state');
assert.match(spaceCss, /\.inspector-section-header\s*\{[^}]*justify-content:space-between;/, 'Section header must space title and add button');
assert.match(spaceCss, /\.catalog-search-wrap input\.catalog-search\s*\{[^}]*border:0 !important;/, 'Search input must not have double borders');
assert.match(spaceCss, /\.inspector select\s*\{[^}]*appearance:none;/, 'Select must use unified custom arrow');
assert.match(spaceCss, /\.inspector-row-order/, 'Inspector row order badge must be styled in CSS');
assert.match(spaceCss, /\.inspector-row\.dragging/, 'Inspector row dragging state must be styled in CSS');

// C. Verify all 24 widget and 9 background renderSettings without inline style attributes
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
