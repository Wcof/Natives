// ── WorkspaceDataBroker（B-016） ──
// adapter key 去重 / 缓存 / 共享订阅：
//   · 相同 key 只触发一次 fetch（in-flight promise 共享，不重复请求）。
//   · 多个订阅者共享同一份数据与刷新事件。
//   · 事件驱动刷新（DB change / domain event）只 invalidate + refetch，不重建。
// Renderer 不直连 IPC —— 数据一律来自 broker loader（domain query/facade）。

import type { WidgetDataContext, WidgetDefinition, WidgetConfig, TimeRange } from './types';

export type BrokerLoader<T> = (ctx: WidgetDataContext) => Promise<T>;

export type BrokerStatus = 'idle' | 'loading' | 'ready' | 'error';

/** WS-05: manual sync outcome. `lastSyncedAt` only advances on `ok`. */
export type SyncOutcome = 'ok' | 'partial' | 'failed' | 'noop';

export interface SyncResult {
  outcome: SyncOutcome;
  /** Number of data components that refreshed successfully. */
  succeeded: number;
  /** Number of data components whose refresh failed (kept their old data). */
  failed: number;
  /** Human-facing failure detail (first error message), if any. */
  detail?: string;
}

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
  lastAccessed: number;
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
const MAX_CACHE_ENTRIES = 64;
const MAX_LISTENERS_PER_KEY = 32;
const MAX_CONCURRENT_LOADS = 6;

export class WorkspaceDataBroker {
  private entries = new Map<string, BrokerEntry>();
  private listeners = new Map<string, Set<Listener>>();
  private _lastSyncedAt: number | null = null;
  private _syncing = false;
  private _syncListeners = new Set<() => void>();
  private _timeRange: TimeRange = '7d';
  private activeLoads = 0;
  private queuedLoads = new Set<string>();

  get lastSyncedAt(): number | null { return this._lastSyncedAt; }
  get syncing(): boolean { return this._syncing; }
  get timeRange(): TimeRange { return this._timeRange; }

  setTimeRange(range: TimeRange): void {
    this._timeRange = range;
  }

  subscribeSyncStatus(fn: () => void): () => void {
    this._syncListeners.add(fn);
    return () => this._syncListeners.delete(fn);
  }

  private notifySyncStatus(): void {
    for (const fn of this._syncListeners) {
      try { fn(); } catch { /* listener threw */ }
    }
  }

  /**
   * 触发所有有订阅者的 entry 同步刷新（去重，不重复发请求）。
   * WS-05：返回真实结果摘要——只有全部成功(outcome=ok)才推进 lastSyncedAt；
   * 部分失败(partial)或全部失败(failed)保留旧数据并如实回报，禁止伪装成功。
   * 并发调用直接返回（owner 自行去重，避免重复刷新）。
   */
  async syncAll(): Promise<SyncResult> {
    if (this._syncing) return { outcome: 'noop', succeeded: 0, failed: 0 };
    this._syncing = true;
    this.notifySyncStatus();

    const activeKeys = [...this.listeners.keys()].filter((key) => {
      const entry = this.entries.get(key);
      return entry?.loader != null;
    });

    if (activeKeys.length === 0) {
      // No mounted data components: nothing to sync. Do not advance the
      // "last synced" clock for a no-op.
      this._syncing = false;
      this.notifySyncStatus();
      return { outcome: 'noop', succeeded: 0, failed: 0 };
    }

    // Snapshot each entry's promise BEFORE refetch so settle results map to
    // exactly this sync round (not a later in-flight request replacing it).
    for (const key of activeKeys) {
      this.refetch(key);
    }

    const entryByKey = (key: string) => this.entries.get(key);
    const settled = await Promise.allSettled(
      activeKeys.map((key) => {
        const entry = entryByKey(key);
        return entry?.promise
          ? entry.promise.then(() => ({ key, ok: true }), () => ({ key, ok: false }))
          : Promise.resolve({ key, ok: true });
      }),
    );

    // Resolve settle outcomes against the CURRENT status of each key: an entry
    // still in 'error' after settle counts as failed (its old data is kept).
    let succeeded = 0;
    let failed = 0;
    let firstDetail: string | undefined;
    for (const result of settled) {
      const payload = result.status === 'fulfilled' ? result.value : null;
      const key = payload?.key;
      const entry = key ? entryByKey(key) : undefined;
      const failedNow = !payload?.ok || entry?.status === 'error';
      if (failedNow) {
        failed += 1;
        if (!firstDetail) {
          const msg = entry?.error?.message;
          if (msg) firstDetail = msg;
        }
      } else {
        succeeded += 1;
      }
    }

    this._syncing = false;
    // Only a fully successful sync advances the "last synced" timestamp.
    if (failed === 0) {
      this._lastSyncedAt = Date.now();
    }
    this.notifySyncStatus();

    const outcome: SyncOutcome = failed === 0 ? 'ok' : succeeded === 0 ? 'failed' : 'partial';
    return { outcome, succeeded, failed, detail: firstDetail };
  }

