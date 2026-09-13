import assert from 'node:assert/strict';
import { setupTestDomEnvironment } from './test-dom-mock.js';

setupTestDomEnvironment();
globalThis.window = new EventTarget();
globalThis.location = new URL('chrome-extension://abcdefghijklmnopabcdefghijklmnop/app.html?app=sample');
for (const id of ['app-stage', 'app-title', 'app-sub', 'app-toast', 'app-back']) {
  const element = document.createElement('div');
  element.id = id;
  document.body.append(element);
}
const ports = [];
const app = {
  app_id: 'sample', name: 'Sample', kind: 'managed_local',
  version: '1.0.0', enabled: true, host_registered: true,
  runtime_host: 'com.natives.app.sample-host', revision: 3,
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
            else error = 'appId is required';
          } else if (host === app.runtime_host) {
            if (request.method === 'app:handshake') result = { protocolVersion: 1, appId: app.app_id, appVersion: app.version };
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

await assert.doesNotReject(import('./app.js?boot-regression'), 'the real page entry must boot');
for (let attempt = 0; attempt < 100 && !document.querySelector('iframe'); attempt++) {
  await new Promise((resolve) => setImmediate(resolve));
}
assert.equal(document.getElementById('app-title').textContent, 'Sample');
assert.equal(document.querySelector('iframe').getAttribute('sandbox'), 'allow-scripts allow-forms');
assert.ok(ports.some((port) => port.host === app.runtime_host), 'app must connect directly to its registered host');
window.dispatchEvent(new Event('pagehide'));
assert.ok(ports.every((port) => port.disconnected), 'pagehide must disconnect every Native Port');
console.log('app boot: generic managed host, sandbox and EOF cleanup passed');
