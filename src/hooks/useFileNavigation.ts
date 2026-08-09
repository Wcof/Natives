'use client';

/**
 * useFileNavigation — FileBrowser 导航状态（路径 / 历史 / 最近模式）。
 *
 * 职责边界（F3-04 拆分，ARCH-002）：
 * - currentPath + 前进/后退/上移历史（fanbox 语义：去重、截断 forward 历史）
 * - 「最近修改」/「最近打开」互斥模式切换
 * - 外部导航（Sidebar / CommandPalette / Terminal / Assistant → FileBrowser）
 * - 路径栏解析（resolveAndNavigate：file → 父目录 + soft-select；dir → 打开）
 * - Cmd+[ / Cmd+] 历史快捷键
 *
 * 不持有：entries/loading、选择、文件操作、拖拽、preview selection、toast。
 * 导航发生后通过 resetSelection 通知外层清空选择（保持原行为）。
 */

import { useCallback, useEffect, useRef, useState } from 'react';
import { t, type Locale } from '@/i18n';
import { fsApi, hasNativeFiles } from '@/lib/files-api';
import {
  FILE_EVENTS,
  consumePendingNavigate,
  onFileEvent,
  setPendingSelectFile,
  type NavigateFilesPayload,
} from '@/lib/file-events';
import { shouldApplyHomeFallback } from '@/lib/file-startup-navigation';

export interface UseFileNavigationOptions {
  /** 导航后清空文件选择（由 useFileSelection 提供） */
  resetSelection: () => void;
  showToast: (msg: string) => void;
  locale: Locale;
}

export interface UseFileNavigationResult {
  currentPath: string;
  segments: string[];
  /** 历史 ref 变更后用于驱动 chrome 按钮重渲染的 tick */
  historyTick: number;
  recentMode: boolean;
  recentOpenedMode: boolean;
  canGoBack: boolean;
  canGoForward: boolean;
  canGoUp: boolean;
  toggleRecent: () => void;
  toggleRecentOpened: () => void;
  navigateTo: (path: string) => void;
  goBack: () => void;
  goForward: () => void;
  goUp: () => void;
  resolveAndNavigate: (raw: string) => Promise<void>;
}

