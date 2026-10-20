import assert from 'node:assert/strict';

globalThis.document = Object.assign(new EventTarget(), { visibilityState: 'visible' });
globalThis.window = new EventTarget();
globalThis.chrome = { runtime: { connectNative() { throw Error('must reuse the owning Core port'); } } };
const { subscribeCoreEvents } = await import('./events.js');
const received = [];
const stop = subscribeCoreEvents(event => received.push(event));
await new Promise(resolve => setImmediate(resolve));
const send = event => { for (const handler of globalThis.__nativesNativeEventBus) handler(event); };
send({ type: 'connected', epoch: 'one', sequence: 0 });
const change = { type: 'workspace_changed', domain: 'workspace', epoch: 'one', sequence: 1 };
send(change);
send(change);
assert.equal(received.filter(event => event.type === 'workspace_changed').length, 1,
  'the same Core invalidation arriving through two owned ports is delivered once');
assert.equal(received.filter(event => event.reason === 'sequence').length, 0,
  'duplicate relay delivery is not a sequence gap');
send({ ...change, sequence: 3 });
assert.equal(received.filter(event => event.reason === 'sequence').length, 1);
stop();
assert.equal(globalThis.__nativesNativeEventBus.size, 0);
console.log('Native relay duplicate suppression, genuine gap and disposal passed');
