// Ephemeral page coordination only. Host locks and the registry remain authoritative.
export function appLifecycle(onEvent = () => {}) {
  const channel = typeof window !== 'undefined' && typeof BroadcastChannel !== 'undefined'
    ? new BroadcastChannel('natives.apps.lifecycle.v1') : null;
  let closed = false;
  if (channel) channel.onmessage = (event) => {
    const { type, appId } = event.data || {};
    if (['maintenance', 'changed', 'stop'].includes(type) && typeof appId === 'string') onEvent({ type, appId });
  };
  const ownsApp = (appId) => {
    try { return new URLSearchParams(globalThis.location?.search || '').get('app') === appId; }
    catch { return false; }
  };
  const runtimeListener = (message, _sender, respond) => {
    if (message?.type !== 'natives-app-stop' || typeof message.appId !== 'string') return undefined;
    // runtime.sendMessage reaches every extension page. Only the matching
    // app.html owner may acknowledge a stop; the App Center is not evidence
    // that the Native Port has stopped.
    if (!ownsApp(message.appId)) return undefined;
    Promise.resolve(onEvent({ type: 'stop', appId: message.appId }))
      .then(() => respond?.({ stopped: true }), () => respond?.({ stopped: false }));
    return true;
  };
  globalThis.chrome?.runtime?.onMessage?.addListener?.(runtimeListener);
  return {
    notify: (type, appId) => { if (!closed) channel?.postMessage({ type, appId }); },
    close: () => { closed = true; channel?.close(); globalThis.chrome?.runtime?.onMessage?.removeListener?.(runtimeListener); },
  };
}
