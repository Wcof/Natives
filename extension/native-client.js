import { createWebSurfaceLease, ensureWebSurface } from './platform/surface.js';
import { subscribeCoreEvents } from './platform/events.js';

function createWebPort(name) {
  const messageListeners = new Set();
  const disconnectListeners = new Set();
  const requests = new Set();
  let connected = true;
  let appLease;
  let modelLease;
  window.addEventListener('natives-web-session-restored', disconnect);
  const unsubscribe = subscribeCoreEvents(event => {
    if (connected) for (const fn of messageListeners) fn(event);
  });

  function disconnect() {
    if (!connected) return;
    connected = false;
    window.removeEventListener('natives-web-session-restored', disconnect);
    unsubscribe();
    for (const controller of requests) controller.abort();
    requests.clear();
    appLease?.dispose();
    modelLease?.dispose();
    for (const fn of disconnectListeners) fn();
  }

  return {
    onMessage: {
      addListener: (fn) => messageListeners.add(fn),
      removeListener: (fn) => messageListeners.delete(fn),
    },
    onDisconnect: {
      addListener: (fn) => disconnectListeners.add(fn),
      removeListener: (fn) => disconnectListeners.delete(fn),
    },
    disconnect,
    async postMessage(msg) {
      if (!connected) return;
      const controller = new AbortController();
      requests.add(controller);
      try {
        if (msg.method === 'app:handshake' && !appLease) appLease = createWebSurfaceLease(msg.params.appId, disconnect);
        if (msg.method.startsWith('model_') && !modelLease) modelLease=createWebSurfaceLease(undefined,disconnect);
        if (msg.method.startsWith('app:') && !appLease) throw Error('APP_SURFACE_NOT_BOUND');
        const lease=msg.method.startsWith('app:')?appLease:msg.method.startsWith('model_')?modelLease:null;
        const surfaceId = await (lease ? lease.ensure() : ensureWebSurface());
        if (!connected) return;
        const base = typeof location !== 'undefined' && location.origin ? location.origin : 'http://127.0.0.1:8770';
        const headers = { 'Content-Type': 'application/json' };
        if (typeof sessionStorage !== 'undefined') {
          const csrf = sessionStorage.getItem('natives_csrf');
          if (csrf) headers['X-CSRF-Token'] = csrf;
        }
        headers['X-Natives-Surface-Id'] = surfaceId;

        const res = await fetch(`${base}/web/rpc`, {
          method: 'POST',
          headers,
          body: JSON.stringify(msg),
          signal: controller.signal,
        });
        if (!res.ok) {
          const text=await res.text().catch(()=> '');
          let error;
          try {error=JSON.parse(text);}catch {error={error:text};}
          if (!connected) return;
          for(const fn of messageListeners) fn({id:msg.id,ok:false,error:error.error||`HTTP ${res.status}`,errorCode:error.errorCode||`http_${res.status}`});
          return;
        }

        const data = await res.json();
        if (!connected) return;
        for (const fn of messageListeners) {
          fn(data);
        }
      } catch (err) {
        if (!connected) return;
        for (const fn of messageListeners) {
          fn({ id: msg.id, ok: false, error: err.message || '网络连接失败' });
        }
      } finally {
        requests.delete(controller);
      }
    },
  };
}

function defaultConnectNative(name) {
  if (globalThis.__NATIVES_DEV_NATIVE_CONNECT__) {
    return globalThis.__NATIVES_DEV_NATIVE_CONNECT__(name);
  }
  if (globalThis.chrome?.runtime?.connectNative) {
    return globalThis.chrome.runtime.connectNative(name);
  }
  return createWebPort(name);
}

