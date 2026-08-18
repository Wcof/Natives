import { describe, it, beforeEach, afterEach } from 'node:test';
import assert from 'node:assert/strict';
import { IframeManager } from './iframe-manager';

function createMockIframe() {
  const attrs: Record<string, string> = {};
  const style: Record<string, string> = {};
  let _onload: (() => void) | null = null;
  let removed = false;
  const messages: Array<{ data: unknown; targetOrigin: string }> = [];
  const contentWindow = {
    postMessage: (data: unknown, targetOrigin: string) => {
      messages.push({ data, targetOrigin });
    },
  };
  return {
    setAttribute: (name: string, value: string) => { attrs[name] = value; },
    getAttribute: (name: string) => attrs[name],
    get style() { return style; },
    get onload() { return _onload; },
    set onload(fn: (() => void) | null) { _onload = fn; },
    remove: () => { removed = true; },
    get removed() { return removed; },
    contentWindow,
    messages,
    triggerLoad() { _onload?.(); },
  };
}

type MockIframeElement = ReturnType<typeof createMockIframe>;

function setupMocks() {
  const mockElements: MockIframeElement[] = [];
  const messageListeners = new Set<(event: MessageEvent) => unknown>();
  let tokenSequence = 0;

  (globalThis as unknown as { document?: unknown }).document = {
    createElement: (tag: string) => {
      if (tag !== 'iframe') throw new Error(`Unexpected createElement('${tag}')`);
      const el = createMockIframe();
      mockElements.push(el);
      return el;
    },
  };
  (globalThis as unknown as { window?: unknown }).window = {
    nativesAPI: {
      bridge: {
        generateToken: async () => `token-${++tokenSequence}`,
        getHttpPort: async () => 4321,
      },
    },
    addEventListener: (type: string, listener: (event: MessageEvent) => unknown) => {
      if (type === 'message') messageListeners.add(listener);
    },
    removeEventListener: (type: string, listener: (event: MessageEvent) => unknown) => {
      if (type === 'message') messageListeners.delete(listener);
    },
  };
  return {
    mockElements,
    dispatchMessage: async (event: MessageEvent) => {
      await Promise.all([...messageListeners].map((listener) => listener(event)));
    },
  };
}

function teardownMocks() {
  delete (globalThis as unknown as { document?: unknown }).document;
  delete (globalThis as unknown as { window?: unknown }).window;
}

