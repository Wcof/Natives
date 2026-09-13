// AI Performance shared Model Host client (ADR-0028 D1, T7).
//
// The usage data authority is model-host's usage_events store. The Space page
// reaches it over Native Messaging with the same contract as the model
// settings page; the client is reference-counted so N widgets share one port
// and the port is dropped when the last widget is destroyed (no resident
// process beyond the open port).
//
// T7 additions: Host push events (`model_usage_updated`, ...) are dispatched
// to page-level subscribers so the shared query layer can invalidate caches
// and refresh widgets. The API singleton is created once with the dispatcher
// wired in — later subscribers never miss events.

import { createModelSettingsAPI } from '../model-settings-api.js';

let api = null;
let refCount = 0;
const eventListeners = new Set();

function dispatchEvent(evt) {
  for (const listener of eventListeners) {
    try {
      listener(evt);
    } catch {
      // 一个订阅者抛错不影响其他订阅者。
    }
  }
}

export function acquireModelApi() {
  if (!api) api = createModelSettingsAPI({ onEvent: dispatchEvent });
  refCount += 1;
  return api;
}

export function releaseModelApi() {
  refCount = Math.max(0, refCount - 1);
  if (refCount === 0 && api) {
    api.disconnect();
    api = null;
  }
}

// subscribeModelEvents 注册页面级事件监听（Host 推送），返回取消函数。
export function subscribeModelEvents(listener) {
  eventListeners.add(listener);
  return () => eventListeners.delete(listener);
}

// __emitModelEventForTest 仅测试使用：模拟 Host 推送一条事件。
export function __emitModelEventForTest(evt) {
  dispatchEvent(evt);
}
