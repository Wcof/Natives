import {
  isAuthenticatedLifecycleMessage,
  isExpectedMessageSource,
} from './message-source';

// ── Sandbox Security Constant ──
// MUST NOT include 'allow-same-origin' — that would let plugin iframes
// access the host's cookies, localStorage, and DOM (R-S2 violation).
export const IFRAME_SANDBOX = 'allow-scripts allow-forms';

/**
 * Runtime assertion: verify a sandbox attribute value does not contain
 * 'allow-same-origin'. Call this after setting sandbox on any iframe.
 * Throws in development, logs error in production.
 */
export function assertSecureSandbox(sandbox: string, context: string): void {
  if (sandbox.includes('allow-same-origin')) {
    const msg = `[SECURITY] iframe sandbox includes 'allow-same-origin' in ${context}. ` +
      'This violates R-S2: plugin iframes must never have same-origin access.';
    if (process.env.NODE_ENV === 'development') {
      throw new Error(msg);
    } else {
      console.error(msg);
    }
  }
}

export interface IframeInstance {
  moduleId: string;
  element: HTMLIFrameElement | null;
  createdAt: number;
  lastAccessed: number;
  state: 'loading' | 'hot' | 'warm' | 'cold' | 'persistent';
  /** Session token bound to this sandbox instance. Only valid for this
   *  module's lifetime; scoped to its own data namespace. */
  sessionToken?: string;
  /** Whether this is an AI-generated module (stricter isolation rules). */
  isGenerated?: boolean;
}

const MAX_BACKGROUND = 5;

// ── State persistence via unified IPC bridge (US16) ──
// Delegates to state-persistence.ts on the main process through
// the dedicated state:save / state:load / state:clear IPC channels.
// This avoids the previous fragmentation where the renderer wrote to
// module_data with module_id='__renderer__' while the main-process
// state-persistence.ts used module_id='__system__'.

function getStateApi() {
  return (typeof window !== 'undefined' && window.nativesAPI?.state) || null;
}

async function persistModuleState(moduleId: string, state: Record<string, unknown>): Promise<void> {
  try {
    const api = getStateApi();
    if (api) await api.save(moduleId, JSON.stringify(state));
  } catch { /* state preservation should not crash the app */ }
}

async function retrieveModuleState(moduleId: string): Promise<Record<string, unknown> | null> {
  try {
    const api = getStateApi();
    if (api) {
      const raw = await api.load(moduleId);
      return raw ? JSON.parse(raw) : null;
    }
  } catch { /* ignore */ }
  return null;
}

async function clearModuleState(moduleId: string): Promise<void> {
  try {
    const api = getStateApi();
    if (api) await api.clear(moduleId);
  } catch { /* ignore */ }
}

export class IframeManager {
  private instances = new Map<string, IframeInstance>();
  private accessOrder: string[] = [];
  private heartbeatTimers = new Map<string, ReturnType<typeof setInterval>>();
  private heartbeatTimeoutCallbacks = new Map<string, () => void>();
  private crashCallbacks = new Map<string, () => void>();
  private heartbeatMisses = new Map<string, number>();
  private crashOverlays = new Map<string, HTMLDivElement>();
  private messageListenerCleanups = new Map<string, () => void>();
  private tokenDispatches = new Map<string, Promise<void>>();

  createIframe(moduleId: string, url: string, isGenerated = false): HTMLIFrameElement {
    // If already exists, destroy old one
    this.destroyIframe(moduleId);

    const iframe = document.createElement('iframe');
    // ── ABSOLUTE SECURITY: sandbox without allow-same-origin ──
    // This forces the browser to assign a UNIQUE ORIGIN to the iframe.
    // The generated page CANNOT access the host's cookies, localStorage,
    // sessionStorage, or any same-origin resources. This is the #1 defense
    // line against data exfiltration from AI-generated code.
    iframe.setAttribute('sandbox', IFRAME_SANDBOX);
    assertSecureSandbox(IFRAME_SANDBOX, `createIframe(${moduleId})`);
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
      isGenerated,
    };

    // Loading updates lifecycle state only. The iframe SDK must request its
    // token after installing its listener (ADR-0001).
    iframe.onload = () => {
      instance.state = 'hot';
      instance.lastAccessed = Date.now();
    };

    // Listen for token-request messages from the sandbox (bridge SDK)
    this.setupMessageListener(moduleId, iframe);

    this.instances.set(moduleId, instance);
    this.touch(moduleId);