describe('IframeManager', () => {
  let mgr: IframeManager;

  beforeEach(() => {
    setupMocks();
    mgr = new IframeManager();
  });

  afterEach(() => {
    mgr.destroyAll();
    teardownMocks();
  });

  describe('createIframe', () => {
    it('should create an iframe and track it', () => {
      const el = mgr.createIframe('mod1', 'http://example.com');
      assert.ok(el);
      assert.deepEqual(mgr.getAllModuleIds(), ['mod1']);
      const inst = mgr.getInstance('mod1');
      assert.ok(inst);
      assert.equal(inst.moduleId, 'mod1');
      assert.equal(inst.state, 'loading');
      assert.equal(inst.element, el);
    });

    it('should set sandbox and src attributes', () => {
      const el = mgr.createIframe('mod1', 'http://example.com');
      assert.equal((el as unknown as MockIframeElement).getAttribute('sandbox'), 'allow-scripts allow-forms');
      assert.equal((el as unknown as MockIframeElement).getAttribute('src'), 'http://example.com');
    });

    it('should replace existing iframe when creating duplicate', () => {
      const el1 = mgr.createIframe('mod1', 'http://first.com');
      const el2 = mgr.createIframe('mod1', 'http://second.com');
      assert.ok(el2);
      assert.deepEqual(mgr.getAllModuleIds(), ['mod1']);
      assert.equal(mgr.getInstance('mod1')?.element, el2);
      assert.ok((el1 as unknown as MockIframeElement).removed);
    });

    it('grants a token only after a source-verified request and targets the opaque frame with *', async () => {
      const mocks = setupMocks();
      const el = mgr.createIframe('mod1', 'http://example.com') as unknown as MockIframeElement;

      el.triggerLoad();
      await Promise.resolve();
      assert.equal(el.messages.length, 0, 'onload must not proactively grant a token');

      await mocks.dispatchMessage({ source: {}, data: { type: 'token-request' } } as MessageEvent);
      assert.equal(el.messages.length, 0, 'a different window cannot request a token');

      await mocks.dispatchMessage({
        source: el.contentWindow,
        data: { type: 'token-request' },
      } as unknown as MessageEvent);
      assert.equal(el.messages.length, 1);
      assert.equal(el.messages[0]?.targetOrigin, '*');
      assert.deepEqual(el.messages[0]?.data, {
        type: 'token-granted',
        token: 'token-1',
        moduleId: 'mod1',
        namespace: 'custom_module_data_mod1',
      });
    });

    it('rejects stale sources and accepts only lifecycle messages with the current token', async () => {
      const mocks = setupMocks();
      const oldEl = mgr.createIframe('mod1', 'http://first.com') as unknown as MockIframeElement;
      await mocks.dispatchMessage({
        source: oldEl.contentWindow,
        data: { type: 'token-request' },
      } as unknown as MessageEvent);

      const newEl = mgr.createIframe('mod1', 'http://second.com') as unknown as MockIframeElement;
      await mocks.dispatchMessage({
        source: oldEl.contentWindow,
        data: { type: 'token-request' },
      } as unknown as MessageEvent);
      assert.equal(newEl.messages.length, 0, 'destroyed iframe source must be rejected');

      await mocks.dispatchMessage({
        source: newEl.contentWindow,
        data: { type: 'token-request' },
      } as unknown as MessageEvent);
      const instance = mgr.getInstance('mod1');
      assert.equal(instance?.sessionToken, 'token-2');
      assert.ok(instance);
      instance.lastAccessed = 0;

      await mocks.dispatchMessage({
        source: newEl.contentWindow,
        data: { type: 'lifecycle:ready', moduleId: 'mod1', token: 'token-1' },
      } as unknown as MessageEvent);
      assert.equal(instance.lastAccessed, 0, 'old token must be rejected');

      await mocks.dispatchMessage({
        source: newEl.contentWindow,
        data: { type: 'lifecycle:ready', moduleId: 'mod1', token: 'token-2' },
      } as unknown as MessageEvent);
      assert.ok(instance.lastAccessed > 0, 'current source and token must succeed');
    });
  });

  describe('showIframe / hideIframe', () => {
    it('should transition state on show and hide', () => {
      mgr.createIframe('mod1', 'http://example.com');

      // Initially loading
      assert.equal(mgr.getInstance('mod1')?.state, 'loading');

      // showIframe transitions to hot
      mgr.showIframe('mod1');
      assert.equal(mgr.getInstance('mod1')?.state, 'hot');
      assert.equal(mgr.getActiveCount(), 1);

      // hideIframe transitions to warm (background)
      mgr.hideIframe('mod1');
      assert.equal(mgr.getInstance('mod1')?.state, 'warm');
      assert.equal(mgr.getBackgroundCount(), 1);
      assert.equal(mgr.getActiveCount(), 0);
    });

    it('should return null for nonexistent module', () => {
      assert.equal(mgr.showIframe('nonexistent'), null);
    });
  });

  describe('destroyIframe', () => {
    it('should remove the instance and clean up', () => {
      mgr.createIframe('mod1', 'http://example.com');
      mgr.onHeartbeatTimeout('mod1', () => {});
      mgr.onCrash('mod1', () => {});
      mgr.startHeartbeat('mod1', 1000);

      mgr.destroyIframe('mod1');
      assert.equal(mgr.getInstance('mod1'), undefined);
      assert.deepEqual(mgr.getAllModuleIds(), []);
    });

    it('should clear the onload handler to prevent leaks', () => {
      mgr.createIframe('mod1', 'http://example.com');
      const inst = mgr.getInstance('mod1');
      assert.ok(inst?.element);
      mgr.destroyIframe('mod1');
      // After destroy, the element's onload should be nullified
      assert.equal((inst.element as unknown as MockIframeElement).onload, null);
    });

    it('should not throw when destroying nonexistent iframe', () => {
      mgr.destroyIframe('nonexistent');
    });
  });

  describe('destroyAll', () => {
    it('should remove all instances', () => {
      mgr.createIframe('mod1', 'http://example.com');
      mgr.createIframe('mod2', 'http://example.com');
      mgr.createIframe('mod3', 'http://example.com');
      assert.equal(mgr.getAllModuleIds().length, 3);

      mgr.destroyAll();
      assert.equal(mgr.getAllModuleIds().length, 0);
      assert.equal(mgr.getActiveCount(), 0);
      assert.equal(mgr.getBackgroundCount(), 0);
    });
  });

  describe('LRU enforcement', () => {
    it('should evict oldest background iframes when exceeding limit', () => {
      for (let i = 1; i <= 7; i++) {
        mgr.createIframe(`mod${i}`, 'http://example.com');
      }

      // Hide all so they become background
      for (let i = 1; i <= 7; i++) {
        mgr.hideIframe(`mod${i}`);
      }

      // Show mod1 - this makes it active, leaving 6 bg
      // enforceLRU runs: 6 bg > MAX_BACKGROUND(5), evicts 1
      mgr.showIframe('mod1');

      const remaining = mgr.getAllModuleIds();
      assert.ok(remaining.length <= 6, `Expected at most 6 remaining, got ${remaining.length}`);
      assert.ok(!remaining.includes('mod2') || !remaining.includes('mod3'), 'One of the background iframes should have been evicted');
    });
  });

  describe('getActiveCount / getBackgroundCount', () => {
    it('should count correctly', () => {
      mgr.createIframe('mod1', 'http://example.com');
      mgr.createIframe('mod2', 'http://example.com');

      // Initially both are 'loading' (not active, not background)
      assert.equal(mgr.getActiveCount(), 0);
      assert.equal(mgr.getBackgroundCount(), 0);

      // showIframe sets state to 'active'
      mgr.showIframe('mod1');
      assert.equal(mgr.getActiveCount(), 1);

      // hideIframe sets state to 'background'
      mgr.hideIframe('mod1');
      assert.equal(mgr.getActiveCount(), 0);
      assert.equal(mgr.getBackgroundCount(), 1);

      // showIframe again
      mgr.showIframe('mod1');
      assert.equal(mgr.getActiveCount(), 1);
      assert.equal(mgr.getBackgroundCount(), 0);
    });
  });

  describe('isManagedMessageSource', () => {
    it('accepts the contentWindow of a managed module iframe', () => {
      mgr.createIframe('mod1', 'http://example.com');
      const inst = mgr.getInstance('mod1');
      assert.ok(inst?.element);
      const iframeWin = (inst.element as unknown as MockIframeElement).contentWindow;
      assert.equal(mgr.isManagedMessageSource(iframeWin), true);
    });

    it('rejects a window that is not a managed module iframe', () => {
      mgr.createIframe('mod1', 'http://example.com');
      assert.equal(mgr.isManagedMessageSource({}), false);
      assert.equal(mgr.isManagedMessageSource(null), false);
    });

    it('rejects contentWindows of destroyed iframes', () => {
      mgr.createIframe('mod1', 'http://example.com');
      const inst = mgr.getInstance('mod1');
      assert.ok(inst?.element);
      const iframeWin = (inst.element as unknown as MockIframeElement).contentWindow;
      assert.equal(mgr.isManagedMessageSource(iframeWin), true);
      mgr.destroyIframe('mod1');
      assert.equal(mgr.isManagedMessageSource(iframeWin), false);
    });

    it('returns false when no iframes are managed', () => {
      assert.equal(mgr.isManagedMessageSource({}), false);
    });
  });

  describe('singleton', () => {
    it('should return the same instance from getIframeManager', async () => {
      const { getIframeManager } = await import('./iframe-manager');
      const a = getIframeManager();
      const b = getIframeManager();
      assert.equal(a, b);
    });
  });
});
