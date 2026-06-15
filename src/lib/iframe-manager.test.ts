import { describe, it, beforeEach, afterEach } from 'node:test';
import assert from 'node:assert/strict';
import { IframeManager } from './iframe-manager';

function createMockIframe() {
  const attrs: Record<string, string> = {};
  const style: Record<string, string> = {};
  let _onload: (() => void) | null = null;
  let removed = false;
  return {
    setAttribute: (name: string, value: string) => { attrs[name] = value; },
    getAttribute: (name: string) => attrs[name],
    get style() { return style; },
    get onload() { return _onload; },
    set onload(fn: (() => void) | null) { _onload = fn; },
    remove: () => { removed = true; },
    get removed() { return removed; },
    triggerLoad() { _onload?.(); },
  };
}

function setupMocks() {
  const mockElements: ReturnType<typeof createMockIframe>[] = [];
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  (globalThis as any).document = {
    createElement: (tag: string) => {
      if (tag !== 'iframe') throw new Error(`Unexpected createElement('${tag}')`);
      const el = createMockIframe();
      mockElements.push(el);
      return el;
    },
  };
  return mockElements;
}

function teardownMocks() {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  delete (globalThis as any).document;
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
      assert.equal((el as any).getAttribute('sandbox'), 'allow-scripts allow-forms');
      assert.equal((el as any).getAttribute('src'), 'http://example.com');
    });

    it('should replace existing iframe when creating duplicate', () => {
      const el1 = mgr.createIframe('mod1', 'http://first.com');
      const el2 = mgr.createIframe('mod1', 'http://second.com');
      assert.ok(el2);
      assert.deepEqual(mgr.getAllModuleIds(), ['mod1']);
      assert.equal(mgr.getInstance('mod1')?.element, el2);
      assert.ok((el1 as any).removed);
    });
  });

  describe('showIframe / hideIframe', () => {
    it('should transition state on show and hide', () => {
      mgr.createIframe('mod1', 'http://example.com');

      // Initially loading
      assert.equal(mgr.getInstance('mod1')?.state, 'loading');

      // showIframe transitions to active
      mgr.showIframe('mod1');
      assert.equal(mgr.getInstance('mod1')?.state, 'active');
      assert.equal(mgr.getActiveCount(), 1);

      // hideIframe transitions to background
      mgr.hideIframe('mod1');
      assert.equal(mgr.getInstance('mod1')?.state, 'background');
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
      assert.equal((inst.element as any).onload, null);
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

  describe('singleton', () => {
    it('should return the same instance from getIframeManager', async () => {
      const { getIframeManager } = await import('./iframe-manager');
      const a = getIframeManager();
      const b = getIframeManager();
      assert.equal(a, b);
    });
  });
});
