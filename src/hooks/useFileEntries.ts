'use client';

/**
 * useFileEntries — FileBrowser 目录加载 / 变更监听 / 卡片点亮状态。
 *
 * 职责边界（F3-04 拆分，ARCH-002）：
 * - entries / dirProject / loading / nativeMissing 状态
 * - loadEntries：普通目录 / 「最近修改」（后端递归）/「最近打开」（LRU + stat）
 *   三种来源；代次守卫丢弃过期响应
 * - fs_watch 接线：噪声过滤 + selfOpened 窗口 + 防抖整目录刷新 + 150ms 合并卡片点亮
 * - fileFlash 事件 → flashPaths（卡片 heat）
 * - selfOpened 登记（markSelfOpened，供打开/预览类动作标记，防止假变更点亮）
 *
 * 不持有：导航、选择、文件操作、拖拽、preview selection。
 */

import { useCallback, useEffect, useRef, useState } from 'react';
import { t, type Locale } from '@/i18n';
import { type FileEntry, type StatResult } from '@/types/file';
import { fsApi, hasNativeFiles } from '@/lib/files-api';
import { FILE_EVENTS, dispatchFileEvent, onFileEvent } from '@/lib/file-events';
import {
  SelfOpenedTracker,
  isNoisyChangePath,
  topChildOf,
} from '@/lib/fs-change-filter';
import { useFsWatch } from '@/lib/use-fs-watch';

/** 并发映射（loadEntries 内部使用；8 并发拉取最近打开文件 stat） */
async function mapWithConcurrency<T, R>(items: T[], limit: number, fn: (item: T) => Promise<R>): Promise<R[]> {
  const result: R[] = [];
  let cursor = 0;
  async function worker() {
    while (cursor < items.length) {
      const index = cursor++;
      result[index] = await fn(items[index]!);
    }
  }
  await Promise.all(Array.from({ length: Math.min(limit, items.length) }, worker));
  return result;
}

export interface UseFileEntriesOptions {
  currentPath: string;
  sortBy: 'name' | 'mtime' | 'size';
  sortDir: 'asc' | 'desc';
  showHidden: boolean;
  recentMode: boolean;
  recentOpenedMode: boolean;
  /** 最近打开 LRU 快照（来自 useRecentFiles） */
  recentOpenedPaths: string[];
  showToast: (msg: string) => void;
  locale: Locale;
}

export interface UseFileEntriesResult {
  entries: FileEntry[];
  dirProject: string | null;
  loading: boolean;
  nativeMissing: boolean;
  /** 最近一次变更点亮路径集合（fileFlash 事件驱动） */
  flashPaths: Set<string>;
  loadEntries: () => Promise<void>;
  /** 打开/预览类动作登记 selfOpened，3s 窗口内 fs_watch 忽略其假变更 */
  markSelfOpened: (path: string) => void;
}

