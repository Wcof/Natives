import assert from 'node:assert/strict';
import { WIDGET_KEYS, BACKGROUND_KEYS, widgetPlugins, backgroundPlugins } from './space-plugins.js';

console.log('--- Space Plugins Behavior Smoke Tests ---');

// 1. Global Node & Browser Constants
globalThis.Node = {
  ELEMENT_NODE: 1,
  ATTRIBUTE_NODE: 2,
  TEXT_NODE: 3,
};

// 2. Global Browser & Chrome API Stubs
globalThis.fetch = async (url) => {
  const urlStr = String(url);
  if (urlStr.includes('open-meteo')) {
    return {
      json: async () => ({ current: { temperature_2m: 22, weather_code: 0 } }),
    };
  }
  if (urlStr.includes('er-api.com')) {
    return {
      json: async () => ({ rates: { CNY: 7.23, USD: 1 } }),
    };
  }
  if (urlStr.includes('coingecko')) {
    return {
      json: async () => ({ bitcoin: { usd: 65000, cny: 460000 } }),
    };
  }
  if (urlStr.includes('ipify')) {
    return {
      json: async () => ({ ip: '114.114.114.114' }),
    };
  }
  return { json: async () => ({}) };
};

globalThis.chrome = {
  bookmarks: {
    getRecent(count, cb) {
      cb([{ id: '1', title: 'Example', url: 'https://example.com' }]);
    },
  },
  topSites: {
    get(cb) {
      cb([{ title: 'GitHub', url: 'https://github.com' }]);
    },
  },
};

// 3. Minimal DOM Stub
class MockDOMElement {
  constructor(tagName = 'div') {
    this.tagName = tagName.toUpperCase();
    this.nodeType = globalThis.Node.ELEMENT_NODE;
    this.style = {};
    this.dataset = {};
    this.attributes = [];
    this.childNodes = [];
    this.classList = {
      _classes: new Set(),
      add(...cls) { cls.forEach((c) => this._classes.add(c)); },
      remove(...cls) { cls.forEach((c) => this._classes.delete(c)); },
      contains(c) { return this._classes.has(c); },
    };
    this._textContent = '';
    this._innerHTML = '';
    this.onchange = null;
    this.onclick = null;
    this.onkeydown = null;
  }

  get children() {
    return this.childNodes.filter((n) => n.nodeType === globalThis.Node.ELEMENT_NODE);
  }

  get textContent() {
    return this._textContent;
  }
  set textContent(val) {
    this._textContent = String(val);
    this._innerHTML = String(val);
    this.childNodes = [];
  }

  get innerHTML() {
    return this._innerHTML;
  }
  set innerHTML(html) {
    this._innerHTML = html;
    this.childNodes = [];
    const matches = html.matchAll(/<([a-z0-9-]+)([^>]*)>/gi);
    for (const match of matches) {
      const tag = match[1];
      const attrs = match[2];
      const child = new MockDOMElement(tag);
      const idMatch = attrs.match(/id="([^"]+)"/);
      if (idMatch) child.id = idMatch[1];
      const typeMatch = attrs.match(/type="([^"]+)"/);
      if (typeMatch) child.type = typeMatch[1];
      const valMatch = attrs.match(/value="([^"]*)"/);
      if (valMatch) child.value = valMatch[1];
      if (/checked/i.test(attrs)) child.checked = true;
      this.childNodes.push(child);
    }
  }

  append(...nodes) {
    for (const node of nodes) {
      this.childNodes.push(node);
    }
  }

  replaceChildren(...nodes) {
    this.childNodes = [...nodes];
    this._textContent = '';
    this._innerHTML = '';
  }

  setAttribute(k, v) {
    const existing = this.attributes.find((a) => a.name === k);
    if (existing) {
      existing.value = String(v);
    } else {
      this.attributes.push({ name: k, value: String(v) });
    }
  }

  getAttribute(k) {
    const attr = this.attributes.find((a) => a.name === k);
    return attr ? attr.value : null;
  }

  removeAttribute(k) {
    this.attributes = this.attributes.filter((a) => a.name !== k);
  }

  remove() {
    if (this.parentNode) {
      this.parentNode.childNodes = this.parentNode.childNodes.filter((c) => c !== this);
    }
  }

  querySelector(selector) {
    if (selector.startsWith('#')) {
      const id = selector.slice(1);
      return this.children.find((c) => c.id === id) || this.children.find((c) => c.querySelector(selector)) || new MockDOMElement();
    }
    const tag = selector.toUpperCase();
    return this.children.find((c) => c.tagName === tag) || new MockDOMElement(tag);
  }

  querySelectorAll(selector) {
    return this.children.filter((c) => c.tagName === selector.toUpperCase());
  }
}