    return iframe;
  }

  showIframe(moduleId: string): HTMLIFrameElement | null {
    const instance = this.instances.get(moduleId);
    if (!instance) return null;

    const wasCold = instance.state === 'cold' || instance.state === 'persistent';
    instance.state = 'hot';
    instance.lastAccessed = Date.now();
    this.touch(moduleId);

    // Load persisted state on cold → hot transition
    if (wasCold) {
      retrieveModuleState(moduleId).then((saved) => {
        if (saved) {
          instance.lastAccessed = Number(saved.lastAccessed) || Date.now();
        }
      });
    }

    // Enforce LRU limit
    this.enforceLRU();

    return instance.element;
  }

  hideIframe(moduleId: string): void {
    const instance = this.instances.get(moduleId);
    if (instance) {
      instance.state = 'warm';
      // Persist state on hot → warm transition
      persistModuleState(moduleId, { lastAccessed: Date.now(), state: 'warm' });
    }
  }

  destroyIframe(moduleId: string): void {
    const instance = this.instances.get(moduleId);
    if (instance?.element) {
      // Persist state before destroying (warm → cold)
      if (instance.state === 'warm' || instance.state === 'cold') {
        persistModuleState(moduleId, { lastAccessed: Date.now(), state: 'cold' });
      }
      instance.element.onload = null;
      instance.element.remove();
    }
    // Clean up message listener
    this.messageListenerCleanups.get(moduleId)?.();
    this.messageListenerCleanups.delete(moduleId);
    this.stopHeartbeat(moduleId);
    this.heartbeatTimeoutCallbacks.delete(moduleId);
    this.crashCallbacks.delete(moduleId);
    this.removeCrashOverlay(moduleId);
    this.instances.delete(moduleId);
    this.accessOrder = this.accessOrder.filter((id) => id !== moduleId);
    // Clear persisted state on full uninstall
    clearModuleState(moduleId);
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
      if (instance.state === 'hot') count++;
    }
    return count;
  }

  getBackgroundCount(): number {
    let count = 0;
    for (const instance of this.instances.values()) {
      if (instance.state === 'warm') count++;
    }
    return count;
  }

  getAllModuleIds(): string[] {
    return Array.from(this.instances.keys());
  }

  /**
   * True when `source` matches a currently-managed module iframe's contentWindow.
   * Used to gate host-side window messages that modules may legitimately send
   * (e.g. shell:focus-base) without trusting event.origin (R-S3).
   */
  isManagedMessageSource(source: unknown): boolean {
    for (const instance of this.instances.values()) {
      if (instance.element?.contentWindow === source) return true;
    }
    return false;
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
          // Show crash overlay over the iframe container
          const instance = this.instances.get(moduleId);
          const container = instance?.element?.parentNode as HTMLElement | null;
          if (container) {
            this.showCrashOverlay(moduleId, container);
          }
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

  showCrashOverlay(moduleId: string, container: HTMLElement): void {
    this.removeCrashOverlay(moduleId);

    const overlay = document.createElement('div');
    overlay.style.position = 'absolute';
    overlay.style.top = '0';
    overlay.style.left = '0';
    overlay.style.width = '100%';
    overlay.style.height = '100%';
    overlay.style.display = 'flex';
    overlay.style.flexDirection = 'column';
    overlay.style.alignItems = 'center';
    overlay.style.justifyContent = 'center';
    overlay.style.background = 'rgba(0,0,0,0.7)';
    overlay.style.color = '#fff';
    overlay.style.zIndex = '1000';
    overlay.style.fontSize = '16px';
    overlay.style.gap = '12px';

    const message = document.createElement('span');
    message.textContent = 'Plugin crashed. Click to reload.';
    overlay.appendChild(message);

    const reloadBtn = document.createElement('button');
    reloadBtn.textContent = 'Reload';
    reloadBtn.style.padding = '8px 20px';
    reloadBtn.style.border = 'none';
    reloadBtn.style.borderRadius = '4px';
    reloadBtn.style.background = 'var(--primary)';
    reloadBtn.style.color = '#fff';
    reloadBtn.style.cursor = 'pointer';
    reloadBtn.style.fontSize = '14px';

    reloadBtn.addEventListener('click', () => {
      // Re-create the iframe by extracting the src from the existing iframe
      const instance = this.instances.get(moduleId);
      if (instance?.element?.src) {
        this.createIframe(moduleId, instance.element.src);
      }
      this.removeCrashOverlay(moduleId);
    });

    overlay.appendChild(reloadBtn);
    container.appendChild(overlay);
    this.crashOverlays.set(moduleId, overlay);
  }

  removeCrashOverlay(moduleId: string): void {
    const overlay = this.crashOverlays.get(moduleId);
    if (overlay) {
      overlay.remove();
      this.crashOverlays.delete(moduleId);
    }
  }

  // ── Two-Stage Token Handshake ──

  /**
   * Dispatch a session-scoped token into the sandbox iframe via postMessage.
   * The token is generated by the Rust backend (HMAC-SHA256) and is bound
   * to this specific module_id. It grants access ONLY to the module's own
   * data namespace (`custom_module_data_[module_id]`).
   */
  private dispatchSessionToken(
    moduleId: string,
    expectedIframe: HTMLIFrameElement,
  ): Promise<void> {
    const previous = this.tokenDispatches.get(moduleId) ?? Promise.resolve();
    const dispatch = previous
      .catch(() => {})
      .then(() => this.grantSessionToken(moduleId, expectedIframe));
    this.tokenDispatches.set(moduleId, dispatch);
    void dispatch.finally(() => {
      if (this.tokenDispatches.get(moduleId) === dispatch) {
        this.tokenDispatches.delete(moduleId);
      }
    });
    return dispatch;
  }

  private async grantSessionToken(
    moduleId: string,
    expectedIframe: HTMLIFrameElement,
  ): Promise<void> {
    const instance = this.instances.get(moduleId);
    if (instance?.element !== expectedIframe || !expectedIframe.contentWindow) return;
    const target = expectedIframe.contentWindow;

    try {
      // Generate token via Tauri backend (HMAC-SHA256, 24h TTL)
      const token = await window.nativesAPI?.bridge?.generateToken(moduleId);
      if (!token) return;

      // A reload/recreate may occur while the Host command is in flight.
      // Never deliver that result to a replacement iframe.
      if (this.instances.get(moduleId) !== instance || instance.element !== expectedIframe) return;

      instance.sessionToken = token;

      // Stage 2: push token + module_id + namespace constraint into sandbox
      target.postMessage(
        {
          type: 'token-granted',
          token,
          moduleId,
          // Namespace isolation: all db operations from this sandbox are
          // forcibly scoped to keys prefixed with this module's namespace.
          // The backend enforces this in route_bridge.
          namespace: `custom_module_data_${moduleId}`,
        },
        '*',
      );
    } catch {
      // Fail closed: no token means no Workshop Bridge access.
      console.error('[IframeManager] Session token handshake failed; Bridge access denied');
    }
  }

  /**
   * Set up a message listener on the host window to handle requests
   * from the sandbox iframe. Data operations use the authenticated HTTP
   * Bridge; postMessage is limited to handshake and lifecycle signals.
   */
  private setupMessageListener(moduleId: string, iframe: HTMLIFrameElement): void {
    // SSR / Node test environments have no `window` — skip listener setup.
    // The iframe still works; only host-side message routing is disabled.
    if (typeof window === 'undefined') return;

    // Clean up any existing listener
    this.messageListenerCleanups.get(moduleId)?.();

    const handler = async (event: MessageEvent) => {
      // Verify the message came from our iframe (R-S3: MessageEvent.source check)
      if (!isExpectedMessageSource(event, iframe.contentWindow)) return;
      const data = event.data;
      if (!data || typeof data !== 'object') return;

      const instance = this.instances.get(moduleId);
      if (!instance) return;

      // Handle token-request from bridge SDK
      if (data.type === 'token-request') {
        // Fail closed while rotating: lifecycle events from the prior session
        // must stop being accepted as soon as a new handshake begins.
        instance.sessionToken = undefined;
        await this.dispatchSessionToken(moduleId, iframe);
        return;
      }

      if (isAuthenticatedLifecycleMessage(
        event,
        iframe.contentWindow,
        moduleId,
        instance.sessionToken,
      )) {
        if (data.type === 'lifecycle:heartbeat') this.onHeartbeatReceived(moduleId);
        if (data.type === 'lifecycle:ready') instance.lastAccessed = Date.now();
        return;
      }
    };

    window.addEventListener('message', handler);
    this.messageListenerCleanups.set(moduleId, () => {
      window.removeEventListener('message', handler);
    });
  }

  private touch(moduleId: string): void {
    this.accessOrder = this.accessOrder.filter((id) => id !== moduleId);
    this.accessOrder.push(moduleId);
  }

  private enforceLRU(): void {
    // Count background instances
    const bgIds = this.accessOrder.filter((id) => {
      const inst = this.instances.get(id);
      return inst?.state === 'warm';
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
        .filter((i) => i.state === 'warm' || i.state === 'cold')
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
