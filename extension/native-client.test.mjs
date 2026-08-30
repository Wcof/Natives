import assert from 'node:assert/strict';
import { createNativeClient } from './native-client.js';

function fakePort() {
  const messages = [];
  const disconnects = [];
  return {
    onMessage: { addListener(listener) { messages.push(listener); } },
    onDisconnect: { addListener(listener) { disconnects.push(listener); } },
    postMessage(request) { queueMicrotask(() => messages.forEach((listener) => listener({ id: request.id, ok: true, result: { method: request.method } }))); },
    disconnect() { disconnects.forEach((listener) => listener()); },
    emit(message) { messages.forEach((listener) => listener(message)); },
  };
}

const port = fakePort();
const events = [];
const responses = [];
const client = createNativeClient({ host: 'test', connectNative: () => port, writeMethods: new Set(['write_file']), onEvent: (message) => events.push(message), onResponse: (message) => responses.push(message) });
assert.deepEqual(await client.call('version'), { method: 'version' });
assert.equal(responses.length, 1);
assert.equal(client.connected, true);
assert.equal(client.inFlight, 0);
port.emit({ result: { event: 'fs_changed' } });
assert.equal(events.length, 1);
const pending = client.call('write_file');
client.disconnect();
await assert.rejects(pending, /Native Host 已断开/);
assert.equal(client.connected, false);
assert.equal(client.writesInFlight, 0);
console.log('native client seam test passed');