  /** 同步读取当前快照（用于 hook 初始 state）。 */
  getSnapshot<T>(key: string): BrokerSnapshot<T> {
    const entry = this.entries.get(key);
    if (!entry) {
      return { key, status: 'idle', data: null, error: null, refetching: false, lastUpdated: null };
    }
    entry.lastAccessed = Date.now();
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
    if (set.size >= MAX_LISTENERS_PER_KEY) throw new Error(`[data-broker] listener limit exceeded for '${key}'`);
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
          this.activeLoads = Math.max(0, this.activeLoads - 1);
          entry.controller = null;
          entry.promise = null;
          entry.status = entry.data != null ? 'ready' : 'idle';
          this.drainLoadQueue();
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
        lastAccessed: Date.now(),
      };
      this.entries.set(key, entry);
      this.evictInactiveEntries();
    }
    return entry;
  }

  private evictInactiveEntries(): void {
    if (this.entries.size <= MAX_CACHE_ENTRIES) return;
    const candidates = [...this.entries.entries()]
      .filter(([key, entry]) => !this.listeners.has(key) && !entry.promise)
      .sort((a, b) => a[1].lastAccessed - b[1].lastAccessed);
    for (const [key] of candidates) {
      this.entries.delete(key);
      if (this.entries.size <= MAX_CACHE_ENTRIES) break;
    }
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

    if (this.activeLoads >= MAX_CONCURRENT_LOADS) {
      this.queuedLoads.add(key);
      return;
    }

    this.activeLoads += 1;
    entry.controller = new AbortController();
    entry.status = 'loading';
    entry.refetching = entry.lastUpdated != null;
    this.notify(key);

    const ctx: WidgetDataContext = { signal: entry.controller.signal, timeRange: this._timeRange };
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
        this.finishLoad();
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
        this.finishLoad();
      },
    );
  }

  private finishLoad(): void {
    this.activeLoads = Math.max(0, this.activeLoads - 1);
    this.drainLoadQueue();
  }

  private drainLoadQueue(): void {
    while (this.activeLoads < MAX_CONCURRENT_LOADS) {
      const key = this.queuedLoads.values().next().value as string | undefined;
      if (!key) return;
      this.queuedLoads.delete(key);
      if (this.listeners.has(key)) this.load(key);
    }
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
  def: Pick<WidgetDefinition<unknown, TSettings>, 'adapterKeyBuilder' | 'type' | 'timeAware'>,
  config: WidgetConfig<TSettings>,
  timeRange?: TimeRange,
): string {
  const base = def.adapterKeyBuilder ? def.adapterKeyBuilder(config) : def.type;
  return def.timeAware && timeRange ? `${base}:${timeRange}` : base;
}
