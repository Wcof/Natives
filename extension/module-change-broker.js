import { subscribeCoreEvents } from './platform/events.js';

// Only invalidation crosses the sandbox; values remain with the module owner.
export function createModuleChangeBroker({ appId, context, subscribe = subscribeCoreEvents }) {
  const originId = crypto.randomUUID();
  let timer, disposed = false;
  const unsubscribe = subscribe(event => {
    if (event.type !== 'resync' && (event.type !== 'module_changed' || event.appId !== appId || event.originId === originId)) return;
    const { frame, generation, challenge } = context();
    frame?.contentWindow?.postMessage({ type: 'natives:data-invalidated', generation, challenge }, '*');
  }, ['modules']);
  return {
    handle(event) {
      if (event.data?.type !== 'natives:data-changed') return false;
      const { frame, generation, challenge, host } = context();
      const message = event.data;
      if (disposed || !host || !frame || event.source !== frame.contentWindow || !generation || !challenge || message.generation !== generation || message.challenge !== challenge || Object.keys(message).some(key => !['type', 'generation', 'challenge'].includes(key))) return true;
      if (!timer) timer = setTimeout(() => {
        timer = undefined;
        if (disposed) return;
        const current = context();
        if (current.host !== host || current.generation !== generation) return;
        // A lost hint is recovered by resync; never replay the business write.
        void host.call('app:invalidate', { appId, originId }).catch(() => {});
      }, 100);
      return true;
    },
    dispose() { disposed = true; clearTimeout(timer); unsubscribe(); },
  };
}
