// tokenusage 模块页面级共享查询层。
//
// 关键契约：
//   - 相同 `(method, normalizedFilter)` 的并发请求合并为一次 Host 查询，
//     N 个消费组件同参只发 1 次（验收矩阵 §9.3 第一条）；
//   - 结果按 revision 缓存（上限 32 组），`model_usage_updated` 等事件
//     递增 revision 后缓存整体失效；
//   - 一个组件销毁不中止其他组件共用的查询；最后一个订阅者
//     取消后才释放上游（引用计数在 client.js）。
//
// 缓存只存最近结果；绝不把陈旧数据标成实时——revision 失效后下一次
// 调用必然重新查询。

import { subscribeModelEvents } from './client.js';

const MAX_CACHE_ENTRIES = 32;

// cache: Map<key, { revision, promise }>
const cache = new Map();
// inflight: Map<key, { revision, promise }>，同版本的同参请求合并
const inflight = new Map();

let usageRevision = 0;
// Register before renderer subscriptions so their refresh observes the new revision.
subscribeModelEvents((evt) => {
  if (!evt || typeof evt !== 'object') return;
  // Host 推送的数据变更事件：revision 递增 → 缓存整体失效。
  if (['model_usage_updated', 'model_quota_updated', 'model_client_released', 'model_changed', 'resync'].includes(evt.type || evt.method)) {
    invalidateQueries();
  }
});

function cacheKey(method, params) {
  return method + '?' + JSON.stringify(normalizeParams(params));
}

function normalizeParams(params) {
  if (!params || typeof params !== 'object') return {};
  const out = {};
  for (const k of Object.keys(params).sort()) {
    out[k] = params[k];
  }
  return out;
}

function evictOldest() {
  while (cache.size > MAX_CACHE_ENTRIES) {
    const oldest = cache.keys().next().value;
    cache.delete(oldest);
  }
}

// sharedCall 合并同参调用并按 revision 缓存。
export function sharedCall(api, method, params = {}) {
  const key = cacheKey(method, params);

  const cached = cache.get(key);
  if (cached && cached.revision === usageRevision) {
    return cached.promise;
  }
  const running = inflight.get(key);
  if (running?.revision === usageRevision) {
    return running.promise;
  }

  const revision = usageRevision;
  const promise = api[method](params)
    .then((result) => {
      if (revision === usageRevision) {
        cache.set(key, { revision, promise: Promise.resolve(result) });
        evictOldest();
      }
      return result;
    })
    .finally(() => {
      if (inflight.get(key)?.promise === promise) inflight.delete(key);
    });
  inflight.set(key, { revision, promise });
  return promise;
}

// invalidateQueries 手动失效（导入/价格变更后由页面显式调用；Host 事件之外的安全兜底）。
export function invalidateQueries() {
  usageRevision += 1;
  cache.clear();
}

// resetSharedQueriesForTest 清空全部内部状态（仅测试使用）。
export function resetSharedQueriesForTest() {
  cache.clear();
  inflight.clear();
  usageRevision += 1;
}
