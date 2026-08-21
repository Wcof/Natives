// ── WorkspaceDataBroker（B-016） ──
// adapter key 去重 / 缓存 / 共享订阅：
//   · 相同 key 只触发一次 fetch（in-flight promise 共享，不重复请求）。
//   · 多个订阅者共享同一份数据与刷新事件。
//   · 事件驱动刷新（DB change / domain event）只 invalidate + refetch，不重建。
// Renderer 不直连 IPC —— 数据一律来自 broker loader（domain query/facade）。

import type { WidgetDataContext, WidgetDefinition, WidgetConfig } from './types';

export type BrokerLoader<T> = (ctx: WidgetDataContext) => Promise<T>;

export type BrokerStatus = 'idle' | 'loading' | 'ready' | 'error';

export interface BrokerSnapshot<T = unknown> {
  key: string;
  status: BrokerStatus;
  data: T | null;
  error: Error | null;
  /** 有旧数据且正在后台刷新。 */
  refetching: boolean;
  lastUpdated: number | null;
}

type Listener<T = unknown> = (snapshot: BrokerSnapshot<T>) => void;

interface BrokerEntry {
  status: BrokerStatus;
  data: unknown;
  error: Error | null;
  promise: Promise<unknown> | null;
  controller: AbortController | null;
  loader: BrokerLoader<unknown> | null;
  lastUpdated: number | null;
  refetching: boolean;
  eventUnsub: (() => void) | null;
}

export interface BrokerSubscribeOptions<T> {
  /** 距上次成功多久后重新拉取（默认 0 = 每次新订阅者都重新拉取，但共享在途请求）。 */
  staleTimeMs?: number;
  /** 事件驱动刷新（DB change / domain event）。 */
  onEvent?: (emit: () => void) => (() => void) | void;
  /** 每次快照变化时的回调（必填：订阅者依赖它接收数据）。 */
  onSnapshot?: (snapshot: BrokerSnapshot<T>) => void;
}

/** 默认 staleTime：15s（同一布局内快速重挂载不重复请求）。 */
const DEFAULT_STALE_MS = 15_000;

export class WorkspaceDataBroker {
  private entries = new Map<string, BrokerEntry>();
  private listeners = new Map<string, Set<Listener>>();

  /** 同步读取当前快照（用于 hook 初始 state）。 */
  getSnapshot<T>(key: string): BrokerSnapshot<T> {
    const entry = this.entries.get(key);
    if (!entry) {
      return { key, status: 'idle', data: null, error: null, refetching: false, lastUpdated: null };
    }
    return {
      key,
      status: entry.status,
      data: entry.data as T | null,
      error: entry.error,
      refetching: entry.refetching,
      lastUpdated: entry.lastUpdated,
    };
  }

  /**
   * 订阅一个 adapter key。返回取消订阅函数。
   * 同 key 多次订阅共享一次 load（不重复 fetch）。
   */
  subscribe<T>(key: string, loader: BrokerLoader<T> | null, options: BrokerSubscribeOptions<T> = {}): () => void {
    const entry = this.ensureEntry(key);
    if (loader && !entry.loader) {
      entry.loader = loader as BrokerLoader<unknown>;
    }

    let set = this.listeners.get(key);
    if (!set) {
      set = new Set<Listener>();
      this.listeners.set(key, set);
    }

    // 首个订阅者才挂事件刷新（AI Status DB change 等）。
    if (options.onEvent && !entry.eventUnsub) {
      const unsub = options.onEvent(() => this.refetch(key));
      entry.eventUnsub = typeof unsub === 'function' ? unsub : null;
    }

    const listener: Listener<T> | undefined = options.onSnapshot;

    // 无 onSnapshot 的订阅者（例如仅预热 cache）仍需要一个占位回调，
    // 避免 notify 遍历时抛空。
    const activeListener = (listener ?? (() => {})) as Listener<unknown>;
    set.add(activeListener);

    // 首次订阅立即同步当前快照。
    options.onSnapshot?.(this.getSnapshot<T>(key));

    // 触发加载：idle 或 超过 staleTime
    const needsLoad =
      entry.status === 'idle' ||
      (entry.status === 'ready' &&
        options.staleTimeMs !== Infinity &&
        Date.now() - (entry.lastUpdated ?? 0) > (options.staleTimeMs ?? DEFAULT_STALE_MS));

    if (needsLoad) {
      this.load(key);
    } else if (entry.status === 'error') {
      // error 状态也允许再次订阅时自动重试（避免卡死在错误态）。
      this.load(key);
    }

    let active = true;
    return () => {
      if (!active) return;
      active = false;
      set.delete(activeListener);
      if (set.size === 0) {
        this.listeners.delete(key);
        if (entry.eventUnsub) {
          entry.eventUnsub();
          entry.eventUnsub = null;
        }
        // 保留缓存；中止 in-flight（避免孤儿请求占资源）。
        if (entry.status === 'loading') {
          entry.controller?.abort();
          entry.controller = null;
          entry.promise = null;
          entry.status = entry.data != null ? 'ready' : 'idle';
        }
      }
    };
  }

