// Web leases are owned by a document or a module client; one shared heartbeat.
import { initWebSession, getCsrfToken } from './web-session.js';

const leases = new Set();
let heartbeat, hiddenTimer, eventsBound = false;
const headers = () => ({ 'Content-Type': 'application/json', 'X-CSRF-Token': getCsrfToken() || '' });
function announce(state) {
  if (typeof CustomEvent !== 'undefined') window.dispatchEvent(new CustomEvent('natives-web-surface', { detail: { state } }));
}
function releaseAll() { for (const lease of [...leases]) lease.release(); }
function bindLifecycle() {
  if (eventsBound) return;
  eventsBound = true;
  window.addEventListener('pagehide', releaseAll);
  window.addEventListener('natives-web-session-restored', releaseAll);
  window.addEventListener('pageshow', event => { if (event.persisted) void ensureWebSurface().catch(() => announce('error')); });
  document.addEventListener('visibilitychange', () => {
    clearTimeout(hiddenTimer);
    if (document.visibilityState === 'hidden') hiddenTimer = setTimeout(releaseAll, 60_000);
    else for (const lease of [...leases]) void lease.renew().catch(() => announce('error'));
  });
}
function track(lease) {
  leases.add(lease);
  bindLifecycle();
  if (!heartbeat) heartbeat = setInterval(() => {
    if (document.visibilityState !== 'hidden') for (const owned of [...leases]) void owned.renew().catch(() => announce('error'));
  }, 20_000);
}
function untrack(lease) {
  leases.delete(lease);
  if (!leases.size) { clearInterval(heartbeat); heartbeat = undefined; clearTimeout(hiddenTimer); }
}

const webLeaseFactory = typeof __NATIVES_EXTENSION_BUILD__ !== 'undefined' && __NATIVES_EXTENSION_BUILD__ ? undefined : function (appId, onRelease = () => {}) {
  let id, opening, renewing, generation = 0, disposed = false;
  const requests = new Set();
  async function request(path, options) {
    const controller = new AbortController();
    requests.add(controller);
    const timer = setTimeout(() => controller.abort(), 10_000);
    try {
      const response = await fetch(path, { ...options, signal: controller.signal });
      const result = response.ok ? await response.json() : null;
      return { ok: response.ok, result };
    } finally {
      clearTimeout(timer);
      requests.delete(controller);
    }
  }
  async function ensure() {
    if (disposed) throw Error('WEB_SURFACE_RELEASED');
    if (id) return id;
    if (opening) return opening;
    const owned = generation;
    track(lease);
    const pending = (async () => {
      if (!(await initWebSession())?.authenticated) throw Error('WEB_SESSION_REQUIRED');
      if (disposed || owned !== generation) throw Error('WEB_SURFACE_RELEASED');
      const response = await request('/web/surfaces', {
        method: 'POST', headers: headers(), body: JSON.stringify(appId ? { appId } : {}),
      });
      if (!response.ok) throw Error('WEB_SURFACE_UNAVAILABLE');
      const result = response.result;
      if (typeof result.surfaceId !== 'string') throw Error('WEB_SURFACE_INVALID');
      if (owned !== generation || disposed) {
        void remove(result.surfaceId);
        throw Error('WEB_SURFACE_RELEASED');
      }
      id = result.surfaceId;
      announce('ready');
      return id;
    })();
    opening = pending;
    try { return await pending; }
    catch (error) { if (owned === generation && !id) untrack(lease); throw error; }
    finally { if (opening === pending) opening = undefined; }
  }
  function remove(owned) {
    return fetch(`/web/surfaces/${encodeURIComponent(owned)}`, {
      method: 'DELETE', headers: headers(), keepalive: true,
    }).catch(() => {});
  }
  function release() {
    const active = id || opening;
    generation++;
    opening = undefined;
    for (const controller of requests) controller.abort();
    requests.clear();
    const owned = id;
    id = undefined;
    untrack(lease);
    if (owned) void remove(owned);
    if (active) onRelease();
  }
  function renew() {
    if (!id || document.visibilityState === 'hidden') return Promise.resolve();
    if (renewing) return renewing;
    const owned = id;
    renewing = (async () => {
      const response = await request(`/web/surfaces/${encodeURIComponent(owned)}`, {
        method: 'POST', headers: headers(), body: '{}',
      });
      if (id === owned && !response.ok) { release(); throw Error('WEB_SURFACE_EXPIRED'); }
    })().finally(() => { renewing = undefined; });
    return renewing;
  }
  const lease = { ensure, renew, release, dispose() { disposed = true; release(); } };
  return lease;
};
export function createWebSurfaceLease(appId, onRelease) {
  if (!webLeaseFactory) throw Error('WEB_ONLY');
  return webLeaseFactory(appId, onRelease);
}
const documentLease = webLeaseFactory?.();
export const ensureWebSurface = () => documentLease ? documentLease.ensure() : Promise.reject(Error('WEB_ONLY'));
