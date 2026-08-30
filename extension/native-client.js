export function createNativeClient({ host, connectNative = (name) => globalThis.__NATIVES_DEV_NATIVE_CONNECT__?.(name) || globalThis.chrome.runtime.connectNative(name), writeMethods = new Set(), timeoutMs = 15_000, onEvent, onDisconnect, onResponse }) {
  const writeSet = writeMethods instanceof Set ? writeMethods : new Set(writeMethods || []);
  let port;
  let intentional = false;
  let inFlight = 0;
  let writesInFlight = 0;
  const pending = new Map();

  function disconnectError() {
    return globalThis.chrome?.runtime?.lastError?.message || 'Native Host 已断开';
  }
  function handleDisconnect(next) {
    if (port !== next) return;
    const error = new Error(disconnectError());
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
    return next;
  }
  function disconnect() {
    const next = port;
    if (!next) return;
    intentional = true;
    try { next.disconnect(); } finally { handleDisconnect(next); }
  }
  function call(method, params = {}, requestId) {
    return new Promise((resolve, reject) => {
      let next;
      try { next = connect(); } catch (error) { reject(error); return; }
      const id = requestId || crypto.randomUUID();
      const isWrite = writeSet.has(method);
      inFlight++;
      if (isWrite) writesInFlight++;
      const settle = (message, transportError) => {
        clearTimeout(timeout);
        inFlight--;
        if (isWrite) writesInFlight--;
        if (transportError) reject(transportError);
        else if (message?.ok) resolve(message.result);
        else reject(new Error(message?.error || '操作失败'));
      };
      const timeout = setTimeout(() => {
        if (!pending.delete(id)) return;
        settle(undefined, new Error('请求超时'));
      }, timeoutMs);
      pending.set(id, settle);
      try { next.postMessage({ id, method, params }); } catch (error) {
        clearTimeout(timeout);
        pending.delete(id);
        inFlight--;
        if (isWrite) writesInFlight--;
        reject(error);
      }
    });
  }
  return { call, connect, disconnect, get connected() { return Boolean(port); }, get inFlight() { return inFlight; }, get writesInFlight() { return writesInFlight; } };
}
