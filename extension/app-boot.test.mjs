import assert from 'node:assert/strict';
import { setupTestDomEnvironment } from './test-dom-mock.js';

setupTestDomEnvironment();
globalThis.window = new EventTarget();
globalThis.location = new URL('chrome-extension://abcdefghijklmnopabcdefghijklmnop/app.html?app=sample');
globalThis.fetch = async path => {
  assert.match(path, /^_locales\/(zh_CN|en)\/messages\.json$/);
  return { ok: true, json: async () => ({ appName: { message: 'Natives' } }) };
};
for (const id of ['app-stage', 'app-title', 'app-sub', 'app-toast', 'app-back']) {
  const element = document.createElement('div');
  element.id = id;
  document.body.append(element);
}
const ports = [];
let productConfigured = false;
const app = {
  app_id: 'sample', name: 'Sample', version: '1.0.0', enabled: true,
  capabilities: [], requires: [], product_generation: 3, revision: 3,
};
const runtimeHost = 'com.natives.local.app_runtime';
globalThis.chrome = {
  storage: {
    local: { async get(keys) { return typeof keys==='object' && !Array.isArray(keys) ? {...keys} : {}; }, async set() {}, async remove() {} },
    onChanged:{addListener() {},removeListener() {}},
  },
  runtime: {
    getURL: (path) => `chrome-extension://abcdefghijklmnopabcdefghijklmnop/${path}`,
    connectNative(host) {
      const listeners = [];
      const port = {
        host, disconnected: false,
        onMessage: { addListener: (listener) => listeners.push(listener) },
        onDisconnect: { addListener() {} },
        disconnect() { port.disconnected = true; },
        postMessage(request) {
          let result;
          let error;
          if (host === 'com.natives.file_manager') {
            if (request.method === 'preferences_get') result={revision:0,entries:{},sources:[]};
            else if (request.method === 'apps:handshake') { productConfigured = true; result = { ok: true }; }
            else if (request.method === 'apps:list') result = { apps: [app], revision: 3 };
            else if (request.method === 'apps:get' && request.params.appId === app.app_id) { assert.ok(productConfigured, 'first direct entry verifies the product before projection'); result = { app, runtime_host: runtimeHost }; }
            else if (request.method === 'app:handshake') result = { protocolVersion: 2, appId: app.app_id, appVersion: app.version };
            else if (request.method === 'app:start') result = { state: 'ready', port: 49152, instanceId: 'i'.repeat(22), generation: 'g'.repeat(22) };
            else if (request.method === 'app:session') result = { newGeneration: 'n'.repeat(22) };
            else if (request.method === 'app:stop') result = { stopped: true };
            else error = 'unsupported method';
          } else error = 'unsupported host';
          queueMicrotask(() => listeners.forEach((listener) => listener({ id: request.id, ok: !error, result, error })));
        },
      };
      ports.push(port);
      return port;
    },
  },
};

await assert.doesNotReject(import('./app-boot.js?boot-regression'), 'the real page entry must boot');
for (let attempt = 0; attempt < 100 && !document.querySelector('iframe'); attempt++) {
  await new Promise((resolve) => setImmediate(resolve));
}
assert.equal(document.getElementById('app-title').textContent, 'Sample');
assert.equal(document.querySelector('iframe').getAttribute('sandbox'), 'allow-scripts allow-forms');
assert.ok(ports.length >= 2 && ports.every(port => port.host === 'com.natives.file_manager'), 'Core verification and module sessions must both use the unique Core relay');
window.dispatchEvent(new Event('pagehide'));
assert.ok(ports.every((port) => port.disconnected), 'pagehide must disconnect every Native Port');

const tracked = new Map();
for (const method of ['addEventListener', 'removeEventListener']) {
  const original = window[method].bind(window);
  window[method] = (type, listener, options) => {
    if (!tracked.has(type)) tracked.set(type, new Set());
    if (method === 'addEventListener') tracked.get(type).add(listener);
    else tracked.get(type).delete(listener);
    return original(type, listener, options);
  };
}
const { createEmbeddedAppSurface } = await import('./app.js');
const embedded = createEmbeddedAppSurface({
  appId: 'sample', stage: document.createElement('section'), openSettings() {},
});
await embedded.open();
assert.equal(tracked.get('message').size, 1);
await embedded.dispose();
for (const [type, listeners] of tracked) assert.equal(listeners.size, 0, `embedded dispose releases ${type}`);
assert.ok(ports.every(port => port.disconnected));
console.log('app boot: generic managed host, sandbox and EOF cleanup passed');