export function createNativeClient({ host, connectNative = defaultConnectNative, writeMethods = new Set(), timeoutMs = 15_000, onEvent, onDisconnect, onResponse, handshake }) {
  const writeSet = writeMethods instanceof Set ? writeMethods : new Set(writeMethods || []);
  let port;
  let intentional = false;
  let inFlight = 0;
  let writesInFlight = 0;
  let handshakePromise = null;
  const pending = new Map();

  function disconnectError() {
    return globalThis.chrome?.runtime?.lastError?.message || 'Native Host 已断开';
  }
  function handleDisconnect(next) {
    if (port !== next) return;
    const error = new Error(disconnectError());
    error.code = 'host_disconnected';
    const wasIntentional = intentional;
    intentional = false;
    port = undefined;
    for (const respond of pending.values()) respond(undefined, error);
    pending.clear();
    inFlight = 0;
    writesInFlight = 0;
    onDisconnect?.(error, wasIntentional);
  }
  function connect() {
    if (port) return port;
    intentional = false;
    const next = connectNative(host);
    port = next;
    next.onMessage.addListener((message) => {
      if(message?.type==='resync' && message.reason==='unavailable') {
        try { next.disconnect(); } finally { handleDisconnect(next); }
        return;
      }
      const responder = message?.id && pending.get(message.id);
      if (responder) { pending.delete(message.id); onResponse?.(message); responder(message); return; }
      const bus = globalThis.__nativesNativeEventBus;
      if (bus) for (const handler of [...bus]) { try { handler(message); } catch { /* one subscriber cannot break the port */ } }
      onEvent?.(message);
    });
    next.onDisconnect.addListener(() => handleDisconnect(next));
    if (handshake) {
      // ADR-0025 D16: the host must learn the REAL caller origin before
      // host-manifest registration can succeed. The handshake rides the
      // same request/response framing as every other call. A failed
      // handshake means the host treats this connection as origin-less
      // (registration is explicitly skipped) — the caller surfaces the
      // error instead of fabricating an origin. The pending map also
      // settles it on disconnect, so no caller can wait forever.
      handshakePromise = new Promise((resolve, reject) => {
        const id = crypto.randomUUID();
        let timer;
        const settle = (message, transportError) => {
          clearTimeout(timer);
          if (transportError) reject(transportError);
          else if (message?.ok) resolve(message.result);
          else reject(Object.assign(new Error(message?.error || '握手失败'), {
            code: message?.errorCode || 'handshake_failed',
          }));
        };
        pending.set(id, settle);
        timer = setTimeout(() => {
          if (pending.delete(id)) settle(undefined, Object.assign(new Error('握手超时'), { code: 'request_timeout' }));
        }, timeoutMs);
        next.postMessage({ id, method: handshake.method, params: handshake.params });
      });
    }
    return next;
  }
  function disconnect() {
    const next = port;
    if (!next) return;
    intentional = true;
    try { next.disconnect(); } finally { handleDisconnect(next); }
  }
  function call(method, params = {}, requestId) {
    let next;
    try { next = connect(); } catch (error) {
      // connect() can throw synchronously (connectNative rejects) —
      // surface it as a rejected call, NOT an async throw that skips
      // the bookkeeping below.
      const error_ = new Promise((resolve, reject) => { reject(error); });
      return error_;
    }
    // The pending entry MUST be registered synchronously (before any
    // microtask) so a same-tick disconnect() settles this call.
    return new Promise((resolve, reject) => {
      const id = requestId || crypto.randomUUID();
      if(pending.has(id) || pending.size>=16) {
        reject(Object.assign(new Error('请求过多或请求标识重复'),{code:'request_limit'}));return;
      }
      const isWrite = writeSet.has(method);
      inFlight++;
      if (isWrite) writesInFlight++;
      let settled = false;
      const settle = (message, transportError) => {
        if (settled) return;
        settled = true;
        clearTimeout(timeout);
        inFlight--;
        if (isWrite) writesInFlight--;
        if (transportError) reject(transportError);
        else if (message?.ok) resolve(message.result);
        else {
          const body = message?.error;
          const error = new Error(typeof body === 'object' ? body.message : body || '操作失败');
          error.code = (typeof body === 'object' ? body.code : message?.errorCode || /\bAPP_[A-Z_]+\b/.exec(String(body))?.[0]) || 'internal_error';
          error.retryable = Boolean(typeof body === 'object' && body.retryable);
          reject(error);
        }
      };
      const timeout = setTimeout(() => {
        if (!pending.delete(id)) return;
        const error = new Error('请求超时'); error.code = 'request_timeout'; settle(undefined, error);
      }, timeoutMs);
      pending.set(id, settle);
      const send = () => {
        if (settled) return;
        try { next.postMessage({ id, method, params }); } catch (error) {
          pending.delete(id);
          settle(undefined, error);
        }
      };
      if (handshakePromise) {
        // ADR-0025 D16: the first call on the port is the handshake;
        // it rides the same pending map, so a disconnect settles it
        // too. Calls waiting on a failed handshake are rejected with
        // the handshake error via settle() (idempotent).
        handshakePromise.then(send).catch((handshakeError) => {
          pending.delete(id);
          settle(undefined, handshakeError);
        });
      } else {
        send();
      }
    });
  }
  return { call, connect, disconnect, get connected() { return Boolean(port); }, get inFlight() { return inFlight; }, get writesInFlight() { return writesInFlight; } };
}
