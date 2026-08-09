//! Creative Dock controller (T10).
//!
//! Projects the T07 window snapshot (per running app, `windowList`) into dock
//! tabs, and drives the real Window API for open/focus/minimize/close. The
//! active key tracks the window this session last acted on — honest local
//! projection, never a fabricated window row.

import { useCallback, useEffect, useMemo, useState } from 'react';
import { useLocale, t } from '@/i18n';
import { classifyError } from '@/lib/error-classifier';
import type { CreativeAppSummary, CreativeAppWindow } from '@/lib/tauri-adapter';
// W4: shared type from lib — hooks never depend on component internals.
import type { CreativeDockTab } from '@/lib/creative-dock-types';

const OPEN_BOUNDS = { x: 280, y: 80, width: 900, height: 640 };

/** Which app states appear in the dock (running or starting). */
function isDockable(app: CreativeAppSummary): boolean {
  return app.state === 'running' || app.state === 'starting';
}

export function useCreativeDock(apps: CreativeAppSummary[], onToast: (message: string) => void) {
  const locale = useLocale();
  const [windowsByApp, setWindowsByApp] = useState<Record<string, CreativeAppWindow[]>>({});
  const [activeKey, setActiveKey] = useState<string | null>(null);

  /** Snapshot: the latest non-closed windows for every running app. */
  const loadSnapshot = useCallback(async () => {
    const running = apps.filter(isDockable);
    const next: Record<string, CreativeAppWindow[]> = {};
    for (const app of running) {
      const appId = app.applicationId;
      if (!appId) continue;
      try {
        const wins = await window.nativesAPI?.creativeApp?.windowList?.(appId);
        next[appId] = Array.isArray(wins) ? wins.filter((w) => w.state !== 'closed') : [];
      } catch {
        next[appId] = [];
      }
    }
    setWindowsByApp(next);
  }, [apps]);

  useEffect(() => {
    void loadSnapshot();
  }, [loadSnapshot]);

  /** Open a real window for an app that has none yet. */
  const openWindow = useCallback(
    async (app: CreativeAppSummary) => {
      try {
        const api = window.nativesAPI?.creativeApp;
        const appId = app.applicationId;
        if (!api?.windowOpen || !api?.getOpenTarget || !api?.surfaceList || !appId) {
          onToast(t(locale, 'workshop.stateStartFailed'));
          return;
        }
        const target = await api.getOpenTarget(app.id);
        if (!target || target.kind !== 'local_url') {
          onToast(t(locale, 'workshop.stateStartFailed'));
          return;
        }
        const surfaces = await api.surfaceList(appId);
        const surface = (surfaces ?? []).find((s) => s.kind === 'main') ?? (surfaces ?? [])[0];
        if (!surface) {
          onToast(t(locale, 'workshop.stateStartFailed'));
          return;
        }
        const win = await api.windowOpen(appId, surface.id, target.url, OPEN_BOUNDS);
        setActiveKey(`win:${win.id}`);
        void loadSnapshot();
      } catch (err) {
        onToast(classifyError(err).userMessage);
      }
    },
    [locale, loadSnapshot, onToast],
  );

  /** Click a tab: restore/focus its window, or open one when none exists. */
  const select = useCallback(
    (tab: CreativeDockTab) => {
      if (tab.window) {
        if (tab.state === 'minimized') {
          window.nativesAPI?.creativeApp?.windowRestore?.(tab.window.id).catch((err) =>
            onToast(classifyError(err).userMessage),
          );
        }
        setActiveKey(tab.key);
        void loadSnapshot();
      } else {
        void openWindow(tab.app);
      }
    },
    [loadSnapshot, onToast, openWindow],
  );

  const minimize = useCallback(
    (tab: CreativeDockTab) => {
      if (!tab.window) return;
      window.nativesAPI?.creativeApp?.windowMinimize?.(tab.window.id)
        .catch((err) => onToast(classifyError(err).userMessage))
        .finally(() => {
          void loadSnapshot();
        });
    },
    [loadSnapshot, onToast],
  );

  const close = useCallback(
    (tab: CreativeDockTab) => {
      if (!tab.window) return;
      setActiveKey((prev) => (prev === tab.key ? null : prev));
      window.nativesAPI?.creativeApp?.windowClose?.(tab.window.id)
        .catch((err) => onToast(classifyError(err).userMessage))
        .finally(() => {
          void loadSnapshot();
        });
    },
    [loadSnapshot, onToast],
  );

  const tabs = useMemo<CreativeDockTab[]>(() => {
    const out: CreativeDockTab[] = [];
    for (const app of apps) {
      if (!isDockable(app)) continue;
      const appId = app.applicationId;
      const wins = (appId && windowsByApp[appId]) || [];
      if (wins.length === 0) {
        out.push({ key: `app:${app.id}`, app, window: null, state: 'no-window' });
        continue;
      }
      for (const win of wins) {
        out.push({
          key: `win:${win.id}`,
          app,
          window: win,
          state: win.state === 'minimized' ? 'minimized' : 'open',
        });
      }
    }
    return out;
  }, [apps, windowsByApp]);

  return { tabs, activeKey, select, minimize, close, reloadSnapshot: loadSnapshot };
}