  /** 失效缓存：清数据并（如有订阅者）重新加载。 */
  invalidate(key: string): void {
    const entry = this.entries.get(key);
    if (!entry) return;
    entry.status = 'idle';
    entry.data = null;
    entry.error = null;
    entry.lastUpdated = null;
    entry.promise = null;
    this.notify(key);
    if ((this.listeners.get(key)?.size ?? 0) > 0) {
      this.load(key);
    }
  }

  /** 失效所有 key（主题/工作区切换时可选）。 */
  invalidateAll(): void {
    for (const key of [...this.entries.keys()]) {
      this.invalidate(key);
    }
  }

  /** 后台刷新：保留旧数据，refetch 后整体替换。 */
  refetch(key: string): void {
    const entry = this.entries.get(key);
    if (!entry) return;
    if (entry.status === 'loading') return; // 已在途，共享。
    this.load(key);
  }

  private ensureEntry(key: string): BrokerEntry {
    let entry = this.entries.get(key);
    if (!entry) {
      entry = {
        status: 'idle',
        data: null,
        error: null,
        promise: null,
        controller: null,
        loader: null,
        lastUpdated: null,
        refetching: false,
        eventUnsub: null,
      };
      this.entries.set(key, entry);
    }
    return entry;
  }

  private notify(key: string): void {
    const snapshot = this.getSnapshot(key);
    const set = this.listeners.get(key);
    if (!set) return;
    for (const listener of set) {
      try {
        (listener as Listener)(snapshot);
      } catch (err) {
        console.error('[data-broker] listener threw:', err);
      }
    }
  }

  private load(key: string): void {
    const entry = this.ensureEntry(key);
    if (entry.status === 'loading' && entry.promise) {
      // 同 key in-flight 共享 —— 不重复 fetch。
      entry.promise
        .catch(() => {})
        .then(() => this.notify(key));
      return;
    }
    if (!entry.loader) {
      // 无 loader（纯展示 Widget）：直接 ready，data=null。
      entry.status = 'ready';
      entry.data = null;
      entry.error = null;
      entry.lastUpdated = Date.now();
      this.notify(key);
      return;
    }

    entry.controller = new AbortController();
    entry.status = 'loading';
    entry.refetching = entry.lastUpdated != null;
    this.notify(key);

    const ctx: WidgetDataContext = { signal: entry.controller.signal };
    const promise = Promise.resolve().then(() => entry.loader!(ctx));
    entry.promise = promise;

    promise.then(
      (data) => {
        const current = this.entries.get(key);
        if (!current || current.promise !== promise) return; // 已被后续请求取代
        current.status = 'ready';
        current.data = data;
        current.error = null;
        current.lastUpdated = Date.now();
        current.refetching = false;
        current.promise = null;
        this.notify(key);
      },
      (err: unknown) => {
        const current = this.entries.get(key);
        if (!current || current.promise !== promise) return;
        if (err instanceof DOMException && err.name === 'AbortError') {
          current.status = current.data != null ? 'ready' : 'idle';
        } else {
          current.status = 'error';
          current.error = err instanceof Error ? err : new Error(String(err));
        }
        current.refetching = false;
        current.promise = null;
        this.notify(key);
      },
    );
  }
}

/** 全局单例（Renderer/Inspector 共用，保证同 key 去重跨组件生效）。 */
export const workspaceDataBroker = new WorkspaceDataBroker();

/**
 * 计算 Widget 的 broker key：
 * 优先用 def.adapterKeyBuilder（Today Usage 与 Token Metrics 共享 usage.summary key），
 * 否则退回 def.type。
 */
export function buildWidgetBrokerKey<TSettings extends Record<string, unknown>>(
  def: Pick<WidgetDefinition<unknown, TSettings>, 'adapterKeyBuilder' | 'type'>,
  config: WidgetConfig<TSettings>,
): string {
  return def.adapterKeyBuilder ? def.adapterKeyBuilder(config) : def.type;
}
