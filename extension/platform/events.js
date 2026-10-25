// One owned event subscription per document; gaps always invalidate snapshots.
import { initWebSession, invalidateWebSession } from './web-session.js';
import { isExtension, getBaseEndpoint } from './env.js';
import { ensureWebSurface } from './surface.js';

const subscribers = new Set();
let source, opening, retry, attempt = 0, generation = 0, bound = false;
let epoch, sequence, connected = false;
let pageActive = true;
const web = !(typeof __NATIVES_EXTENSION_BUILD__ !== 'undefined' ? __NATIVES_EXTENSION_BUILD__ : isExtension);
let documentOwned = false;
const hasOwners = () => subscribers.size || (web && documentOwned);
const backoff = [1000, 2000, 4000, 8000, 15000];
function deliver(payload) {
  if (web && document.visibilityState === 'hidden') return;
  for (const {handler, domains} of subscribers) {
    if (domains && !['resync','connected','shutdown'].includes(payload.type) && !domains.includes(payload.domain)) continue;
    try { handler(payload); } catch { /* A consumer cannot break other subscriptions. */ }
  }
}
function receive(payload) {
  if (web && document.visibilityState === 'hidden') return;
  if (!payload || typeof payload !== 'object') return;
  const next = payload.sequence;
  // A document can own several relays for the same Core event stream.
  if (epoch === payload.epoch && Number.isInteger(next) && Number.isInteger(sequence) && next <= sequence) return;
  if (payload.type === 'connected') {
    deliver({type:'resync',reason:connected ? (epoch !== payload.epoch ? 'epoch' : 'reconnect') : 'connected',epoch:payload.epoch});
    connected = true; attempt = 0;
  } else if (epoch !== undefined && (epoch !== payload.epoch || (Number.isInteger(next) && next !== sequence + 1))) {
    deliver({type:'resync',reason:epoch !== payload.epoch ? 'epoch' : 'sequence',epoch:payload.epoch});
  }
  if (payload.epoch !== undefined) epoch = payload.epoch;
  if (Number.isInteger(next)) sequence = next;
  deliver(payload);
}
function close() {
  generation++; clearTimeout(retry); retry = undefined;
  const owned = source; source = undefined;
  if ((typeof __NATIVES_EXTENSION_BUILD__ !== 'undefined' ? __NATIVES_EXTENSION_BUILD__ : isExtension)) owned?.disconnect(); else owned?.close();
}
function failed(owned) {
  if (source !== owned) return;
  close(); invalidateWebSession();
  deliver({type:'resync',reason:'unavailable'});
  if (hasOwners() && pageActive && document.visibilityState !== 'hidden') {
    retry = setTimeout(() => { retry = undefined; void open(); }, backoff[Math.min(attempt++, backoff.length-1)]);
  }
}
async function open() {
  if (source || opening || retry || !pageActive || !hasOwners() || (!web && document.visibilityState === 'hidden')) return;
  const ownedGeneration = generation;
  const dormant = web && document.visibilityState === 'hidden';
  opening = (async () => {
    if (!(typeof __NATIVES_EXTENSION_BUILD__ !== 'undefined' ? __NATIVES_EXTENSION_BUILD__ : isExtension) && !(await initWebSession())?.authenticated) {
      if(typeof CustomEvent!=='undefined')window.dispatchEvent(new CustomEvent('natives-web-session-required'));
      throw Error('WEB_SESSION_REQUIRED');
    }
    if (ownedGeneration !== generation || !pageActive || !hasOwners() || dormant !== (web && document.visibilityState === 'hidden')) return;
    if (web) documentOwned = true;
    if ((typeof __NATIVES_EXTENSION_BUILD__ !== 'undefined' ? __NATIVES_EXTENSION_BUILD__ : isExtension)) {
      // The page's Core Port already carries its own Surface-scoped relay.
      // Reuse that stream instead of opening a second Native Surface, so
      // progress can be delivered only to the owning window.
      const bus = globalThis.__nativesNativeEventBus ||= new Set();
      const owned = { disconnect() { bus.delete(handler); } };
      const handler = payload => {
        if (source !== owned) return;
        if (payload.type === 'resync' && payload.reason === 'unavailable') failed(owned);
        else receive(payload);
      };
      bus.add(handler);
      source = owned;
    } else {
      let surface = '';
      if (!dormant) surface = await ensureWebSurface();
      if (ownedGeneration !== generation || !pageActive || !hasOwners()) return;
      const owned = new EventSource(`${getBaseEndpoint()}/web/events${surface ? `?surface=${encodeURIComponent(surface)}` : ''}`);
      source = owned;
      owned.onmessage = event => {
        if (source !== owned) return;
        try { receive(JSON.parse(event.data)); } catch { failed(owned); }
      };
      owned.onerror = () => failed(owned);
    }
  })();
  try { await opening; }
  catch {
    if (ownedGeneration === generation && pageActive && hasOwners() && document.visibilityState !== 'hidden') {
      deliver({type:'resync',reason:'unavailable'});
      retry = setTimeout(() => { retry = undefined; void open(); }, backoff[Math.min(attempt++,backoff.length-1)]);
    }
  } finally {
    opening = undefined;
    if (ownedGeneration !== generation && pageActive && hasOwners()) void open();
  }
}
export function subscribeCoreEvents(handler, domains) {
  if (!(typeof __NATIVES_EXTENSION_BUILD__ !== 'undefined' ? __NATIVES_EXTENSION_BUILD__ : isExtension) && typeof EventSource === 'undefined') return () => {};
  const subscription = {handler, domains};
  subscribers.add(subscription);
  if (!bound) {
    bound = true;
    document.addEventListener('visibilitychange', () => {
      close();
      if (pageActive && (web || document.visibilityState !== 'hidden')) void open();
    });
    window.addEventListener('pagehide', () => { pageActive=false; close(); });
    window.addEventListener('pageshow', () => { pageActive=true; void open(); });
    window.addEventListener('natives-web-session-restored', () => { close(); void open(); });
  }
  void open();
  return () => { subscribers.delete(subscription); if (!hasOwners()) close(); };
}
