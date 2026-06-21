'use client';

import { useReducer, useEffect } from 'react';

interface State {
  content: string | null;
  loading: boolean;
  error: string | null;
}

type Action =
  | { type: 'reset' }
  | { type: 'loading' }
  | { type: 'success'; content: string }
  | { type: 'error'; error: string };

const initialState: State = { content: null, loading: false, error: null };

function reducer(state: State, action: Action): State {
  switch (action.type) {
    case 'reset': return { content: null, loading: false, error: null };
    case 'loading': return { content: null, loading: true, error: null };
    case 'success': return { content: action.content, loading: false, error: null };
    case 'error': return { content: null, loading: false, error: action.error };
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
          const text = typeof result === 'string'
            ? result
            : (result as Record<string, unknown>).content !== undefined
              ? String((result as Record<string, unknown>).content)
              : String(result);
          dispatch({ type: 'success', content: text });
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
