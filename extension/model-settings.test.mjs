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
port.emit({ ok: true, event: 'model_oauth_state_changed', result: { state: 'succeeded' } });
assert.equal(event.event, 'model_oauth_state_changed');
api.disconnect();
delete globalThis.__NATIVES_DEV_NATIVE_CONNECT__;

const [files, space, controller, view, zh, en] = await Promise.all([
  readFile(new URL('./files.js', import.meta.url), 'utf8'),
  readFile(new URL('./space.js', import.meta.url), 'utf8'),
  readFile(new URL('./model-settings.js', import.meta.url), 'utf8'),
  readFile(new URL('./model-settings-view.js', import.meta.url), 'utf8'),
  readFile(new URL('./_locales/zh_CN/messages.json', import.meta.url), 'utf8'),
  readFile(new URL('./_locales/en/messages.json', import.meta.url), 'utf8'),
]);
for (const source of [files, space]) assert.match(source, /import\('\.\/model-settings\.js'\)/, 'model settings must stay lazy-loaded');
for (const source of [controller, view]) assert.doesNotMatch(source, /\b(?:alert|prompt|confirm)\s*\(/, 'model settings must use graphical controls');
assert.match(view, /showModal\(\)/);
assert.match(view, /aria-live="polite"/);
assert.match(view, /data-action="test-new-provider"/);
assert.match(controller, /model_catalog_changed/);
for (const key of ['modelSettings', 'modelGatewaySummary', 'modelTestConnection', 'modelAccountNeedsReauth', 'modelOAuthModelsHint', 'modelAccounts']) {
  assert.ok(JSON.parse(zh)[key] && JSON.parse(en)[key], `locale key ${key} must exist in zh_CN and en`);
}
for (const key of new Set(controller.match(/modelError[A-Za-z]+/g))) {
  assert.ok(JSON.parse(zh)[key] && JSON.parse(en)[key], `safe error locale ${key} must exist in zh_CN and en`);
}

console.log('model settings API, lazy-load, accessibility, and locale tests passed');
