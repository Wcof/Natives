import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createModelSettingsAPI } from './model-settings-api.js';

class FakePort {
  constructor() {
    this.messages = [];
    this.messageListeners = [];
    this.disconnectListeners = [];
    this.onMessage = { addListener: (listener) => this.messageListeners.push(listener) };
    this.onDisconnect = { addListener: (listener) => this.disconnectListeners.push(listener) };
  }
  postMessage(message) {
    this.messages.push(message);
    queueMicrotask(() => this.emit({ id: message.id, ok: true, result: message.method === 'model_snapshot' ? { revision: 1, providers: [], resident: false } : { accepted: message.params } }));
  }
  emit(message) { for (const listener of this.messageListeners) listener(message); }
  disconnect() { for (const listener of this.disconnectListeners) listener(); }
}

const port = new FakePort();
let connectedHost;
globalThis.__NATIVES_DEV_NATIVE_CONNECT__ = (host) => { connectedHost = host; return port; };
let event;
const api = createModelSettingsAPI({ onEvent: (message) => { event = message; } });
assert.equal((await api.snapshot()).revision, 1);
assert.equal(connectedHost, 'com.natives.file_manager');
const setup = { name: 'Global AI', baseUrl: 'https://example.test/v1', protocol: 'openai_chat', apiKey: 'synthetic', modelId: 'model::version', setAsDefault: true, expectedRevision: 1 };
assert.deepEqual((await api.createProvider(setup)).accepted, setup);
const tested = await api.testProvider({ providerId: 'custom', baseUrl: 'https://example.test/v1', protocol: 'openai_chat', apiKey: 'temporary', allowLan: false });
assert.deepEqual(tested.accepted, { providerId: 'custom', baseUrl: 'https://example.test/v1', protocol: 'openai_chat', apiKey: 'temporary', allowLan: false });
assert.deepEqual((await api.getUsageOverview({ range: 'today' })).accepted, { range: 'today' });
assert.deepEqual((await api.setResident({ expectedRevision: 1, resident: true })).accepted, { expectedRevision: 1, resident: true });
assert.equal(typeof api.startGateway, 'undefined', 'gateway API must be gone');
assert.equal(typeof api.createGatewayKey, 'undefined', 'gateway key API must be gone');
assert.equal(typeof api.startOAuth, 'undefined', 'OAuth login API must be gone');
assert.equal(typeof api.getAccountModels, 'undefined', 'account model routing API must be gone');
assert.equal(typeof api.syncAgentClientConfigs, 'undefined', 'agent config injection API must be gone');
assert.equal(typeof api.checkKernelUpdate, 'undefined', 'kernel update API must be gone');

port.emit({ ok: true, event: 'model_usage_updated', result: { updatedAt: '2026-08-31T00:00:00Z' } });
assert.equal(event.event, 'model_usage_updated');

api.disconnect();
delete globalThis.__NATIVES_DEV_NATIVE_CONNECT__;

