// Core coordinates module maintenance across browser profiles and entry types.
import { subscribeCoreEvents } from './platform/events.js';
import { createNativeClient } from './native-client.js';
export function appLifecycle(onEvent, appId, {subscribe=subscribeCoreEvents,client=createNativeClient({host:'com.natives.file_manager'})}={}) {
  let unsubscribe, generation=0;
  const close = () => { generation++; unsubscribe?.(); unsubscribe=undefined; client.disconnect(); };
  const resume = () => {
    if (unsubscribe) return;
    unsubscribe=subscribe(async event => {
      if (event.type==='apps_changed' && event.appId===appId && event.maintenance) {
        generation++; await onEvent({type:'maintenance',appId}); return;
      }
      if (event.type!=='apps_changed' && (event.type!=='resync' || event.reason==='unavailable')) return;
      const current=++generation;
      try {
        const detail=await client.call('apps:get',{appId});
        if (current===generation && detail.app?.enabled===false) await onEvent({type:'maintenance',appId});
      } catch { /* Keep the draft while Core is unavailable; resync retries the read. */ }
    },['apps']);
  };
  resume();
  return {close,resume};
}
