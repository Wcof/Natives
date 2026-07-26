'use client';

import { useReducer, useEffect, useCallback, useState } from 'react';
import { fsApi } from '@/lib/files-api';

interface State {
  content: string | null;
  loading: boolean;
  error: string | null;
  /** Backend flagged the read as truncated (content is partial). */
  truncated: boolean;
  /** Encoding reported by the backend, e.g. 'utf8' | 'base64'. */
  encoding: string | null;
  /** 磁盘 mtime（ms）；作为保存时乐观锁（expectedMtime）的基线 */
  mtime: number | null;
}

type Action =
  | { type: 'reset' }
  | { type: 'loading' }
  | { type: 'success'; content: string; truncated: boolean; encoding: string | null; mtime: number | null }
  | { type: 'error'; error: string };

const initialState: State = { content: null, loading: false, error: null, truncated: false, encoding: null, mtime: null };

function reducer(state: State, action: Action): State {
  switch (action.type) {
    case 'reset': return { ...initialState };
    case 'loading': return { ...initialState, loading: true };
    case 'success': return { content: action.content, loading: false, error: null, truncated: action.truncated, encoding: action.encoding, mtime: action.mtime };
    case 'error': return { ...initialState, error: action.error };
  }
}

export function useFileContent(path: string | null) {
  const [state, dispatch] = useReducer(reducer, initialState);
  /** bump 强制重读磁盘（外部变更热重载用） */
  const [reloadTick, setReloadTick] = useState(0);

  useEffect(() => {
    if (!path) {
      dispatch({ type: 'reset' });
      return;
    }

    let cancelled = false;
    dispatch({ type: 'loading' });

    (async () => {
      try {
        const result = await fsApi().readFile(path);
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
          const mtime = asRecord && typeof asRecord.mtime === 'number' ? asRecord.mtime : null;
          dispatch({ type: 'success', content: text, truncated, encoding, mtime });
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
  }, [path, reloadTick]);

  const reload = useCallback(() => setReloadTick((n) => n + 1), []);

  return { ...state, reload };
}
