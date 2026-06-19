'use client';

import { useState, useEffect } from 'react';

/**
 * Shared hook for fetching file content via Tauri IPC.
 * Falls back to HTTP server if direct IPC is unavailable.
 *
 * @param path - File path to fetch
 * @returns { content, loading, error }
 */
export function useFileContent(path: string | null, httpPort = 3456) {
  const [content, setContent] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!path) {
      setContent(null);
      setLoading(false);
      setError(null);
      return;
    }

    let cancelled = false;
    setLoading(true);
    setError(null);

    (async () => {
      try {
        // Try Tauri IPC first
        const text = await window.nativesAPI?.fs?.readFile?.(path) as string | undefined;
        if (cancelled) return;
        if (text !== undefined) {
          setContent(text);
          setLoading(false);
          return;
        }
        // Fallback: HTTP bridge
        const res = await fetch(`http://localhost:${httpPort}/api/fs/raw?path=${encodeURIComponent(path)}`);
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        const textFromHttp = await res.text();
        if (!cancelled) {
          setContent(textFromHttp);
          setLoading(false);
        }
      } catch (err) {
        if (!cancelled) {
          setError((err as Error).message);
          setContent(null);
          setLoading(false);
        }
      }
    })();

    return () => { cancelled = true; };
  }, [path, httpPort]);

  return { content, loading, error };
}
