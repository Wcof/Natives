import { describe, it, beforeEach, afterEach } from 'node:test';
import assert from 'node:assert/strict';
import {
  createAppearanceCoordinator,
  ThemeCoordinatorError,
  parseThemeId,
  resolveTheme,
  getAppearanceCoordinator,
  DEFAULT_APPEARANCE_THEME,
  type AppearanceSnapshot,
  type ThemeHost,
} from './coordinator';

// ── 最小的 DOM double（document.documentElement；getAttribute/setAttribute） ──
function setupDocument() {
  const attrs = new Map<string, string>();
  const props: Array<[string, string]> = [];
  const root = {
    getAttribute: (name: string) => (attrs.has(name) ? attrs.get(name) : null),
    setAttribute: (name: string, value: string) => attrs.set(name, value),
    style: { setProperty: (key: string, value: string) => props.push([key, value]) },
    classList: { add: () => {}, remove: () => {} },
  };
  (globalThis as unknown as { document?: unknown }).document = { documentElement: root };
  return { root, attrs, props };
}

function teardownDocument() {
  delete (globalThis as unknown as { document?: unknown }).document;
  delete (globalThis as unknown as { navigator?: unknown }).navigator;
  delete (globalThis as unknown as { window?: unknown }).window;
}

function makeHost(overrides: Partial<ThemeHost> = {}): ThemeHost & {
  setThemeLog: string[];
  themeChangedListeners: Array<(theme: AppearanceSnapshot['theme']) => void>;
} {
  const setThemeLog: string[] = [];
  const themeChangedListeners: Array<(theme: AppearanceSnapshot['theme']) => void> = [];
  let stored = 'dark';
  const host: ThemeHost = {
    getTheme: async () => stored,
    setTheme: async (theme) => {
      setThemeLog.push(theme);
      stored = theme;
      themeChangedListeners.forEach((cb) => cb(theme as AppearanceSnapshot['theme']));
    },
    themeReady: () => {},
    onThemeChanged: (cb) => {
      themeChangedListeners.push(cb);
      return () => {
        const i = themeChangedListeners.indexOf(cb);
        if (i >= 0) themeChangedListeners.splice(i, 1);
      };
    },
    ...overrides,
  };
  return { ...host, setThemeLog, themeChangedListeners };
}

