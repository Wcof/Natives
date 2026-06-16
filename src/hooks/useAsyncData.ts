import { useState, useCallback, useRef } from 'react';
import { classifyError, type ClassifiedError } from '@/lib/error-classifier';

interface State<T> {
  data: T | null;
  loading: boolean;
  error: ClassifiedError | null;
}

/**
 * 通用异步数据获取 hook —— 三态管理（loading / error / success）
 *
 * 符合 R-E10（MUST）：任何从 Main/IPC 取数的 UI 必须处理 loading/error/success 三态。
 * 符合 R-E12（MUST）：所有错误经 classifyError 后展示。
 *
 * @param fetcher 异步取数函数
 * @param deps 依赖数组，变化时自动 reload
 */
export function useAsyncData<T>(
  fetcher: () => Promise<T>,
  deps: React.DependencyList = [],
) {
  const [state, setState] = useState<State<T>>({ data: null, loading: true, error: null });
  const mounted = useRef(true);

  // 组件卸载时标记已卸载，防止 unmounted 后 setState
  useState(() => {
    mounted.current = true;
    return () => { mounted.current = false; };
  });

  const reload = useCallback(async () => {
    setState((s) => ({ ...s, loading: true, error: null }));
    try {
      const data = await fetcher();
      if (mounted.current) setState({ data, loading: false, error: null });
    } catch (err) {
      if (mounted.current) setState({ data: null, loading: false, error: classifyError(err) });
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);

  const setError = useCallback((e: unknown) => {
    setState((s) => ({ ...s, error: classifyError(e), loading: false }));
  }, []);

  return { ...state, reload, setError };
}
