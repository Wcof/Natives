'use client';

import { useState, useEffect } from 'react';

/**
 * Shared hook for fetching file content from the local HTTP server.
 * Replaces the repeated fetch(`/api/fs/raw?path=...`) pattern.
 *
 * @param path - File path to fetch
 * @param httpPort - Local HTTP server port (default: 3001)
 * @returns { content, loading, error }
 */
export function useFileContent(path: string | null, httpPort = 3001) {
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

    fetch(`http://localhost:${httpPort}/api/fs/raw?path=${encodeURIComponent(path)}`)
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
  }, [path, httpPort]);

  return { content, loading, error };
}
