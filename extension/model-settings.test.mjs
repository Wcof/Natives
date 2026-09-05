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
    queueMicrotask(() => this.emit({ id: message.id, ok: true, result: message.method === 'model_snapshot' ? { revision: 1, providers: [], accounts: [], gateway: { state: 'stopped' } } : { accepted: message.params } }));
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
assert.equal(connectedHost, 'com.natives.model_host');
const started = await api.startGateway({ expectedRevision: 1 });
assert.deepEqual(started.accepted, { expectedRevision: 1 });
const probed = await api.testProvider({ providerId: 'custom', baseUrl: 'https://example.test/v1', protocol: 'openai_chat', apiKey: 'temporary', allowLan: false });
assert.deepEqual(probed.accepted, { providerId: 'custom', baseUrl: 'https://example.test/v1', protocol: 'openai_chat', apiKey: 'temporary', allowLan: false });
const usageOverview = await api.getUsageOverview({ range: 'today' });
assert.deepEqual(usageOverview.accepted, { range: 'today' });
const createdKey = await api.createGatewayKey({ expectedRevision: 1, name: 'Test Key' });
assert.deepEqual(createdKey.accepted, { expectedRevision: 1, name: 'Test Key' });

port.emit({ ok: true, event: 'model_oauth_state_changed', result: { state: 'succeeded' } });
assert.equal(event.event, 'model_oauth_state_changed');
port.emit({ ok: true, event: 'model_usage_updated', result: { updatedAt: '2026-08-31T00:00:00Z' } });
assert.equal(event.event, 'model_usage_updated');

api.disconnect();
delete globalThis.__NATIVES_DEV_NATIVE_CONNECT__;

