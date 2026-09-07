/**
 * Shared DOM and Chrome Environment Mock for Extension Unit Tests (<120 lines).
 */

globalThis.Node = { ELEMENT_NODE: 1, ATTRIBUTE_NODE: 2, TEXT_NODE: 3 };
// NOTE: do NOT override globalThis.Event — Node ≥18 ships a real Event with
// stopPropagation()/preventDefault(); tests rely on those.

export class MockElement {
  constructor(tagName = 'div') {
    this.tagName = tagName.toUpperCase();
    this.nodeType = globalThis.Node.ELEMENT_NODE;
    this.id = '';
    this.type = '';
    this.value = '';
    this.style = {};
    this.dataset = {};
    this.attributes = [];
    this.childNodes = [];
    this.parentNode = null;
    this.classList = {
      _classes: new Set(),
      add(...cls) { cls.forEach((c) => this._classes.add(c)); },
      remove(...cls) { cls.forEach((c) => this._classes.delete(c)); },
      contains(c) { return this._classes.has(c); },
    };
    this._textContent = '';
  }

  get className() { return Array.from(this.classList._classes).join(' '); }
  set className(val) {
    this.classList._classes.clear();
    String(val).split(/\s+/).filter(Boolean).forEach((c) => this.classList._classes.add(c));
  }

  get children() { return this.childNodes.filter((n) => n.nodeType === globalThis.Node.ELEMENT_NODE); }
  get textContent() {
    if (this.childNodes.length > 0) {
      const parts = [];
      if (this._textContent) parts.push(this._textContent);
      for (const n of this.childNodes) {
        if (n.textContent) parts.push(n.textContent);
      }
      return parts.join(' ');
    }
    return this._textContent;
  }
  set textContent(val) {
    this._textContent = String(val);
    this.childNodes = [];
  }

  get innerHTML() {
    if (this.childNodes.length > 0) {
      return this.childNodes.map((n) => {
        const tag = n.tagName.toLowerCase();
        const attrs = n.attributes.map((a) => ` ${a.name}="${a.value}"`).join('');
        return `<${tag}${attrs}>${n.innerHTML || n.textContent}</${tag}>`;
      }).join('');
    }
    return this._textContent;
  }
  set innerHTML(html) {
    this.childNodes = [];
    const str = String(html || '');
    if (!str.includes('<')) {
      this._textContent = str;
      return;
    }
    // Extract leading text before the first child tag
    const leading = str.replace(/<[\s\S]*$/, '').trim();
    this._textContent = leading;
    this._parseHtmlInto(this, str);
  }

  _parseHtmlInto(parent, html) {
    const tagMatch = /<([a-z0-9-]+)([^>]*)>(.*?)<\/\1>|<([a-z0-9-]+)([^>]*)\/?>/gis;
    let match;
    while ((match = tagMatch.exec(html)) !== null) {
      const tag = match[1] || match[4];
      const attrs = match[2] || match[5] || '';
      const inner = match[3] || '';
      const child = new MockElement(tag);
      child.parentNode = parent;

      const idMatch = attrs.match(/id="([^"]+)"/);
      if (idMatch) child.id = idMatch[1];
      const classMatch = attrs.match(/class="([^"]+)"/);
      if (classMatch) child.className = classMatch[1];
      const typeMatch = attrs.match(/type="([^"]+)"/);
      if (typeMatch) child.type = typeMatch[1];
      const valMatch = attrs.match(/value="([^"]*)"/);
      if (valMatch) child.value = valMatch[1];
      if (/checked/i.test(attrs)) child.checked = true;

      for (const dMatch of attrs.matchAll(/data-([a-zA-Z0-9-]+)="([^"]*)"/g)) {
        child.dataset[dMatch[1]] = dMatch[2];
      }

      if (inner) {
        if (inner.includes('<')) {
          child.innerHTML = inner;
        } else {
          child.textContent = inner;
        }
      }
      parent.childNodes.push(child);
    }
  }

  append(...nodes) {
    nodes.forEach((n) => {
      if (typeof n === 'string' || typeof n === 'number') {
        const textNode = new MockElement('#text');
        textNode.nodeType = globalThis.Node.TEXT_NODE;
        textNode._textContent = String(n);
        n = textNode;
      }
      n.parentNode = this;
      this.childNodes.push(n);
    });
  }
  replaceChildren(...nodes) {
    this.childNodes = [];
    this.append(...nodes);
    this._textContent = '';
  }
  setAttribute(k, v) {
    const existing = this.attributes.find((a) => a.name === k);
    if (existing) existing.value = String(v);
    else this.attributes.push({ name: k, value: String(v) });
  }
  getAttribute(k) {
    const a = this.attributes.find((attr) => attr.name === k);
    return a ? a.value : null;
  }
  removeAttribute(k) { this.attributes = this.attributes.filter((a) => a.name !== k); }
  addEventListener(evt, fn) {
    if (!this._listeners) this._listeners = {};
    if (!this._listeners[evt]) this._listeners[evt] = [];
    this._listeners[evt].push(fn);
  }
  removeEventListener(evt, fn) {
    if (this._listeners?.[evt]) {
      this._listeners[evt] = this._listeners[evt].filter((f) => f !== fn);
    }
  }
  dispatchEvent(evt) {
    const handlers = this._listeners?.[evt.type || evt] || [];
    handlers.forEach((h) => h(evt));
  }
  focus() {}
  remove() {
    if (this.parentNode) {
      this.parentNode.childNodes = this.parentNode.childNodes.filter((c) => c !== this);
    }
  }
  closest(sel) {
    let p = this.parentNode;
    while (p) {
      if (sel.startsWith('.')) {
        if (p.classList?.contains(sel.slice(1))) return p;
      }
      p = p.parentNode;
    }
    return null;
  }
  querySelector(sel) { return this.querySelectorAll(sel)[0] || null; }
  querySelectorAll(sel) {
    const parts = sel.trim().split(/\s+/).map(parseCompoundSelector);
    const results = [];
    const walk = (node, index) => {
      for (const child of node.childNodes) {
        if (child.nodeType !== globalThis.Node.ELEMENT_NODE) continue;
        if (!matchesCompound(child, parts[index])) {
          walk(child, index);
          continue;
        }
        if (index === parts.length - 1) results.push(child);
        else walk(child, index + 1);
      }
    };
    walk(this, 0);
    return results;
  }
}

