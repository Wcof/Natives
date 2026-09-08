import assert from 'node:assert/strict';
import { setupTestDomEnvironment } from './test-dom-mock.js';

setupTestDomEnvironment();
globalThis.window = new EventTarget();
globalThis.location = new URL('chrome-extension://abcdefghijklmnopabcdefghijklmnop/app.html?app=com.natives.app.demo');
for (const id of ['app-stage', 'app-title', 'app-sub', 'app-toast', 'app-back']) {
  const element = document.createElement('div');
  element.id = id;
  document.body.append(element);
}
const ports = [];
const app = {
  app_id: 'com.natives.app.demo', name: 'Demo', kind: 'extension_app',
  version: '1.0.0', enabled: true, host_registered: true,
  runtime_spec_json: '{"host":"com.natives.app.demo"}',
};
globalThis.chrome = {
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
            if (request.method === 'apps:get' && request.params.appId === app.app_id) result = { app, packages: [], permissions: [] };
            else error = 'appId is required';
          } else if (request.method === 'ping') result = { pong: true };
          else if (request.method === 'version') result = { version: '1.0.0', host };
          else if (request.method === 'health') result = { ok: true };
          else error = 'unsupported method';
          queueMicrotask(() => listeners.forEach((listener) => listener({ id: request.id, ok: !error, result, error })));
        },
      };
      ports.push(port);
      return port;
    },
  },
};

await assert.doesNotReject(import('./app.js?boot-regression'), 'the real page entry must boot');
for (let attempt = 0; attempt < 100 && !document.getElementById('demo-host-status'); attempt++) {
  await new Promise((resolve) => setImmediate(resolve));
}
assert.equal(document.getElementById('app-title').textContent, 'Demo');
assert.ok(document.getElementById('demo-host-status'), 'the installed UI must mount from the page entry');
assert.ok(ports.some((port) => port.host === 'com.natives.app.demo'), 'the installed runtime must connect');
window.dispatchEvent(new Event('pagehide'));
assert.ok(ports.every((port) => port.disconnected), 'pagehide must disconnect every Native Port');
console.log('app boot: page entry, Host contract, runtime mount and EOF cleanup passed');
