'use client';

/**
 * useFsWatch — 订阅一个目录的递归文件变更（Rust notify → `fs-watch-change` 事件）。
 *
 * 只负责「监听编排 + 作用域过滤」：目录切换时增量开/关后端 watcher，
 * 事件不在当前目录下的直接丢弃。噪声过滤、防抖刷新等策略留给调用方
 * （见 fs-change-filter.ts），保持本 hook 无策略。
 *
 * 非 Tauri 环境（无 nativesAPI.fsWatch）静默降级为 no-op。
 */

import { useEffect, useRef } from 'react';
import { fsWatchApiOrNull } from '@/lib/files-api';

export interface FsChangeEvent {
  path: string;
  kind: string; // create | modify | rename | remove
}

export function useFsWatch(dir: string | null, onEvent: (event: FsChangeEvent) => void): void {
  // 回调走 ref：事件处理逻辑变化不应导致 watcher 重启
  const onEventRef = useRef(onEvent);
  onEventRef.current = onEvent;

  useEffect(() => {
    if (!dir) return;
    const api = fsWatchApiOrNull();
    if (!api) return;

    let disposed = false;
    const prefix = dir.endsWith('/') ? dir : dir + '/';

    void api.start(dir).catch(() => {
      /* 目录不可监听（权限/不存在）时静默：浏览功能不受影响 */
    });
    const off = api.onChange((event) => {
      if (disposed) return;
      if (event.path !== dir && !event.path.startsWith(prefix)) return;
      onEventRef.current(event);
    });

    return () => {
      disposed = true;
      off();
      void api.stop(dir).catch(() => {});
    };
  }, [dir]);
}
