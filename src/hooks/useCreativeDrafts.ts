'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import type { CreativeDraft } from '@/lib/tauri-adapter';

/**
 * Draft list + lifecycle actions for the creator workbench.
 *
 * Kept separate from `useCreativeAppCatalog` because drafts and published apps
 * answer different questions: a draft is work in progress with no module
 * identity, an app is something that already exists in the sidebar. Merging the
 * two would force every consumer to re-split them.
 */
export function useCreativeDrafts(options: { enabled?: boolean } = {}) {
  const { enabled = true } = options;
  const [drafts, setDrafts] = useState<CreativeDraft[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Guards against a slow reload landing after a newer one and resurrecting
  // drafts the user already published or deleted.
  const requestSeq = useRef(0);

  const reload = useCallback(async () => {
    if (!enabled) return;
    const api = typeof window !== 'undefined' ? window.nativesAPI : undefined;
    if (!api?.creativeDraft?.list) {
      setDrafts([]);
      return;
    }
    const seq = ++requestSeq.current;
    setLoading(true);
    try {
      const list = await api.creativeDraft.list();
      if (seq === requestSeq.current) {
        setDrafts(list);
        setError(null);
      }
    } catch (err) {
      if (seq === requestSeq.current) {
        setError(err instanceof Error ? err.message : String(err));
      }
    } finally {
      if (seq === requestSeq.current) setLoading(false);
    }
  }, [enabled]);

  useEffect(() => {
    void reload();
  }, [reload]);

  // The host broadcasts on the `creative-draft` channel after every create,
  // revision, rollback, publish and delete, so the list follows state changes
  // instead of polling for them.
  useEffect(() => {
    if (!enabled) return;
    const api = typeof window !== 'undefined' ? window.nativesAPI : undefined;
    if (!api?.onDbStateChanged) return;
    const unsub = api.onDbStateChanged((_event, channel) => {
      if (channel !== 'creative-draft') return;
      void reload();
    });
    return () => {
      unsub?.();
    };
  }, [enabled, reload]);

  return { drafts, loading, error, reload };
}