export function useFileNavigation({
  resetSelection,
  showToast,
  locale,
}: UseFileNavigationOptions): UseFileNavigationResult {
  const [currentPath, setCurrentPath] = useState('/');
  const navigationIntentRef = useRef(false);
  const [recentMode, setRecentMode] = useState(false);
  /** 「最近打开」视图（读取 LRU），与 recentMode（最近修改，后端扫描）互斥 */
  const [recentOpenedMode, setRecentOpenedMode] = useState(false);

  // Navigation history for back/forward
  const historyRef = useRef<string[]>(['/']);
  const historyIndexRef = useRef(0);
  /** Bumped so canGoBack/canGoForward re-render after ref mutations. */
  const [historyTick, setHistoryTick] = useState(0);

  const navigateTo = useCallback(
    (path: string) => {
      const normalized = path === '' ? '/' : path.replace(/\/+$/, '') || '/';
      // Truncate forward history and append (skip no-op)
      if (historyRef.current[historyIndexRef.current] === normalized) {
        setCurrentPath(normalized);
        setRecentMode(false);
        setRecentOpenedMode(false);
        resetSelection();
        return;
      }
      const hist = historyRef.current.slice(0, historyIndexRef.current + 1);
      hist.push(normalized);
      historyRef.current = hist;
      historyIndexRef.current = hist.length - 1;
      setHistoryTick((n) => n + 1);
      setCurrentPath(normalized);
      setRecentMode(false);
      setRecentOpenedMode(false);
      resetSelection();
    },
    [resetSelection],
  );

  const goBack = useCallback(() => {
    if (historyIndexRef.current > 0) {
      historyIndexRef.current--;
      setHistoryTick((n) => n + 1);
      setCurrentPath(historyRef.current[historyIndexRef.current]!);
      setRecentMode(false);
      setRecentOpenedMode(false);
      resetSelection();
    }
  }, [resetSelection]);

  const goForward = useCallback(() => {
    if (historyIndexRef.current < historyRef.current.length - 1) {
      historyIndexRef.current++;
      setHistoryTick((n) => n + 1);
      setCurrentPath(historyRef.current[historyIndexRef.current]!);
      setRecentMode(false);
      setRecentOpenedMode(false);
      resetSelection();
    }
  }, [resetSelection]);

  const goUp = useCallback(() => {
    if (currentPath === '/' || currentPath === '') return;
    const parentPath = currentPath.substring(0, currentPath.lastIndexOf('/')) || '/';
    if (parentPath !== currentPath) navigateTo(parentPath);
  }, [currentPath, navigateTo]);

  const canGoBack = historyIndexRef.current > 0;
  const canGoForward = historyIndexRef.current < historyRef.current.length - 1;
  const canGoUp = currentPath !== '/' && currentPath !== '';

  /** Resolve pasted/typed path via fs.stat; open parent if target is a file. */
  const resolveAndNavigate = useCallback(
    async (raw: string) => {
      navigationIntentRef.current = true;
      let path = raw.trim();
      if (!path) return;
      // Expand bare ~ to home if roots available
      if (path === '~' || path.startsWith('~/')) {
        try {
          const roots = hasNativeFiles() ? await fsApi().roots() : null;
          const home = Array.isArray(roots) ? roots.find((r) => r.id === 'home') : null;
          if (home?.path && shouldApplyHomeFallback(currentPath, navigationIntentRef.current)) {
            path = path === '~' ? home.path : home.path + path.slice(1);
          }
        } catch {
          /* keep as-is */
        }
      }
      // Normalize double slashes except leading
      path = path.replace(/\/{2,}/g, '/');
      if (path.length > 1 && path.endsWith('/')) path = path.slice(0, -1);

      try {
        const fs = hasNativeFiles() ? fsApi() : null;
        if (fs?.stat) {
          const st = await fs.stat(path);
          if (st?.found) {
            if (st.isDir) {
              navigateTo(st.path || path);
            } else {
              const parent = (st.path || path).substring(0, (st.path || path).lastIndexOf('/')) || '/';
              navigateTo(parent);
              // Soft-select after list loads: stash intended selection
              setPendingSelectFile(st.path || path);
            }
            return;
          }
          showToast(t(locale, 'fileBrowser.pathNotFound'));
          return;
        }
      } catch {
        // fall through to best-effort navigate
      }
      navigateTo(path);
    },
    [navigateTo, locale, showToast],
  );

  // 「最近修改」「最近打开」互斥切换
  const toggleRecent = useCallback(() => {
    setRecentMode((prev) => {
      const next = !prev;
      if (next) setRecentOpenedMode(false);
      return next;
    });
  }, []);

  const toggleRecentOpened = useCallback(() => {
    setRecentOpenedMode((prev) => {
      const next = !prev;
      if (next) setRecentMode(false);
      return next;
    });
  }, []);

  // 首次挂载：真实 home root 优先于 "/"（fanbox roots）
  useEffect(() => {
    async function load() {
      try {
        const fs = hasNativeFiles() ? fsApi() : null;
        if (fs?.roots && currentPath === '/') {
          const roots = await fs.roots();
          const home = Array.isArray(roots) ? roots.find((r) => r.id === 'home') : null;
          if (home?.path) {
            historyRef.current = [home.path];
            historyIndexRef.current = 0;
            setHistoryTick((n) => n + 1);
            setCurrentPath(home.path);
          }
        }
      } catch {
        /* ignore */
      }
    }
    void load();
  }, []);

  // Listen for external navigation events (sidebar quick access / favorites)
  useEffect(() => {
    const applyNav = (raw: NavigateFilesPayload) => {
      let path: string | undefined;
      if (typeof raw === 'string') path = raw;
      else if (raw && typeof raw === 'object') {
        path = raw.path ?? raw.directory;
      }
      if (!path) return;
      // resolveAndNavigate: file → parent dir + soft-select; dir → open
      void resolveAndNavigate(path);
    };

    // 挂载前发出的跳转（挂载竞态）：consume pending 兜底
    const pending = consumePendingNavigate();
    if (pending) {
      applyNav(pending);
    }

    return onFileEvent(FILE_EVENTS.navigateFiles, (payload) => {
      // 事件到达即代表在线处理，清掉发起方留下的 pending，防止下次挂载重放
      consumePendingNavigate();
      applyNav(payload);
    });
  }, [resolveAndNavigate]);

  // Keyboard shortcuts: Cmd+[ back, Cmd+] forward
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.metaKey && e.key === '[') {
        e.preventDefault();
        goBack();
      }
      if (e.metaKey && e.key === ']') {
        e.preventDefault();
        goForward();
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [goBack, goForward]);

  const segments = currentPath.split('/').filter(Boolean);

  return {
    currentPath,
    segments,
    historyTick,
    recentMode,
    recentOpenedMode,
    canGoBack,
    canGoForward,
    canGoUp,
    toggleRecent,
    toggleRecentOpened,
    navigateTo,
    goBack,
    goForward,
    goUp,
    resolveAndNavigate,
  };
}
