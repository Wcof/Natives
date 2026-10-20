import assert from 'node:assert/strict';
import { extensionFileBytes } from '../../scripts/extension-package.mjs';

const chromeSurface = extensionFileBytes('platform/surface.js').toString();
assert.ok(!chromeSurface.includes('/web/surfaces'), 'Chrome payload excludes the Web-only lease transport');
assert.ok(!chromeSurface.includes('setInterval'), 'Chrome payload has no unused Web heartbeat');
const chromeEvents = extensionFileBytes('platform/events.js').toString();
assert.ok(!chromeEvents.includes('EventSource'), 'Chrome uses its existing Native event relay');
assert.ok(!chromeEvents.includes('/web/events'), 'Chrome payload excludes the Web SSE transport');
const chromeStorage = extensionFileBytes('platform/storage.js').toString();
assert.ok(!chromeStorage.includes('localStorage.setItem'), 'Chrome uses its existing browser storage for local projections');
const chromeSession = extensionFileBytes('platform/web-session.js').toString();
assert.ok(!chromeSession.includes('web-entry.js')&&!chromeSession.includes('./session.js'), 'Chrome never imports Web authentication');

globalThis.document = Object.assign(new EventTarget(), { visibilityState: 'visible' });
globalThis.window = new EventTarget();
globalThis.location = { origin: 'http://127.0.0.1:8771', hash: '' };
globalThis.sessionStorage = { setItem() {}, removeItem() {} };
let authenticated = false, holdRpc = false, holdRenewal = false, holdOpening = false, renewals = 0;
let nextSurface = 0, sessionReads = 0;
const ownedSurfaces = new Set();
const streams = [];
globalThis.EventSource = class {
  constructor() { streams.push(this); }
  close() { this.closed = true; }
};
globalThis.fetch = async (url, options = {}) => {
  const path = new URL(url, location.origin).pathname;
  const result = value => ({ ok: true, json: async () => value });
  if (path === '/web/session') {sessionReads++;return result({ authenticated, csrfToken: 'isolated-csrf' });}
  if (path === '/web/session/exchange') { authenticated = true; ownedSurfaces.clear(); return result({ csrfToken: 'isolated-csrf' }); }
  if (path === '/web/surfaces' && !holdOpening) {
    const surfaceId = `surf_isolated_${++nextSurface}`;
    ownedSurfaces.add(surfaceId);
    return result({ surfaceId });
  }
  if (path.startsWith('/web/surfaces/')) {
    if (options.method === 'POST') renewals++;
    if (!holdRenewal || options.method === 'DELETE') return result({ ok: true });
  } else if (path === '/web/rpc' && !holdRpc) {
    if (!ownedSurfaces.has(options.headers['X-Natives-Surface-Id'])) {
      return { ok: false, status: 403, text: async () => '{"error":"ERR_SURFACE_NOT_OWNED"}' };
    }
    return result({ ok: true, result: { method: JSON.parse(options.body).method } });
  }
  return new Promise((resolve, reject) => {
    options.signal.addEventListener('abort', () => reject(Object.assign(Error('aborted'), { name: 'AbortError' })), { once: true });
  });
};
const { initWebSession, exchangePairingCode } = await import('./session.js');
const { createPlatformClient } = await import('./transport.js');
const { createWebSurfaceLease } = await import('./surface.js');
const flush = () => new Promise(resolve => setImmediate(resolve));

assert.equal((await initWebSession()).authenticated, false);
await exchangePairingCode('ISOLATED');
assert.equal((await initWebSession()).authenticated, true, 'successful pairing replaces a cached unauthenticated result');
const received = [];
const first = createPlatformClient({ onEvent: event => received.push(event) });
const second = createPlatformClient();
assert.deepEqual(await first.call('version'), { method: 'version' });
await second.call('roots');
await flush();
assert.equal(streams.length, 1, 'two clients share one document SSE');
streams[0].onmessage({ data: '{"type":"connected","epoch":"one"}' });
streams[0].onmessage({ data: '{"type":"connected","epoch":"two"}' });
assert.ok(received.some(event => event.type === 'resync'));
streams[0].onmessage({data:JSON.stringify({type:'connected',epoch:'two',sequence:3})});
streams[0].onmessage({data:JSON.stringify({type:'preferences_changed',domain:'preferences',epoch:'two',sequence:5})});
assert.ok(received.some(event=>event.type==='resync' && event.reason==='sequence'),'a sequence gap requires a fresh snapshot');
holdRpc = true;
const pending = first.call('read_file');
await flush();
first.disconnect();
await assert.rejects(pending, { code: 'host_disconnected' });
assert.equal(first.inFlight, 0);
assert.ok(!streams[0].closed, 'another client still owns the SSE');
second.disconnect();
assert.equal(streams[0].closed, true);

const lease = createWebSurfaceLease('fund');
await lease.ensure();
holdRenewal = true;
const renewal = lease.renew();
const rejected = assert.rejects(renewal, { name: 'AbortError' });
assert.equal(lease.renew(), renewal, 'renewals are merged while pending');
assert.equal(renewals, 1);
lease.dispose();
await rejected;
await assert.rejects(lease.ensure(), /WEB_SURFACE_RELEASED/);
holdOpening = true;
let released = 0;
const lateLease = createWebSurfaceLease('fund', () => released++);
const late = lateLease.ensure();
const lateRejected = assert.rejects(Promise.race([
  late,
  new Promise((resolve, reject) => setTimeout(() => reject(Object.assign(Error('late lease still pending'), { name: 'UncancelledLease' })), 200)),
]), { name: 'AbortError' });
await flush();
window.dispatchEvent(new Event('pagehide'));
holdOpening = false;
const resumed = lateLease.ensure();
await lateRejected;
assert.equal(released, 1, 'pagehide releases a lease that has not received an id yet');
await resumed;
lateLease.dispose();
holdRpc = false;
holdRenewal = false;
const restored = createPlatformClient();
await restored.call('version');
await exchangePairingCode('NEW_SESSION');
assert.deepEqual(await restored.call('version'), { method: 'version' },
  'a new authenticated session must replace the document Surface without a page reload');
const readsBefore = sessionReads;
ownedSurfaces.clear();
window.dispatchEvent(Object.assign(new Event('storage'),{key:'natives-web-session-revision'}));
await flush();await flush();
assert.ok(sessionReads>readsBefore, 'a peer pairing must refresh the shared Cookie session');
assert.deepEqual(await restored.call('version'), {method:'version'});
restored.disconnect();
window.dispatchEvent(new Event('pagehide'));
console.log('Web pairing, shared SSE, disconnect cancellation and lease renewal regressions passed');
