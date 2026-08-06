//! Creative window controller (batch 10 CR-1003).
//!
//! Owns the child-WebView window state (which app is open, at what URL) and the
//! open/close actions. Extracted from WorkshopPage so the browser surface has
//! one real controller instead of inline state + handlers.

import { useCallback, useState, type RefObject } from 'react';
import { useLocale, t } from '@/i18n';
import { classifyError } from '@/lib/error-classifier';
import type {
  CreativeAppBrowserBounds,
  CreativeAppSummary,
} from '@/lib/tauri-adapter';

export function useCreativeWindows(hostRef: RefObject<HTMLDivElement | null>) {
  const locale = useLocale();
  const [browserApp, setBrowserApp] = useState<CreativeAppSummary | null>(null);
  const [browserUrl, setBrowserUrl] = useState('');

  /** Open the child WebView for a non-internal app. Rolls back on Host failure. */
  const openExternal = useCallback(
    async (app: CreativeAppSummary, onToast: (message: string) => void) => {
      try {
        const target = await window.nativesAPI?.creativeApp?.getOpenTarget?.(app.id);
        if (!target || target.kind !== 'local_url') {
          onToast(t(locale, 'workshop.stateStartFailed'));
          return;
        }
        const el = hostRef.current;
        setBrowserApp(app);
        setBrowserUrl(target.url);
        requestAnimationFrame(() => {
          const host = hostRef.current ?? el;
          const r = host?.getBoundingClientRect();
          const bounds: CreativeAppBrowserBounds = r
            ? { x: r.left, y: r.top, width: r.width, height: r.height }
            : { x: 280, y: 80, width: 900, height: 640 };
          const p = window.nativesAPI?.creativeApp?.browserShow?.(app.id, target.url, bounds);
          if (p) {
            // Host failure must roll back the optimistic panel and stay visible
            // (batch 2 CR-203, #31) instead of leaving a dead Browser pane open.
            p.catch((err) => {
              setBrowserApp(null);
              setBrowserUrl('');
              onToast(classifyError(err).userMessage);
            });
          }
        });
      } catch (err) {
        onToast(classifyError(err).userMessage);
      }
    },
    [hostRef, locale],
  );

  const closeBrowser = useCallback(async () => {
    if (browserApp) {
      await window.nativesAPI?.creativeApp?.browserClose?.(browserApp.id);
    }
    setBrowserApp(null);
    setBrowserUrl('');
  }, [browserApp]);

  return { browserApp, browserUrl, setBrowserApp, openExternal, closeBrowser };
}