// Supports: tag, #id, .class (AND), [attr="value"] / [data-x="value"],
// and the descendant combinator via whitespace between compounds.
function parseCompoundSelector(sel) {
  const compound = { tag: null, id: null, classes: [], attrs: [] };
  // mask [attr="value"] groups first so dots/ids inside values are not
  // misread as classes or #id selectors
  const masked = sel.replace(/\[([a-zA-Z0-9-]+)="([^"]*)"\]/g, (_m, name, value) => {
    compound.attrs.push([name, value]);
    return ' ';
  });
  const tagMatch = masked.match(/^([a-zA-Z][a-zA-Z0-9-]*)/);
  if (tagMatch) compound.tag = tagMatch[1].toUpperCase();
  const idMatch = masked.match(/#([a-zA-Z0-9_-]+)/);
  if (idMatch) compound.id = idMatch[1];
  for (const m of masked.matchAll(/\.([a-zA-Z0-9_-]+)/g)) compound.classes.push(m[1]);
  return compound;
}

function matchesCompound(node, compound) {
  if (!compound.tag && !compound.id && compound.classes.length === 0 && compound.attrs.length === 0) return false;
  if (compound.tag && node.tagName !== compound.tag) return false;
  if (compound.id && node.id !== compound.id) return false;
  if (compound.classes.length > 0 && !compound.classes.every((cls) => node.classList?.contains(cls))) return false;
  for (const [name, value] of compound.attrs) {
    let actual = node.getAttribute(name);
    if (actual === null && name.startsWith('data-')) {
      // dataset camelCase: data-app-id ↔ dataset.appId (matches real DOM)
      const key = name.slice(5).replace(/-([a-z])/g, (_, c) => c.toUpperCase());
      actual = node.dataset?.[key];
    }
    if (actual === null && name === 'type') actual = node.type;
    if (actual !== value) return false;
  }
  return true;
}

export function setupTestDomEnvironment() {
  const bodyEl = new MockElement('body');
  globalThis.document = {
    body: bodyEl,
    documentElement: new MockElement('html'),
    createElement: (tag) => new MockElement(tag),
    createElementNS: (_namespace, tag) => new MockElement(tag),
    getElementById: (id) => bodyEl.querySelector(`#${id}`),
    querySelector: (sel) => bodyEl.querySelector(sel),
    querySelectorAll: (sel) => bodyEl.querySelectorAll(sel),
    addEventListener: () => {},
    removeEventListener: () => {},
  };
  globalThis.window = {
    addEventListener: () => {},
    removeEventListener: () => {},
    innerWidth: 1440,
    innerHeight: 900,
  };
  globalThis.DOMParser = class {
    parseFromString(html) {
      const doc = new MockElement('body');
      doc.innerHTML = html;
      return { body: doc };
    }
  };
}
