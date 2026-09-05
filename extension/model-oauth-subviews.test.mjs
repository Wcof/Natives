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
    setAttribute(k, v) { this[k] = v; },
    focus() {},
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

// 4. Test Icon & Text Alignment CSS Contracts
{
  const { readFile } = await import('node:fs/promises');
  const css = await readFile(new URL('./model-settings.css', import.meta.url), 'utf8');
  assert.match(css, /\.model-oauth-login-btn\s*\{[^}]*display:\s*inline-flex;[^}]*align-items:\s*center;[^}]*justify-content:\s*center;[^}]*gap:\s*6px;/, 'OAuth login button must declare inline-flex centered alignment with 6px gap');
  assert.match(css, /\.btn-af-action\s*\{[^}]*display:\s*inline-flex;[^}]*align-items:\s*center;[^}]*justify-content:\s*center;[^}]*gap:\s*6px;/, 'Auth files action button must declare inline-flex centered alignment');
  assert.match(css, /\.btn-quota-action\s*\{[^}]*display:\s*inline-flex;[^}]*align-items:\s*center;[^}]*justify-content:\s*center;[^}]*gap:\s*6px;/, 'Quota action button must declare inline-flex centered alignment');
  assert.match(css, /\.btn-af-tool\s*\{[^}]*display:\s*inline-flex;[^}]*align-items:\s*center;[^}]*justify-content:\s*center;[^}]*gap:\s*5px;/, 'Row tool buttons must declare inline-flex centered alignment');
  assert.match(css, /\.model-quota-loading\s*\{[^}]*display:\s*inline-flex;[^}]*align-items:\s*center;[^}]*gap:\s*8px;/, 'Quota loading indicator must align icon and text with inline-flex');
  console.log('✓ Model OAuth CSS alignment and icon sizing contracts verified');
}

// 5. Test Account Models Dialog For Auth File
{
  const { openAccountModelsDialog } = await import('./model-account-models-dialog.js');
  let getParams = null;
  let updateParams = null;
  const mockController = {
    t: (k, f) => f || k,
    snapshot: { revision: 1 },
    view: { showNotice: () => {} },
    showError: (err) => { throw err; },
    api: {
      getAccountModels: async (params) => {
        getParams = params;
        return { models: [{ id: 'gemini-2.5-pro', displayName: 'Gemini 2.5 Pro', enabled: true }] };
      },
      updateAccountModels: async (params) => {
        updateParams = params;
        return { ok: true };
      },
    },
  };
  globalThis.document.body = mockElement('body');
  mockElement.prototype = mockElement();
  const dlg = mockElement('dialog');
  dlg.showModal = () => {};
  dlg.close = () => {};
  dlg.remove = () => {};
  dlg.addEventListener = () => {};
  const origCreate = globalThis.document.createElement;
  globalThis.document.createElement = (tag) => {
    if (tag === 'dialog') return dlg;
    return mockElement(tag);
  };

  await openAccountModelsDialog(mockController, { name: 'gemini-key.json', provider: 'gemini' });
  assert.equal(getParams?.name, 'gemini-key.json', 'openAccountModelsDialog must query models by file name');
  globalThis.document.createElement = origCreate;
  console.log('✓ Account Models Dialog for Auth File contract verified');
}

console.log('All model OAuth sub-views tests passed!\n');

// A saved OAuth account must remain visibly signed in after reopening settings.
{
  const c = mockElement();
  renderOAuthLoginView(c, { snapshot: { accounts: [{ id: 'ag', provider: 'antigravity', enabled: true, status: 'active' }] }, pendingOAuth: null, t });
  const html = c.children[1].children.map((card) => card.innerHTML).join('');
  assert.match(html, /已登录/, 'saved accounts must show sign-in success');
  assert.match(html, /role="status"/, 'sign-in state must be accessible');
  renderOAuthLoginView(c, { snapshot: { accounts: [] }, pendingOAuth: null, results: { antigravity: 'failed' }, t });
  assert.match(c.children[1].children.map((card) => card.innerHTML).join(''), /登录失败/);
}
