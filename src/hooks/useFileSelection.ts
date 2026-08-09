'use client';

/**
 * useFileSelection — FileBrowser 选择状态（单选光标 + 多选）。
 *
 * 职责边界（F3-04 拆分，ARCH-002）：
 * - selectedIndex（主光标）/ selectedPaths（shift/cmd 多选）/ lastClickedIndexRef
 * - handleSelect：单点（目录即导航）、shift 区间、cmd 切换多选
 * - handleContextMenu：右键落在多选外时先收敛选择，再打开上下文菜单
 * - 目录/搜索变化时重置选择；软选择（路径栏跳到文件 → 父目录 + 定位）
 * - 选中项滚动进视图（T30 VirtualFileViewHandle 接缝优先，DOM 查询兜底）
 *
 * 键盘文件区快捷键属于跨选择/操作/导航的组合层，留在 FileBrowser 组合。
 */

import { startTransition, useCallback, useEffect, useRef, useState } from 'react';
import { type FileEntry } from '@/types/file';
import type { VirtualFileViewHandle } from '@/lib/preview/contracts';
import { consumePendingSelectFile, setPendingSelectFile } from '@/lib/file-events';

export interface UseFileSelectionOptions {
  entries: FileEntry[];
  filteredEntries: FileEntry[];
  currentPath: string;
  searchQuery: string;
  viewHandleRef: React.MutableRefObject<VirtualFileViewHandle | null>;
  /** 单点目录即导航（FileBrowser 组合层传入 nav.navigateTo） */
  onNavigate: (path: string) => void;
  onOpenContextMenu: (x: number, y: number, entry: FileEntry, mode: 'file' | 'dir') => void;
}

/**
 * 多选集合计算（纯函数，便于单测）。
 * 与原 FileBrowser handleSelect 的语义完全一致：
 * - shift：从 lastClickedIndex 到 idx 的闭区间全部加入（锚点不变）
 * - cmd/meta：切换 entryPath 是否在集合内（锚点更新为 idx）
 * - 其它：返回原集合（单点路径由调用方处理）
 */
export function computeMultiSelect(input: {
  prev: Set<string>;
  entryPath: string;
  idx: number;
  lastClickedIndex: number;
  isShift: boolean;
  isCmd: boolean;
  orderedPaths: string[];
}): { next: Set<string>; lastClickedIndex: number } {
  const next = new Set(input.prev);
  if (input.isShift && input.lastClickedIndex >= 0 && input.idx >= 0) {
    const a = Math.min(input.lastClickedIndex, input.idx);
    const b = Math.max(input.lastClickedIndex, input.idx);
    for (let i = a; i <= b; i++) {
      const p = input.orderedPaths[i];
      if (p) next.add(p);
    }
    return { next, lastClickedIndex: input.lastClickedIndex };
  }
  if (input.isCmd) {
    if (next.has(input.entryPath)) next.delete(input.entryPath);
    else next.add(input.entryPath);
    return { next, lastClickedIndex: input.idx };
  }
  return { next, lastClickedIndex: input.lastClickedIndex };
}

export interface UseFileSelectionResult {
  selectedIndex: number;
  setSelectedIndex: React.Dispatch<React.SetStateAction<number>>;
  selectedPaths: Set<string>;
  setSelectedPaths: React.Dispatch<React.SetStateAction<Set<string>>>;
  lastClickedIndexRef: React.MutableRefObject<number>;
  handleSelect: (entry: FileEntry, e?: { shiftKey?: boolean; metaKey?: boolean; ctrlKey?: boolean }) => void;
  handleContextMenu: (e: React.MouseEvent, entry: FileEntry) => void;
  resetSelection: () => void;
}