describe('appearance/coordinator', () => {
  beforeEach(() => {
    setupDocument();
  });
  afterEach(() => {
    teardownDocument();
  });

  describe('parseThemeId (Zod 防线)', () => {
    it('accepts the canonical vocabulary', () => {
      assert.equal(parseThemeId('dark'), 'dark');
      assert.equal(parseThemeId('light'), 'light');
    });
    it('normalizes legacy aliases to canonical', () => {
      assert.equal(parseThemeId('terminal-volt'), 'dark');
      assert.equal(parseThemeId('frosted-jasmine'), 'light');
    });
    it('falls back to dark for unknown/nullish input', () => {
      assert.equal(parseThemeId('neon'), 'dark');
      assert.equal(parseThemeId(undefined), 'dark');
      assert.equal(parseThemeId(null), 'dark');
      assert.equal(parseThemeId(''), 'dark');
    });
  });

  describe('resolveTheme (normalize alias)', () => {
    it('normalizes legacy names like theme-engine', () => {
      assert.equal(resolveTheme('frosted-jasmine'), 'light');
      assert.equal(resolveTheme('dark'), 'dark');
      assert.equal(resolveTheme('unknown'), 'dark');
    });
  });

  describe('bootstrap', () => {
    it('loads host theme, applies DOM + ready + snapshot revision 1', async () => {
      const host = makeHost({ getTheme: async () => 'light' });
      let readyCalled = false;
      const c = createAppearanceCoordinator(host, { signalReady: () => { readyCalled = true; } });

      const snap = await c.bootstrap();
      assert.deepEqual(snap, { theme: 'light', revision: 1 });
      assert.equal(c.getSnapshot().theme, 'light');
      assert.equal(readyCalled, true, 'ready signal must be sent after DOM apply');
      const root = (globalThis as unknown as { document: { documentElement: { getAttribute: (n: string) => string | null } } }).document.documentElement;
      assert.equal(root.getAttribute('data-theme'), 'light');
    });

    it('defaults to dark when host returns nothing', async () => {
      const host = makeHost({ getTheme: async () => '' });
      const c = createAppearanceCoordinator(host);
      const snap = await c.bootstrap();
      assert.equal(snap.theme, 'dark');
      assert.equal((globalThis as unknown as { document: { documentElement: { getAttribute: (n: string) => string | null } } }).document.documentElement.getAttribute('data-theme'), 'dark');
    });

    it('throws structured ThemeCoordinatorError on host failure and keeps dark fallback', async () => {
      const host = makeHost({
        getTheme: async () => {
          throw new Error('connection refused');
        },
      });
      const c = createAppearanceCoordinator(host);
      await assert.rejects(() => c.bootstrap(), (err: unknown) => {
        assert.ok(err instanceof ThemeCoordinatorError);
        const payload = (err as ThemeCoordinatorError).payload;
        assert.equal(payload.kind, 'load');
        assert.equal(payload.retryable, true);
        assert.equal(payload.current.theme, 'dark');
        return true;
      });
      // 受控 dark fallback：DOM 仍是默认 dark（bootstrap 未改动）。
      assert.equal(c.getSnapshot().theme, 'dark');
    });

    it('classifies DB/network-ish errors into structured categories', async () => {
      const host = makeHost({
        getTheme: async () => {
          throw new Error('no such table: settings');
        },
      });
      const c = createAppearanceCoordinator(host);
      await assert.rejects(() => c.bootstrap(), (err: unknown) => {
        const payload = (err as ThemeCoordinatorError).payload;
        assert.equal(payload.classified.category, 'DB_ERROR');
        return true;
      });
    });
  });

  describe('select', () => {
    it('persists via host, then updates DOM + notifies subscribers', async () => {
      const host = makeHost({ getTheme: async () => 'dark' });
      const c = createAppearanceCoordinator(host);
      await c.bootstrap();

      const seen: AppearanceSnapshot[] = [];
      const unsub = c.subscribe((snap) => seen.push(snap));
      const snap = await c.select('light');

      assert.equal(snap.theme, 'light');
      assert.deepEqual(host.setThemeLog, ['light']);
      assert.equal((globalThis as unknown as { document: { documentElement: { getAttribute: (n: string) => string | null } } }).document.documentElement.getAttribute('data-theme'), 'light');
      assert.equal(seen.length, 1);
      assert.equal(seen[0]?.theme, 'light');
      unsub();
    });

    it('is a no-op when selecting the current theme (no revision churn)', async () => {
      const host = makeHost({ getTheme: async () => 'dark' });
      const c = createAppearanceCoordinator(host);
      await c.bootstrap();
      const before = c.getSnapshot().revision;
      const snap = await c.select('dark');
      assert.equal(snap.revision, before, 'selecting the same theme must not bump revision');
      assert.deepEqual(host.setThemeLog, []);
    });

    it('keeps the old theme and throws structured error when persistence fails', async () => {
      const host = makeHost({
        getTheme: async () => 'dark',
        setTheme: async () => {
          throw new Error('SQLITE_FULL: database or disk is full');
        },
      });
      const c = createAppearanceCoordinator(host);
      await c.bootstrap();

      await assert.rejects(() => c.select('light'), (err: unknown) => {
        assert.ok(err instanceof ThemeCoordinatorError);
        const payload = (err as ThemeCoordinatorError).payload;
        assert.equal(payload.kind, 'persist');
        assert.equal(payload.classified.category, 'DB_ERROR');
        assert.equal(payload.current.theme, 'dark', 'old theme preserved after failed persist');
        return true;
      });
      // DOM 与快照保持旧主题。
      assert.equal(c.getSnapshot().theme, 'dark');
      assert.equal((globalThis as unknown as { document: { documentElement: { getAttribute: (n: string) => string | null } } }).document.documentElement.getAttribute('data-theme'), 'dark');
    });

    it('classifies unknown / IPC failures as retryable persist errors', async () => {
      const host = makeHost({
        getTheme: async () => 'dark',
        setTheme: async () => {
          throw new Error('Tauri command failed: set_theme — invoke timeout on channel');
        },
      });
      const c = createAppearanceCoordinator(host);
      await c.bootstrap();
      const err = await c.select('light').then(
        () => null,
        (e: unknown) => e,
      );
      assert.ok(err instanceof ThemeCoordinatorError);
      assert.equal(err.payload.classified.category, 'IPC_TIMEOUT');
      assert.equal(err.payload.retryable, true);
    });
  });

  describe('multi-window broadcast (db-state-changed theme)', () => {
    it('applies an external theme change and notifies subscribers', async () => {
      const host = makeHost({ getTheme: async () => 'dark' });
      const c = createAppearanceCoordinator(host);
      await c.bootstrap();

      const seen: AppearanceSnapshot[] = [];
      const unsub = c.subscribe((snap) => seen.push(snap));
      host.themeChangedListeners.forEach((cb) => cb('light'));

      assert.equal(c.getSnapshot().theme, 'light');
      assert.equal(seen.length, 1);
      assert.equal(seen[0]?.theme, 'light');
      // 外部变更不改 Host set 面（其它窗口已持久化）。
      assert.deepEqual(host.setThemeLog, []);
      unsub();
    });

    it('ignores broadcasts for the current theme', async () => {
      const host = makeHost({ getTheme: async () => 'dark' });
      const c = createAppearanceCoordinator(host);
      await c.bootstrap();
      const before = c.getSnapshot().revision;
      host.themeChangedListeners.forEach((cb) => cb('dark'));
      assert.equal(c.getSnapshot().revision, before);
    });

    it('unsubscribes stop receiving events', async () => {
      const host = makeHost({ getTheme: async () => 'dark' });
      const c = createAppearanceCoordinator(host);
      await c.bootstrap();
      let count = 0;
      const unsub = c.subscribe(() => { count += 1; });
      unsub();
      host.themeChangedListeners.forEach((cb) => cb('light'));
      assert.equal(count, 0);
    });
  });

  describe('singleton surface factory', () => {
    it('reuses the same coordinator across getAppearanceCoordinator calls', async () => {
      const host = makeHost({ getTheme: async () => 'dark' });
      const a = await getAppearanceCoordinator(host);
      const b = await getAppearanceCoordinator();
      assert.equal(a, b);
    });

    it('exposes the default theme constant', () => {
      assert.equal(DEFAULT_APPEARANCE_THEME, 'dark');
    });
  });
});