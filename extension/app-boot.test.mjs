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
  version: '2.0.0', enabled: true, host_registered: false,
  runtime_spec_json: '{"version":"2.0.0"}',
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
            else if (request.method === 'apps:read_resource') {
              result = { ok: true, app_id: app.app_id, package_id: 'demo-data', format: 'json', total_size: 10, data: Buffer.from('{"ok":true}').toString('base64') };
            }
            else error = 'appId is required';
          } else error = 'unsupported host';
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
assert.ok(ports.some((port) => port.host === 'com.natives.file_manager'), 'app must connect to shared native-file-host');
window.dispatchEvent(new Event('pagehide'));
assert.ok(ports.every((port) => port.disconnected), 'pagehide must disconnect every Native Port');
console.log('app boot: page entry, Host contract, resource mount and EOF cleanup passed');
