//! Browser-window controller (batch 10 CR-1003).
//!
//! Owns the child-WebView host element lifecycle: reports the host div's
//! bounds to the Host so the child WebView is placed/sized correctly, and
//! closes the child WebView when the browser closes or WorkshopPage unmounts.
//! Extracted from WorkshopPage so the browser seam has one real controller
//! instead of inline effects.

import { useEffect, useRef } from 'react';
import type { CreativeAppBrowserBounds, CreativeAppSummary } from '@/lib/tauri-adapter';

/** App id kept in a ref so the unmount cleanup can close the right WebView. */
export function useBrowserWindow(app: CreativeAppSummary | null) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const appIdRef = useRef<string | null>(null);

  // Keep the app id available to the unmount cleanup (which has empty deps).
  useEffect(() => {
    appIdRef.current = app?.id ?? null;
  }, [app]);

  // Report the host element bounds to the Host so the child WebView is
  // placed and sized to match the visible panel.
  useEffect(() => {
    if (!app) return;
    const el = hostRef.current;
    if (!el) return;
    const report = () => {
      const r = el.getBoundingClientRect();
      const bounds: CreativeAppBrowserBounds = {
        x: r.left,
        y: r.top,
        width: r.width,
        height: r.height,
      };
      void window.nativesAPI?.creativeApp?.browserSetBounds?.(app.id, bounds);
    };
    report();
    const ro = new ResizeObserver(report);
    ro.observe(el);
    window.addEventListener('resize', report);
    return () => {
      ro.disconnect();
      window.removeEventListener('resize', report);
    };
  }, [app]);

  // Close the child WebView on unmount so it cannot outlive the surface.
  useEffect(() => {
    return () => {
      const id = appIdRef.current;
      if (id) {
        void window.nativesAPI?.creativeApp?.browserClose?.(id);
      }
    };
  }, []);

  return hostRef;
}
