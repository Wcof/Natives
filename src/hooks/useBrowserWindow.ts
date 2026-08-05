//! Browser-window controller (batch 10 CR-1003).
//!
//! Attaches child-WebView host lifecycle to a caller-provided host div ref:
//! reports the host element bounds to the Host so the child WebView is placed
//! and sized correctly, and closes the child WebView on unmount. Extracted from
//! WorkshopPage so the browser seam has one real controller instead of inline
//! effects. The ref is provided by the caller so the window controller
//! (useCreativeWindows) can share it.

import { useEffect, useRef, type RefObject } from 'react';
import type { CreativeAppSummary } from '@/lib/tauri-adapter';

/** Attach host-bounds reporting + close-on-unmount to a shared host ref. */
export function useBrowserWindow(app: CreativeAppSummary | null, hostRef: RefObject<HTMLDivElement | null>) {
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
      const bounds = {
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
  }, [app, hostRef]);

  // Close the child WebView on unmount so it cannot outlive the surface.
  useEffect(() => {
    return () => {
      const id = appIdRef.current;
      if (id) {
        void window.nativesAPI?.creativeApp?.browserClose?.(id);
      }
    };
  }, []);
}
