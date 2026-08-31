/**
 * Shared DOM and Chrome Environment Mock for Extension Unit Tests (<120 lines).
 */

globalThis.Node = { ELEMENT_NODE: 1, ATTRIBUTE_NODE: 2, TEXT_NODE: 3 };

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
    if (this.childNodes.length > 0) return this.childNodes.map((n) => n.textContent).join(' ');
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
    this._parseHtmlInto(this, html);
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
    const idMatch = sel.match(/#([a-zA-Z0-9_-]+)/);
    const tagMatch = sel.match(/^[a-zA-Z0-9_-]+/);
    const isCheckbox = sel.includes('[type="checkbox"]');
    const classMatches = [...sel.matchAll(/\.([a-zA-Z0-9_-]+)/g)].map((m) => m[1]);

    const results = [];
    const walk = (node) => {
      for (const child of node.childNodes) {
        if (child.nodeType === globalThis.Node.ELEMENT_NODE) {
          let match = true;
          if (idMatch && child.id !== idMatch[1]) match = false;
          if (tagMatch && !sel.startsWith('.') && !sel.startsWith('#') && child.tagName !== tagMatch[0].toUpperCase()) match = false;
          if (classMatches.length > 0 && !classMatches.some((c) => child.classList?.contains(c))) match = false;
          if (isCheckbox && (child.tagName !== 'INPUT' || child.type !== 'checkbox')) match = false;
          if (match) results.push(child);
          walk(child);
        }
      }
    };
    walk(this);
    return results;
  }
}

export function setupTestDomEnvironment() {
  const bodyEl = new MockElement('body');
  globalThis.document = {
    body: bodyEl,
    createElement: (tag) => new MockElement(tag),
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