export function useFileEntries({
  currentPath,
  sortBy,
  sortDir,
  showHidden,
  recentMode,
  recentOpenedMode,
  recentOpenedPaths,
  showToast,
  locale,
}: UseFileEntriesOptions): UseFileEntriesResult {
  const [entries, setEntries] = useState<FileEntry[]>([]);
  /** 当前目录项目类型（node/web/python/rust/go/git），来源：后端 list_dir_detailed */
  const [dirProject, setDirProject] = useState<string | null>(null);
  /** 浏览器 dev 模式（无 Tauri IPC）：渲染明确的降级占位，不再静默报错。 */
  const [nativeMissing, setNativeMissing] = useState(false);
  const [loading, setLoading] = useState(true);
  /** 代次守卫：快速导航时丢弃过期的 loadEntries 响应，防止旧内容覆盖新目录 */
  const loadIdRef = useRef(0);
  /** 供 loadEntries 读取的最新 LRU 快照，避免把 paths 放进依赖数组导致预览时重载 */
  const recentOpenedPathsRef = useRef<string[]>(recentOpenedPaths);
  recentOpenedPathsRef.current = recentOpenedPaths;

  useEffect(() => {
    if (!hasNativeFiles()) setNativeMissing(true);
  }, []);

  const loadEntries = useCallback(async () => {
    // 代次守卫：只有最新一次调用允许写回状态
    const rid = ++loadIdRef.current;
    setLoading(true);
    try {
      const fs = fsApi();

      if (recentOpenedMode) {
        // 最近打开模式：读取 LRU（客户端记录），逐项 stat 过滤死链接。
        // kind/name/dirHint 由后端 stat（StatResult）下发，前端不再本地推断。
        const paths = recentOpenedPathsRef.current;
        const settled = await mapWithConcurrency(paths, 8, async (p) => {
          try {
            const st = (await fs.stat(p)) as StatResult;
            if (!st?.found || st.isDir) return null; // 死链接或已变成目录 → 跳过
            const name = st.name || p.split('/').pop() || '';
            const dir = st.dirHint || p.substring(0, p.lastIndexOf('/')) || '/';
            return {
              name,
              path: st.path || p,
              isDir: false,
              kind: st.kind ?? 'other',
              hidden: name.startsWith('.'),
              size: st.size || 0,
              mtime: st.mtime || 0,
              btime: st.btime || 0,
              dirHint: dir === currentPath ? undefined : dir,
            } as FileEntry;
          } catch {
            return null;
          }
        });
        if (rid !== loadIdRef.current) return;
        setEntries(settled.filter((e): e is FileEntry => e !== null));
      } else if (recentMode) {
        // 最近修改模式：后端递归扫描直接返回完整 FileEntry（含 kind/dirHint）
        const recentData = (await fs.recentFiles(currentPath)) as FileEntry[] | null;
        if (rid !== loadIdRef.current) return;
        if (Array.isArray(recentData)) {
          // 唯一的前端归一化：当前目录内的文件不显示来源目录提示
          setEntries(
            recentData.map((f) => (f.dirHint === currentPath ? { ...f, dirHint: undefined } : f)),
          );
        } else {
          setEntries([]);
        }
      } else {
        const options = { sortBy, sortDir, showHidden, probeProjects: true };
        // Prefer detailed list (entries + project badges on subdirs); fall back to plain listDir
        if (typeof fs.listDirDetailed === 'function') {
          const detailed = (await fs.listDirDetailed(currentPath, options)) as {
            entries?: FileEntry[];
            project?: string | null;
          };
          if (rid !== loadIdRef.current) return;
          setEntries(Array.isArray(detailed?.entries) ? detailed.entries : []);
          setDirProject(detailed?.project ?? null);
        } else {
          const data = await fs.listDir(currentPath, options);
          if (rid !== loadIdRef.current) return;
          setEntries((data as FileEntry[]) || []);
          setDirProject(null);
        }
      }
    } catch (err) {
      if (rid !== loadIdRef.current) return;
      showToast(t(locale, 'fileBrowser.loadFailed'));
      setEntries([]);
    } finally {
      if (rid === loadIdRef.current) setLoading(false);
    }
  }, [currentPath, sortBy, sortDir, showHidden, recentMode, recentOpenedMode, locale, showToast]);

  // 初始加载 + 目录/排序/过滤变化时刷新
  useEffect(() => {
    void loadEntries();
  }, [loadEntries]);

  // 「最近打开」模式下，LRU 变化时刷新列表（loadEntries 用 ref 读取 paths，需显式触发）
  useEffect(() => {
    if (!recentOpenedMode) return;
    void loadEntries();
  }, [recentOpenedPaths, recentOpenedMode]);

  // ── fs_watch 接线：当前目录的真实文件变更 → 卡片点亮（改·N/heat）+ 防抖自动刷新 ──
  const selfOpenedRef = useRef(new SelfOpenedTracker());
  const watchRefreshTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pendingFlashRef = useRef<Set<string>>(new Set());
  const flashFlushTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useFsWatch(
    currentPath && currentPath !== '/' ? currentPath : null,
    useCallback(
      (event) => {
        if (isNoisyChangePath(event.path, currentPath)) return;
        if (selfOpenedRef.current.isSelfNoise(event.path)) return;
        // 点亮直接子项卡片；事件风暴（npm install 级）下按 150ms 合并 dispatch，
        // 避免逐条事件触发 setState 重渲染
        const child = topChildOf(currentPath, event.path);
        // 直接子项事件（create/remove/rename/modify 直接成员）才触发整目录刷新；
        // 深层后代修改只点亮顶层子项，不反复 whole reload（watch storm 预算 ≤2/settling）
        const isDirectChildEvent = child !== null && child === event.path;
        if (child) {
          pendingFlashRef.current.add(child);
          if (!flashFlushTimerRef.current) {
            flashFlushTimerRef.current = setTimeout(() => {
              flashFlushTimerRef.current = null;
              const paths = pendingFlashRef.current;
              pendingFlashRef.current = new Set();
              paths.forEach((p) => dispatchFileEvent(FILE_EVENTS.fileFlash, p));
            }, 150);
          }
        }
        // 仅直接子项变更走 250ms 防抖整目录刷新（fanbox 同参）；loadEntries 自带代次守卫
        if (isDirectChildEvent) {
          if (watchRefreshTimerRef.current) clearTimeout(watchRefreshTimerRef.current);
          watchRefreshTimerRef.current = setTimeout(() => {
            watchRefreshTimerRef.current = null;
            void loadEntries();
          }, 250);
        }
      },
      [currentPath, loadEntries],
    ),
  );

  useEffect(
    () => () => {
      if (watchRefreshTimerRef.current) clearTimeout(watchRefreshTimerRef.current);
      if (flashFlushTimerRef.current) clearTimeout(flashFlushTimerRef.current);
    },
    [],
  );

  // fileFlash 事件 → flashPaths（卡片 heat）；1200ms 后熄灭
  const [flashPaths, setFlashPaths] = useState<Set<string>>(new Set());
  useEffect(() => {
    return onFileEvent(FILE_EVENTS.fileFlash, (path) => {
      if (!path) return;
      setFlashPaths((prev) => new Set(prev).add(path));
      window.setTimeout(() => {
        setFlashPaths((prev) => {
          const next = new Set(prev);
          next.delete(path);
          return next;
        });
      }, 1200);
    });
  }, []);

  const markSelfOpened = useCallback((path: string) => {
    selfOpenedRef.current.mark(path);
  }, []);

  return {
    entries,
    dirProject,
    loading,
    nativeMissing,
    flashPaths,
    loadEntries,
    markSelfOpened,
  };
}