const [files, space, controller, view, zhRaw, enRaw, css, usageView] = await Promise.all([
  readFile(new URL('./files.js', import.meta.url), 'utf8'),
  readFile(new URL('./space.js', import.meta.url), 'utf8'),
  readFile(new URL('./model-settings.js', import.meta.url), 'utf8'),
  readFile(new URL('./model-settings-view.js', import.meta.url), 'utf8'),
  readFile(new URL('./_locales/zh_CN/messages.json', import.meta.url), 'utf8'),
  readFile(new URL('./_locales/en/messages.json', import.meta.url), 'utf8'),
  readFile(new URL('./model-settings.css', import.meta.url), 'utf8'),
  readFile(new URL('./model-usage-view.js', import.meta.url), 'utf8'),
]);
for (const source of [files, space]) assert.match(source, /import\('\.\/model-settings\.js'\)/, 'model settings must stay lazy-loaded');
for (const source of [controller, view]) assert.doesNotMatch(source, /\b(?:alert|prompt|confirm)\s*\(/, 'model settings must use graphical controls');

const declaredRoles = new Set([...view.matchAll(/data-role="([^"]+)"/g)].map(([, role]) => role));
for (const [, role] of view.matchAll(/\broles\.([A-Za-z]\w*)/g)) {
  assert.ok(declaredRoles.has(role), `view references missing data-role="${role}"`);
}
assert.match(view, /showModal\(\)/);
assert.match(view, /aria-live="polite"/);
assert.match(view, /data-action="test-new-provider"/);
for (const page of ['custom', 'usage', 'quota', 'apps']) {
  assert.match(view, new RegExp(`data-action="select-model-page" data-page="${page}"`), `model settings must expose the ${page} second-level page`);
  assert.match(view, new RegExp(`data-page-panel="${page}"`), `model settings must render the ${page} page`);
}
for (const removed of ['oauth', 'gateway', 'agent', 'advanced']) {
  assert.doesNotMatch(view, new RegExp(`data-page="${removed}"`), `retired ${removed} page must not be exposed`);
  assert.doesNotMatch(view, new RegExp(`data-page-panel="${removed}"`), `retired ${removed} page must not be rendered`);
}
assert.match(view, /data-role="appsNavGroup"/, 'primary nav group for apps center must exist');
assert.match(view, /data-role="appsListNav"/, 'secondary nav item for app list must exist');
assert.match(view, /data-role="appsList"/, 'apps list container must exist');
assert.match(view, /data-action="resident"/, 'resident lifecycle control must remain available');
assert.match(view, /modelProxyRetiredNotice/, 'proxy retirement notice must be visible');
assert.match(controller, /import\('\.\/apps\.js'\)/, 'apps center module must stay lazy-loaded');
assert.match(controller, /action === 'select-model-page'/, 'controller must switch model settings subpages');
assert.match(controller, /model_usage_updated/);
assert.doesNotMatch(controller, /startGateway|syncAgentClientConfigs|updateKernel|startOAuth|accountModels/, 'retired proxy side effects must not remain');
assert.match(controller, /form\.dataset\.role\?\.startsWith\('usage-'\)/, 'usage forms must prevent native submission before awaiting work');
assert.match(usageView, /selectFilter\('provider'/);
assert.match(css, /\.model-settings-shell\s*\{[^}]*grid-template-columns:240px/, 'model navigation must use the compact shared width');
assert.match(css, /\.model-toast\.visible\s*\{[^}]*opacity:1;/, 'model settings must provide a visible toast state');
assert.match(view, /showToast\(message\)/, 'model settings view must expose showToast');
for (const [page, icon] of [['custom', 'i-pen'], ['usage', 'i-list'], ['quota', 'i-globe'], ['apps', 'i-grid']]) {
  assert.match(view, new RegExp(`data-page="${page}"[^>]*><svg class="icon"[^>]*><use href="#${icon}"`), `subnav ${page} must declare its ${icon} icon inline`);
}

const zh = JSON.parse(zhRaw);
const en = JSON.parse(enRaw);
for (const key of ['modelDefaultModel', 'modelDefaultModelNone', 'modelDefaultModelHint', 'modelDefaultModelUnavailable', 'modelSetAsDefault']) {
  assert.ok(zh[key] && en[key], `global connection locale ${key} must exist in both languages`);
}
for (const key of ['modelSettings', 'modelConnections', 'modelConnectionsDescription', 'modelUsageRecords', 'modelQuotaInquiry', 'modelQuotaDescription', 'modelProxyRetiredNotice', 'modelResidentHost', 'modelAISettings', 'modelTestConnection', 'appsCenter', 'appsList']) {
  assert.ok(zh[key] && en[key], `locale key ${key} must exist in zh_CN and en`);
}
const errorsSource = await readFile(new URL('./model-settings-errors.js', import.meta.url), 'utf8');
for (const key of new Set([...errorsSource.matchAll(/model(?:Error|Host)[A-Za-z]+/g)].map(([value]) => value))) {
  assert.ok(zh[key] && en[key], `safe error locale ${key} must exist in zh_CN and en`);
}

console.log('model settings API, lazy-load, accessibility, and locale tests passed');
