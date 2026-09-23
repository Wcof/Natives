// tokenusage 模块共享 Model Host 客户端（ADR-0028 D1, T7）。
//
// 统一封装 AI 计量数据权威（model-host 的 usage_events 存储）。
// 页面级引用计数保证并发模块/小组件共享同一个 Native Messaging Port，
// 最后一个订阅者释放时断开连接，避免常驻进程。
//
// Host 推送事件（如 model_usage_updated 等）分发至页面级订阅者，
// 驱动共享查询层失效与自动刷新。

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
