'use client';

import { useState, useEffect } from 'react';

/**
 * Shared hook for fetching file content via Next.js API route.
 * Works in both browser dev mode and Electron mode.
 *
 * @param path - File path to fetch
 * @returns { content, loading, error }
 */
export function useFileContent(path: string | null, _httpPort = 3001) {
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

    fetch(`/api/fs/raw?path=${encodeURIComponent(path)}`)
      .then(r => {
        if (!r.ok) throw new Error(`HTTP ${r.status}`);
        return r.text();
      })
      .then(text => {
        if (!cancelled) {
          setContent(text);
          setLoading(false);
        }
      })
      .catch(err => {
        if (!cancelled) {
          setError(err.message);
          setContent(null);
          setLoading(false);
        }
      });

    return () => { cancelled = true; };
  }, [path]);

  return { content, loading, error };
}
