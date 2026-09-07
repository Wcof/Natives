export function createNativeClient({ host, connectNative = (name) => globalThis.__NATIVES_DEV_NATIVE_CONNECT__?.(name) || globalThis.chrome.runtime.connectNative(name), writeMethods = new Set(), timeoutMs = 15_000, onEvent, onDisconnect, onResponse, handshake }) {
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
      const responder = message?.id && pending.get(message.id);
      if (responder) { pending.delete(message.id); onResponse?.(message); responder(message); return; }
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
        const settle = (message, transportError) => {
          if (transportError) reject(transportError);
          else if (message?.ok) resolve(message.result);
          else reject(Object.assign(new Error(message?.error || '握手失败'), {
            code: message?.errorCode || 'handshake_failed',
          }));
        };
        pending.set(id, settle);
        setTimeout(() => {
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
          const error = new Error(message?.error || '操作失败');
          error.code = message?.errorCode || 'internal_error';
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
