export interface IframeInstance {
  moduleId: string;
  element: HTMLIFrameElement | null;
  createdAt: number;
  lastAccessed: number;
  state: 'active' | 'background' | 'loading';
}

const MAX_BACKGROUND = 5;

export class IframeManager {
  private instances = new Map<string, IframeInstance>();
  private accessOrder: string[] = [];
  private heartbeatTimers = new Map<string, ReturnType<typeof setInterval>>();
  private heartbeatTimeoutCallbacks = new Map<string, () => void>();
  private crashCallbacks = new Map<string, () => void>();
  private heartbeatMisses = new Map<string, number>();

  createIframe(moduleId: string, url: string): HTMLIFrameElement {
    // If already exists, destroy old one
    this.destroyIframe(moduleId);

    const iframe = document.createElement('iframe');
    iframe.setAttribute('sandbox', 'allow-scripts allow-forms');
    iframe.setAttribute('src', url);
    iframe.style.width = '100%';
    iframe.style.height = '100%';
    iframe.style.border = 'none';

    const instance: IframeInstance = {
      moduleId,
      element: iframe,
      createdAt: Date.now(),
      lastAccessed: Date.now(),
      state: 'loading',
    };

    iframe.onload = () => {
      instance.state = 'active';
      instance.lastAccessed = Date.now();
    };

    this.instances.set(moduleId, instance);
    this.touch(moduleId);

    return iframe;
  }

  showIframe(moduleId: string): HTMLIFrameElement | null {
    const instance = this.instances.get(moduleId);
    if (!instance) return null;

    instance.state = 'active';
    instance.lastAccessed = Date.now();
    this.touch(moduleId);

    // Enforce LRU limit
    this.enforceLRU();

    return instance.element;
  }

  hideIframe(moduleId: string): void {
    const instance = this.instances.get(moduleId);
    if (instance) {
      instance.state = 'background';
    }
  }

  destroyIframe(moduleId: string): void {
    const instance = this.instances.get(moduleId);
    if (instance?.element) {
      instance.element.remove();
    }
    this.instances.delete(moduleId);
    this.accessOrder = this.accessOrder.filter((id) => id !== moduleId);
  }

  destroyAll(): void {
    for (const [id] of this.instances) {
      this.destroyIframe(id);
    }
  }

  getInstance(moduleId: string): IframeInstance | undefined {
    return this.instances.get(moduleId);
  }

  getActiveCount(): number {
    let count = 0;
    for (const instance of this.instances.values()) {
      if (instance.state === 'active') count++;
    }
    return count;
  }

  getBackgroundCount(): number {
    let count = 0;
    for (const instance of this.instances.values()) {
      if (instance.state === 'background') count++;
    }
    return count;
  }

  getAllModuleIds(): string[] {
    return Array.from(this.instances.keys());
  }

  // ── Heartbeat & Crash Callbacks ──

  onHeartbeatTimeout(moduleId: string, cb: () => void): void {
    this.heartbeatTimeoutCallbacks.set(moduleId, cb);
  }

  onCrash(moduleId: string, cb: () => void): void {
    this.crashCallbacks.set(moduleId, cb);
  }

  /** Call this when a heartbeat message is received from a plugin */
  onHeartbeatReceived(moduleId: string): void {
    const instance = this.instances.get(moduleId);
    if (instance) {
      instance.lastAccessed = Date.now();
    }
    this.heartbeatMisses.set(moduleId, 0);
  }

  startHeartbeat(moduleId: string, intervalMs = 5000): void {
    // Clear existing timer
    this.stopHeartbeat(moduleId);
    this.heartbeatMisses.set(moduleId, 0);

    const timer = setInterval(() => {
      const instance = this.instances.get(moduleId);
      if (!instance) {
        this.stopHeartbeat(moduleId);
        return;
      }

      const elapsed = Date.now() - instance.lastAccessed;
      if (elapsed > intervalMs * 3) {
        // 3 missed heartbeats
        const misses = (this.heartbeatMisses.get(moduleId) || 0) + 1;
        this.heartbeatMisses.set(moduleId, misses);

        if (misses >= 3) {
          const cb = this.heartbeatTimeoutCallbacks.get(moduleId);
          cb?.();
          const crashCb = this.crashCallbacks.get(moduleId);
          crashCb?.();
          this.destroyIframe(moduleId);
        }
      } else {
        this.heartbeatMisses.set(moduleId, 0);
      }
    }, intervalMs);

    this.heartbeatTimers.set(moduleId, timer);
  }

  stopHeartbeat(moduleId: string): void {
    const timer = this.heartbeatTimers.get(moduleId);
    if (timer) {
      clearInterval(timer);
      this.heartbeatTimers.delete(moduleId);
    }
    this.heartbeatMisses.delete(moduleId);
  }

  private touch(moduleId: string): void {
    this.accessOrder = this.accessOrder.filter((id) => id !== moduleId);
    this.accessOrder.push(moduleId);
  }

  private enforceLRU(): void {
    // Count background instances
    const bgIds = this.accessOrder.filter((id) => {
      const inst = this.instances.get(id);
      return inst?.state === 'background';
    });

    if (bgIds.length <= MAX_BACKGROUND) return;

    // Remove oldest background instances
    const toRemove = bgIds.slice(0, bgIds.length - MAX_BACKGROUND);
    for (const id of toRemove) {
      this.destroyIframe(id);
    }
  }

  // Check memory usage and evict if too many iframes
  checkMemory(): void {
    const totalCount = this.instances.size;
    const backgroundCount = this.getBackgroundCount();

    // If total iframes exceed threshold, evict oldest background
    if (totalCount > MAX_BACKGROUND + 2 && backgroundCount > 0) {
      const oldestBg = this.accessOrder
        .map((id) => this.instances.get(id)!)
        .filter((i) => i.state === 'background')
        .sort((a, b) => a.lastAccessed - b.lastAccessed);

      if (oldestBg.length > 0) {
        this.destroyIframe(oldestBg[0]!.moduleId);
      }
    }
  }
}

// Singleton
let globalManager: IframeManager | null = null;

export function getIframeManager(): IframeManager {
  if (!globalManager) {
    globalManager = new IframeManager();
  }
  return globalManager;
}
