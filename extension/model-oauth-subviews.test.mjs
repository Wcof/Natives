import assert from 'node:assert/strict';
import { renderAuthFilesView } from './model-auth-files-view.js';
import { renderQuotaView } from './model-quota-view.js';
import { renderOAuthLoginView } from './model-oauth-login-view.js';

console.log('--- Model OAuth Sub-Views Test ---');

const mockElement = (tag = 'div') => {
  const el = {
    tagName: tag.toUpperCase(),
    children: [],
    childNodes: [],
    classList: {
      classes: new Set(),
      add(cls) { this.classes.add(cls); },
      remove(cls) { this.classes.delete(cls); },
      toggle(cls, b) { if (b) this.classes.add(cls); else this.classes.delete(cls); },
    },
    dataset: {},
    innerHTML: '',
    textContent: '',
    replaceChildren(...nodes) {
      this.children = [...nodes];
      this.childNodes = [...nodes];
    },
    append(...nodes) {
      this.children.push(...nodes);
      this.childNodes.push(...nodes);
    },
    querySelector(sel) {
      return this.children[0] || mockElement();
    },
    querySelectorAll() {
      return this.children;
    },
  };
  return el;
};

globalThis.document = {
  createElement: (t) => mockElement(t),
};

const t = (k, f) => f || k;

// 1. Test OAuth Login View
{
  const c = mockElement();
  renderOAuthLoginView(c, { snapshot: { providers: [] }, pendingOAuth: null, t });
  assert.ok(c.children.length >= 2, 'OAuth login view must render browser bar and cards grid');
  console.log('✓ OAuth Login View rendered successfully');
}

// 2. Test Auth Files View
{
  const c = mockElement();
  const testFiles = [
    { name: 'antigravity-test@gmail.com.json', provider: 'antigravity', account: 'test@gmail.com', status: 'active', disabled: false, size: 1024, updatedAt: new Date().toISOString(), priority: 0 },
    { name: 'xai-test@qq.com.json', provider: 'xai', account: 'test@qq.com', status: 'disabled', disabled: true, size: 2048, updatedAt: new Date().toISOString(), priority: 5 },
  ];
  renderAuthFilesView(c, { files: testFiles, quotaMap: {}, filter: {}, t, onAction: () => {} });
  assert.ok(c.children.length >= 3, 'Auth files view must render topbar, filterbar and list');
  console.log('✓ Auth Files View rendered successfully');
}

// 3. Test Quota View
{
  const c = mockElement();
  const testFiles = [
    { name: 'antigravity-test@gmail.com.json', provider: 'antigravity', account: 'test@gmail.com', disabled: false },
  ];
  const quotaMap = {
    'antigravity-test@gmail.com.json': {
      status: 'success',
      plan: 'Pro',
      windows: [
        { name: 'Gemini Models · Weekly Limit Remaining', remainingPercent: 98, models: ['Gemini Flash', 'Gemini Pro'] },
        { name: 'Gemini Models · Five Hour Limit Remaining', remainingPercent: 100, models: ['Gemini Flash', 'Gemini Pro'] },
      ],
    },
  };
  renderQuotaView(c, { files: testFiles, quotaMap, t, onAction: () => {} });
  assert.ok(c.children.length >= 2, 'Quota view must render topbar and content');
  console.log('✓ Quota View rendered successfully');
}

console.log('All model OAuth sub-views tests passed!\n');
