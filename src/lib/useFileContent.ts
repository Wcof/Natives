'use client';

import { useReducer, useEffect } from 'react';

interface State {
  content: string | null;
  loading: boolean;
  error: string | null;
  /** Backend flagged the read as truncated (content is partial). */
  truncated: boolean;
  /** Encoding reported by the backend, e.g. 'utf8' | 'base64'. */
  encoding: string | null;
}

type Action =
  | { type: 'reset' }
  | { type: 'loading' }
  | { type: 'success'; content: string; truncated: boolean; encoding: string | null }
  | { type: 'error'; error: string };

const initialState: State = { content: null, loading: false, error: null, truncated: false, encoding: null };

function reducer(state: State, action: Action): State {
  switch (action.type) {
    case 'reset': return { content: null, loading: false, error: null, truncated: false, encoding: null };
    case 'loading': return { content: null, loading: true, error: null, truncated: false, encoding: null };
    case 'success': return { content: action.content, loading: false, error: null, truncated: action.truncated, encoding: action.encoding };
    case 'error': return { content: null, loading: false, error: action.error, truncated: false, encoding: null };
  }
}

export function useFileContent(path: string | null) {
  const [state, dispatch] = useReducer(reducer, initialState);

  useEffect(() => {
    if (!path) {
      dispatch({ type: 'reset' });
      return;
    }

    let cancelled = false;
    dispatch({ type: 'loading' });

    (async () => {
      try {
        const result = await window.nativesAPI?.fs?.readFile?.(path);
        if (cancelled) return;
        if (result !== undefined && result !== null) {
          const asRecord = typeof result === 'object' ? (result as Record<string, unknown>) : null;
          const text = typeof result === 'string'
            ? result
            : asRecord && asRecord.content !== undefined
              ? String(asRecord.content)
              : String(result);
          const truncated = asRecord ? Boolean(asRecord.truncated) : false;
          const encoding = asRecord && typeof asRecord.encoding === 'string' ? asRecord.encoding : null;
          dispatch({ type: 'success', content: text, truncated, encoding });
        } else {
          dispatch({ type: 'error', error: 'File not available' });
        }
      } catch (err) {
        if (!cancelled) {
          dispatch({ type: 'error', error: (err as Error).message });
        }
      }
    })();

    return () => { cancelled = true; };
  }, [path]);

  return state;
}
