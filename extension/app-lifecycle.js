// Ephemeral page coordination only. Host locks and the registry remain authoritative.
export function appLifecycle(onEvent = () => {}) {
  const channel = typeof window !== 'undefined' && typeof BroadcastChannel !== 'undefined'
    ? new BroadcastChannel('natives.apps.lifecycle.v1') : null;
  let closed = false;
  if (channel) channel.onmessage = (event) => {
    const { type, appId } = event.data || {};
    if (['maintenance', 'changed'].includes(type) && typeof appId === 'string') onEvent({ type, appId });
  };
  return {
    notify: (type, appId) => { if (!closed) channel?.postMessage({ type, appId }); },
    close: () => { closed = true; channel?.close(); },
  };
}