const [files, space, controller, view, zh, en, advancedController, css] = await Promise.all([
  readFile(new URL('./files.js', import.meta.url), 'utf8'),
  readFile(new URL('./space.js', import.meta.url), 'utf8'),
  readFile(new URL('./model-settings.js', import.meta.url), 'utf8'),
  readFile(new URL('./model-settings-view.js', import.meta.url), 'utf8'),
  readFile(new URL('./_locales/zh_CN/messages.json', import.meta.url), 'utf8'),
  readFile(new URL('./_locales/en/messages.json', import.meta.url), 'utf8'),
  readFile(new URL('./model-advanced-controller.js', import.meta.url), 'utf8'),
  readFile(new URL('./model-settings.css', import.meta.url), 'utf8'),
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
for (const page of ['custom', 'oauth', 'gateway', 'usage', 'advanced']) {
  assert.match(view, new RegExp(`data-action="select-model-page" data-page="${page}"`), `model settings must expose the ${page} second-level page`);
  assert.match(view, new RegExp(`data-page-panel="${page}"`), `model settings must render the ${page} page`);
}
assert.match(controller, /action === 'select-model-page'/, 'controller must switch model settings subpages');
assert.match(controller, /model_catalog_changed/);
assert.match(controller, /model_usage_updated/);
assert.match(controller, /form\.dataset\.role\?\.startsWith\('usage-'\)/, 'usage forms must prevent native submission before awaiting work');
assert.doesNotMatch(controller, /await handleAdvancedSubmit\([^\n]+\)\) \{ event\.preventDefault/, 'advanced forms must prevent native submission synchronously');
const usageView = await readFile(new URL('./model-usage-view.js', import.meta.url), 'utf8');
const pricingView = await readFile(new URL('./model-pricing-view.js', import.meta.url), 'utf8');
const advancedView = await readFile(new URL('./model-advanced-view.js', import.meta.url), 'utf8');
const gatewayView = await readFile(new URL('./model-gateway-view.js', import.meta.url), 'utf8');

assert.match(usageView, /selectFilter\('provider'/);
assert.match(usageView, /dataset\.role = 'usage-custom-range-form'/);
assert.match(usageView, /model-filter-bar-dimensions/, 'dimensions filter bar modifier class must exist');
assert.match(usageView, /model-col-time[\s\S]*model-col-model[\s\S]*model-col-cost/, 'events table must declare semantic column classes');
assert.match(pricingView, /model-filter-bar-price/, 'pricing form modifier class must exist');
assert.match(pricingView, /model-col-model[\s\S]*model-col-num[\s\S]*model-col-actions/, 'pricing table must declare semantic column classes');
assert.match(advancedView, /model-col-name[\s\S]*model-col-mask[\s\S]*model-col-actions/, 'keys table must declare semantic column classes');
assert.match(gatewayView, /data-action=\"resident\"/, 'gateway overview must preserve the resident control');
assert.match(gatewayView, /#i-box[\s\S]*'i-link'/, 'gateway overview must reuse the shared SVG sprite');
assert.doesNotMatch(gatewayView, /127\.0\.0\.1:8317|v7\.2\.139|v0\.2\.25/, 'gateway overview must not invent runtime values');
assert.doesNotMatch(gatewayView, /<svg[^>]+viewBox=/, 'gateway overview must not embed standalone icon artwork');
assert.match(css, /\.model-oauth-row\s*\{[^}]*grid-template-columns:/, 'oauth row must use shared grid template');
assert.match(css, /\.model-filter-bar-dimensions\s*\{[^}]*repeat\(6,/, 'dimensions filter must arrange in 6 equal columns on wide screens');
assert.match(css, /\.model-adv-form\s*\{[^}]*repeat\(2,/, 'advanced form must use 2-column grid layout');
assert.match(css, /\.model-settings-shell\s*\{[^}]*grid-template-columns:240px/, 'model navigation must use the compact shared width');
assert.match(css, /\.model-settings-header,\.model-settings-pages,\.model-settings-notice\s*\{[^}]*1120px/, 'model content must use the shared maximum width');
assert.match(css, /\.model-settings-subnav button\s*\{[^}]*display:\s*flex;[^}]*align-items:\s*center;/, 'subnav buttons must center their label vertically with flex');
assert.match(css, /\.model-settings-subnav button \.icon\s*\{[^}]*width:\s*15px;[^}]*height:\s*15px;/, 'subnav icons must use the shared 15px sizing');
assert.match(css, /\.model-gateway-layout\s*\{[^}]*grid-template-areas:"runtime endpoints" "keys keys" "summary summary";/, 'gateway layout must place runtime and endpoints side by side with keys and summary full width');
assert.match(css, /\.model-toast\.visible\s*\{[^}]*opacity:1;/, 'model settings must provide a visible toast state for action feedback');
assert.match(view, /showToast\(message\)/, 'model settings view must expose showToast for action feedback');
assert.match(advancedController, /view\.showToast\(controller\.t\(key, fallback\)\)/, 'key copy and rotate actions must surface a toast on success');
for (const [page, icon] of [['custom', 'i-pen'], ['oauth', 'i-globe'], ['gateway', 'i-bolt'], ['usage', 'i-list'], ['advanced', 'i-gear']]) {
  assert.match(view, new RegExp(`data-page="${page}"[^>]*><svg class="icon"[^>]*><use href="#${icon}"`), `subnav ${page} must declare its ${icon} icon inline`);
}
assert.doesNotMatch(usageView, /data-action="search-events"/, 'usage UI must not expose a fake search action');
for (const key of ['modelSettings', 'modelCustomModels', 'modelOAuthModels', 'modelLocalProxy', 'modelUsageRecords', 'modelAdvancedSettings', 'modelGatewaySummary', 'modelTestConnection', 'modelAccountNeedsReauth', 'modelOAuthModelsHint', 'modelAccounts']) {
  assert.ok(JSON.parse(zh)[key] && JSON.parse(en)[key], `locale key ${key} must exist in zh_CN and en`);
}
for (const key of new Set(controller.match(/modelError[A-Za-z]+/g))) {
  assert.ok(JSON.parse(zh)[key] && JSON.parse(en)[key], `safe error locale ${key} must exist in zh_CN and en`);
}

console.log('model settings API, lazy-load, accessibility, and locale tests passed');
