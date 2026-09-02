import { createNativeClient } from './native-client.js';

export function createFilesHostConnection({
  host, writeMethods, t, onMessage, onDisconnect, onVisible,
  onMoveBatchResult, onTrashBatchErrors, hasDirtyEditor, onConnectionChange,
}) {
  let pageClosing = false;
  let idleDisconnectTimer;

  const client = createNativeClient({
    host,
    writeMethods,
    onMessage,
    onDisconnect: (error, wasIntentional) => {
      onDisconnect(error, wasIntentional);
      if (!wasIntentional) scheduleReconnect();
    },
  });

  function disconnect() {
    clearTimeout(idleDisconnectTimer);
    idleDisconnectTimer = undefined;
    client.disconnect();
    onConnectionChange();
  }

  function scheduleReconnect() {
    if (pageClosing || client.connected) return;
    setTimeout(() => {
      if (client.connected || pageClosing) return;
      client.connect();
      onConnectionChange();
    }, 1_000);
  }

  function scheduleIdleDisconnect() {
    clearTimeout(idleDisconnectTimer);
    idleDisconnectTimer = undefined;
    if (!document.hidden || client.inFlight !== 0 || client.writesInFlight !== 0 || !client.connected) return;
    idleDisconnectTimer = setTimeout(() => {
      if (document.hidden && client.inFlight === 0 && client.writesInFlight === 0) disconnect();
    }, 60_000);
  }

  async function call(method, params = {}, requestId) {
    let result;
    try {
      result = await client.call(method, params, requestId);
    } catch (error) {
      if (!client.connected && error.message === 'Native Host 已断开') throw new Error(t('hostDisconnected', 'Native Host 已断开'));
      if (error.message === '请求超时') throw new Error(t('requestTimeout', '请求超时'));
      throw error.message === '操作失败' ? new Error(t('operationFailed', '操作失败')) : error;
    }
    if (method === 'move_batch' && (result?.errors?.length || result?.skipped?.length)) onMoveBatchResult(params, result);
    if (method === 'trash_batch' && result?.errors?.length) onTrashBatchErrors(result.errors);
    if (pageClosing && client.inFlight === 0) disconnect();
    else scheduleIdleDisconnect();
    return result;
  }

  document.addEventListener('visibilitychange', () => {
    if (document.hidden) scheduleIdleDisconnect();
    else {
      pageClosing = false;
      clearTimeout(idleDisconnectTimer);
      if (!client.connected) onVisible();
    }
  });
  window.addEventListener('pagehide', () => {
    pageClosing = true;
    if (client.writesInFlight === 0) disconnect();
    else scheduleIdleDisconnect();
  });
  window.addEventListener('beforeunload', (event) => {
    if (!hasDirtyEditor()) return;
    event.preventDefault();
    event.returnValue = '';
  });

  return { client, call, disconnect, scheduleReconnect };
}
