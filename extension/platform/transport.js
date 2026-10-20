// 双入口统一通信传输层（Native Messaging / HTTP RPC + SSE，ADR-0042）

import { isExtension, getBaseEndpoint } from './env.js';
import { createNativeClient } from '../native-client.js';
import { getCsrfToken, initWebSession } from './web-session.js';
import { ensureWebSurface } from './surface.js';
import { subscribeCoreEvents } from './events.js';

// The signed Core serves this source for Web; Chrome only ships the Native adapter.
const WebRpcClient = typeof __NATIVES_EXTENSION_BUILD__ !== 'undefined' && __NATIVES_EXTENSION_BUILD__ ? undefined : class {
  constructor({ timeoutMs = 15_000, writeMethods = new Set(), onEvent, onDisconnect } = {}) {
    this.timeoutMs = timeoutMs;
    this.writeSet = writeMethods instanceof Set ? writeMethods : new Set(writeMethods || []);
    this._inFlight = 0;
    this._writesInFlight = 0;
    this._connected = false;
    this._eventHandlers = new Set();
    if (onEvent) this._eventHandlers.add(onEvent);
    this._onDisconnect = onDisconnect;
    this._requests = new Set();
    this._generation = 0;
    this._restoreSession = () => {
      this._generation++;
      for (const controller of this._requests) controller.abort();
    };
  }

  get connected() {
    return this._connected;
  }

  get inFlight() {
    return this._inFlight;
  }

  get writesInFlight() {
    return this._writesInFlight;
  }

  connect() {
    if (this._connected) return this;
    this._connected = true;
    window.addEventListener('natives-web-session-restored', this._restoreSession);
    this._unsubscribe = subscribeCoreEvents(payload => {
      for (const handler of this._eventHandlers) handler(payload);
    });
    return this;
  }

  onEvent(handler) {
    this._eventHandlers.add(handler);
    return () => this._eventHandlers.delete(handler);
  }

  disconnect() {
    const connected = this._connected;
    this._generation++;
    this._connected = false;
    window.removeEventListener('natives-web-session-restored', this._restoreSession);
    this._unsubscribe?.();
    this._unsubscribe = undefined;
    for (const controller of this._requests) controller.abort();
    this._requests.clear();
    if (connected) this._onDisconnect?.(Object.assign(new Error('Native Host 已断开'), { code: 'host_disconnected' }), true);
  }

  async call(method, params = {}, requestId) {
    const generation = this._generation;
    await initWebSession();
    const surfaceId = await ensureWebSurface();
    if (generation !== this._generation) throw Object.assign(new Error('Native Host 已断开'), { code: 'host_disconnected' });
    this.connect();
    if (this._inFlight >= 16) throw Object.assign(new Error('请求过多'), { code: 'request_limit' });
    const id = requestId || (typeof crypto !== 'undefined' && crypto.randomUUID ? crypto.randomUUID() : `req_${Date.now()}_${Math.random().toString(36).slice(2)}`);
    const isWrite = this.writeSet.has(method);
    this._inFlight++;
    if (isWrite) this._writesInFlight++;

    const base = getBaseEndpoint();
    const csrfToken = getCsrfToken();
    const headers = {
      'Content-Type': 'application/json',
    };
    if (csrfToken) {
      headers['X-CSRF-Token'] = csrfToken;
    }
    headers['X-Natives-Surface-Id'] = surfaceId;

    const controller = typeof AbortController !== 'undefined' ? new AbortController() : null;
    if (controller) this._requests.add(controller);
    const timer = setTimeout(() => {
      if (controller) controller.abort();
    }, this.timeoutMs);

    try {
      const res = await fetch(`${base}/web/rpc`, {
        method: 'POST',
        headers,
        body: JSON.stringify({ id, method, params }),
        signal: controller ? controller.signal : undefined,
      });

      if (!res.ok) {
        const text=await res.text().catch(()=> '');
        let body;try{body=JSON.parse(text);}catch {body={error:text};}
        const error=body.error;
        throw Object.assign(new Error(typeof error==='object'?error.message:error||`HTTP ${res.status}`),{code:body.errorCode||`http_${res.status}`});
      }

      const body = await res.json();
      if (generation !== this._generation) throw Object.assign(new Error('Native Host 已断开'), { code: 'host_disconnected' });
      if (!body.ok) {
        const errObj = body.error;
        const msg = typeof errObj === 'object' ? errObj.message : errObj || '操作失败';
        const code = typeof errObj === 'object' ? errObj.code : 'rpc_error';
        throw Object.assign(new Error(msg), { code });
      }

      return body.result;
    } catch (err) {
      if (generation !== this._generation) throw Object.assign(new Error('Native Host 已断开'), { code: 'host_disconnected' });
      if (err.name === 'AbortError') {
        throw Object.assign(new Error('请求超时'), { code: 'request_timeout' });
      }
      throw err;
    } finally {
      clearTimeout(timer);
      if (controller) this._requests.delete(controller);
      this._inFlight--;
      if (isWrite) this._writesInFlight--;
    }
  }
};

export function createPlatformClient(options = {}) {
  if (typeof __NATIVES_EXTENSION_BUILD__ !== 'undefined' ? __NATIVES_EXTENSION_BUILD__ : isExtension) {
    return createNativeClient(options);
  }
  return new WebRpcClient(options);
}