globalThis.document = {
  createElement(tag) {
    return new MockDOMElement(tag);
  },
};

globalThis.DOMParser = class {
  parseFromString(html) {
    const doc = new MockDOMElement('body');
    doc.innerHTML = html;
    return { body: doc };
  }
};

const mockContext = {
  t: (k, fallback) => fallback || k,
  lang: 'zh_CN',
  shadowRoot: new MockDOMElement('shadow-root'),
  onDataChange: () => {},
};

// 4. Test All 29 Widgets
console.log(`\nTesting ${WIDGET_KEYS.length} Widgets:`);
for (const key of WIDGET_KEYS) {
  const plugin = widgetPlugins[key];
  assert.ok(plugin, `Widget plugin missing for key: ${key}`);
  assert.equal(plugin.key, key, `Plugin key mismatch for: ${key}`);
  assert.ok(typeof plugin.name === 'string' && plugin.name.length > 0, `Plugin name missing: ${key}`);
  assert.ok(plugin.defaultData && typeof plugin.defaultData === 'object', `defaultData missing: ${key}`);
  assert.doesNotThrow(() => JSON.stringify(plugin.defaultData), `defaultData not serializable: ${key}`);
  assert.ok(typeof plugin.render === 'function', `render function missing: ${key}`);

  // Test render
  const container = new MockDOMElement('div');
  assert.doesNotThrow(() => {
    plugin.render(container, plugin.defaultData, { position: 'middleCentre' }, mockContext);
  }, `Widget render failed: ${key}`);

  // Test renderSettings if present
  if (plugin.renderSettings) {
    assert.ok(typeof plugin.renderSettings === 'function', `renderSettings must be function: ${key}`);
    const settingsContainer = new MockDOMElement('div');
    assert.doesNotThrow(() => {
      plugin.renderSettings(settingsContainer, plugin.defaultData, () => {}, mockContext);
    }, `Widget renderSettings failed: ${key}`);
  }

  // Test styles if present
  if (plugin.styles !== undefined) {
    assert.equal(typeof plugin.styles, 'string', `styles must be string: ${key}`);
  }

  console.log(`  ✓ Widget ${key} smoke passed`);
}

// 5. Test All 9 Backgrounds
console.log(`\nTesting ${BACKGROUND_KEYS.length} Backgrounds:`);
for (const key of BACKGROUND_KEYS) {
  const plugin = backgroundPlugins[key];
  assert.ok(plugin, `Background plugin missing for key: ${key}`);
  assert.equal(plugin.key, key, `Plugin key mismatch for: ${key}`);
  assert.ok(typeof plugin.name === 'string' && plugin.name.length > 0, `Plugin name missing: ${key}`);
  assert.ok(plugin.defaultData && typeof plugin.defaultData === 'object', `defaultData missing: ${key}`);
  assert.doesNotThrow(() => JSON.stringify(plugin.defaultData), `defaultData not serializable: ${key}`);
  assert.ok(typeof plugin.render === 'function', `render function missing: ${key}`);

  // Test render
  const container = new MockDOMElement('div');
  assert.doesNotThrow(() => {
    plugin.render(container, plugin.defaultData, mockContext);
  }, `Background render failed: ${key}`);

  // Test renderSettings if present
  if (plugin.renderSettings) {
    assert.ok(typeof plugin.renderSettings === 'function', `renderSettings must be function: ${key}`);
    const settingsContainer = new MockDOMElement('div');
    assert.doesNotThrow(() => {
      plugin.renderSettings(settingsContainer, plugin.defaultData, () => {}, mockContext);
    }, `Background renderSettings failed: ${key}`);
  }

  // Test styles if present
  if (plugin.styles !== undefined) {
    assert.equal(typeof plugin.styles, 'string', `styles must be string: ${key}`);
  }

  console.log(`  ✓ Background ${key} smoke passed`);
}

console.log('\nAll 29 widgets and 9 backgrounds passed behavioral smoke testing!\n');