export function useFileSelection({
  entries,
  filteredEntries,
  currentPath,
  searchQuery,
  viewHandleRef,
  onNavigate,
  onOpenContextMenu,
}: UseFileSelectionOptions): UseFileSelectionResult {
  const [selectedIndex, setSelectedIndex] = useState(-1);
  /** Multi-selection by path (shift/cmd click). Primary cursor remains selectedIndex. */
  const [selectedPaths, setSelectedPaths] = useState<Set<string>>(new Set());
  const lastClickedIndexRef = useRef<number>(-1);

  const resetSelection = useCallback(() => {
    setSelectedPaths(new Set());
    lastClickedIndexRef.current = -1;
  }, []);

  const handleSelect = useCallback(
    (entry: FileEntry, e?: { shiftKey?: boolean; metaKey?: boolean; ctrlKey?: boolean }) => {
      const idx = filteredEntries.findIndex((x) => x.path === entry.path);
      const isMulti = !!(e && (e.metaKey || e.ctrlKey || e.shiftKey));

      if (isMulti) {
        setSelectedIndex(idx);
        setSelectedPaths((prev) => {
          const { next, lastClickedIndex } = computeMultiSelect({
            prev,
            entryPath: entry.path,
            idx,
            lastClickedIndex: lastClickedIndexRef.current,
            isShift: !!e?.shiftKey,
            isCmd: !!(e?.metaKey || e?.ctrlKey),
            orderedPaths: filteredEntries.map((x) => x.path),
          });
          lastClickedIndexRef.current = lastClickedIndex;
          return next;
        });
        return;
      }

      // Single click: set selection; directories open immediately (fanbox)
      lastClickedIndexRef.current = idx;
      setSelectedIndex(idx);
      setSelectedPaths(new Set([entry.path]));
      if (entry.isDir) {
        onNavigate(entry.path);
      }
    },
    [filteredEntries, onNavigate],
  );

  const handleContextMenu = useCallback(
    (e: React.MouseEvent, entry: FileEntry) => {
      e.preventDefault();
      // Stop propagation so the blank-area handler on the parent doesn't fire
      e.stopPropagation();
      // If right-click target is outside current multi-selection, select only it
      if (!selectedPaths.has(entry.path)) {
        setSelectedPaths(new Set([entry.path]));
        const idx = filteredEntries.findIndex((x) => x.path === entry.path);
        setSelectedIndex(idx);
        lastClickedIndexRef.current = idx;
      }
      onOpenContextMenu(e.clientX, e.clientY, entry, entry.isDir ? 'dir' : 'file');
    },
    [selectedPaths, filteredEntries, onOpenContextMenu],
  );

  // Reset selection when entries or path change; honor pending file selection from path bar
  useEffect(() => {
    startTransition(() => {
      const pending = consumePendingSelectFile();
      if (pending) {
        const idx = entries.findIndex((e) => e.path === pending);
        if (idx >= 0) {
          setSelectedIndex(idx);
          setSelectedPaths(new Set([pending]));
          lastClickedIndexRef.current = idx;
          return;
        }
        // 目标还没随 entries 到货（导航刚发起）：放回，等下一次 entries 更新再消费
        setPendingSelectFile(pending);
      }
      setSelectedIndex(-1);
      setSelectedPaths(new Set());
      lastClickedIndexRef.current = -1;
    });
  }, [currentPath, entries, searchQuery]);

  // Scroll keyboard selection into view（虚拟窗口跨窗口：走 T30 VirtualFileViewHandle 接缝）
  useEffect(() => {
    if (selectedIndex < 0 || selectedIndex >= filteredEntries.length) return;
    const handle = viewHandleRef.current;
    if (handle) {
      handle.scrollToIndex(selectedIndex, { align: 'auto' });
      return;
    }
    const path = filteredEntries[selectedIndex]?.path;
    if (!path) return;
    const el = document.querySelector(`[data-file-entry="${CSS.escape(path)}"]`) as HTMLElement | null;
    el?.scrollIntoView({ block: 'nearest' });
  }, [selectedIndex, filteredEntries]);

  return {
    selectedIndex,
    setSelectedIndex,
    selectedPaths,
    setSelectedPaths,
    lastClickedIndexRef,
    handleSelect,
    handleContextMenu,
    resetSelection,
  };
}
