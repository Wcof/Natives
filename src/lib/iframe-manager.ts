export interface IframeInstance {
  moduleId: string;
  element: HTMLIFrameElement | null;
  createdAt: number;
  lastAccessed: number;
  state: 'active' | 'background' | 'loading';
}

const MAX_BACKGROUND = 5;
const MEMORY_THRESHOLD = 1024; // 1GB in MB (approximate)

export class IframeManager {
  private instances = new Map<string, IframeInstance>();
  private accessOrder: string[] = [];

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

  // Check system memory and evict if needed
  checkMemory(): void {
    if ('memory' in performance) {
      const mem = (performance as unknown as { memory: { jsHeapUsedLimit: number } }).memory;
      if (mem && mem.jsHeapUsedLimit > 0) {
        const memMB = mem.jsHeapUsedLimit / (1024 * 1024);
        if (memMB < MEMORY_THRESHOLD) {
          // Low memory - evict oldest background
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
