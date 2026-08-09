'use client';

import { forwardRef, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { type FileEntry } from '@/types/file';
import { t, useLocale } from '@/i18n';
import type { VirtualFileViewHandle } from '@/lib/preview/contracts';
import FileCard from './FileCard';
import { FONT_SIZE, SPACING } from '@/lib/design-tokens';
import {
  computeGridColumnCount,
  gridIndexRange,
  groupIntoRows,
  rowScrollTop,
} from './virtual-file-view-core';

interface FileGridProps {
  entries: FileEntry[];
  onSelect: (entry: FileEntry, e?: { shiftKey?: boolean; metaKey?: boolean; ctrlKey?: boolean }) => void;
  onContextMenu?: (e: React.MouseEvent, entry: FileEntry) => void;
  selectedIndex?: number;
  selectedPaths?: Set<string>;
  gridSize?: 'sm' | 'md' | 'lg';
  onEditRequest?: (entry: FileEntry) => void;
  favorites?: string[];
  onFavoriteToggle?: (entry: FileEntry) => void;
  cutPaths?: Set<string>;
  onMoveDrop?: (sourcePaths: string[], destDir: string) => void;
  dragPaths?: string[];
  flashPaths?: Set<string>;
  /** T31 seam: 真实 scroll container（默认回退到 window） */
  scrollContainerRef?: React.RefObject<HTMLElement | null>;
  /** T31 seam: 冻结的 VirtualFileViewHandle（scrollToIndex/getColumnCount） */
  onViewHandleReady?: (handle: VirtualFileViewHandle) => void;
}

const GRID_COLS: Record<string, number> = { sm: 100, md: 140, lg: 200 };
const DEFAULT_GRID_COL = 140;
const GRID_GAP = 10;
const GRID_ROW_HEIGHT = 180 + GRID_GAP; // card 高 + gap 的固定估算（窗口化需要行高）

const FileGrid = forwardRef<HTMLDivElement, FileGridProps>(function FileGrid(
  { entries, onSelect, onContextMenu, selectedIndex = -1, selectedPaths, gridSize = 'md', onEditRequest, favorites, onFavoriteToggle, cutPaths, onMoveDrop, dragPaths, flashPaths, scrollContainerRef, onViewHandleReady },
  ref,
) {
  const locale = useLocale();
  const containerRef = useRef<HTMLDivElement>(null);
  const [columnCount, setColumnCount] = useState(computeGridColumnCount(0, DEFAULT_GRID_COL, GRID_GAP));
  const [scroll, setScroll] = useState({ scrollTop: 0, viewportHeight: 0 });

  // 容器宽度 → 列数（ResizeObserver；grid resize 后 column count 语义正确）
  useEffect(() => {
    const node = containerRef.current;
    if (!node || typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver(([item]) => {
      const width = item?.contentRect?.width ?? node.clientWidth;
      setColumnCount((prev) => {
        const next = computeGridColumnCount(width, GRID_COLS[gridSize] ?? DEFAULT_GRID_COL, GRID_GAP);
        return next === prev ? prev : next;
      });
    });
    observer.observe(node);
    return () => observer.disconnect();
  }, [gridSize]);

  // 滚动源：T31 传入的 scroll container 或 window（T30 不越权改 FileBrowser）
  const getScroller = useCallback((): { scrollTop: number; viewportHeight: number } | null => {
    const el = scrollContainerRef?.current;
    if (el) return { scrollTop: el.scrollTop, viewportHeight: el.clientHeight };
    if (typeof document === 'undefined') return null;
    const se = document.scrollingElement;
    if (!se) return null;
    return { scrollTop: se.scrollTop, viewportHeight: window.innerHeight };
  }, [scrollContainerRef]);

  useEffect(() => {
    const update = () => {
      const s = getScroller();
      if (s) setScroll(s);
    };
    update();
    const el = scrollContainerRef?.current;
    if (el) {
      el.addEventListener('scroll', update, { passive: true });
      return () => el.removeEventListener('scroll', update);
    }
    window.addEventListener('scroll', update, { passive: true });
    window.addEventListener('resize', update);
    return () => {
      window.removeEventListener('scroll', update);
      window.removeEventListener('resize', update);
    };
  }, [getScroller, scrollContainerRef]);

  // 有界窗口：行级别窗口化（DOM 与 viewport 近似 O(viewport)，滚动后不累积）
  const rows = useMemo(() => groupIntoRows(entries, columnCount), [entries, columnCount]);
  const rowRange = useMemo(
    () => gridIndexRange(scroll.scrollTop, scroll.viewportHeight || 900, entries.length, columnCount, GRID_ROW_HEIGHT),
    [scroll.scrollTop, scroll.viewportHeight, entries.length, columnCount],
  );
  const startRow = Math.floor(rowRange.start / Math.max(1, columnCount));
  const visibleRowCount = Math.ceil((rowRange.end - rowRange.start) / Math.max(1, columnCount));
  const visibleRows = rows.slice(startRow, startRow + visibleRowCount);

  // 冻结的 VirtualFileViewHandle 接缝（C0）：T31 持 ref 调 scrollToIndex/getColumnCount
  const handle = useMemo<VirtualFileViewHandle>(
    () => ({
      scrollToIndex(index, options) {
        const target = rowScrollTop(index, columnCount, GRID_ROW_HEIGHT);
        const el = scrollContainerRef?.current;
        const align = options?.align ?? 'auto';
        if (el) {
          if (align === 'start') el.scrollTop = target;
          else if (align === 'center') el.scrollTop = Math.max(0, target - el.clientHeight / 2);
          else if (align === 'end') el.scrollTop = Math.max(0, target - el.clientHeight + GRID_ROW_HEIGHT);
          else el.scrollTo({ top: target, behavior: 'auto' });
        } else if (typeof window !== 'undefined') {
          const doc = document.scrollingElement;
          if (doc) doc.scrollTop = target;
        }
      },
      getColumnCount() {
        return columnCount;
      },
    }),
    [columnCount, scrollContainerRef],
  );
  useEffect(() => {
    onViewHandleReady?.(handle);
  }, [handle, onViewHandleReady]);

  if (entries.length === 0) {
    return (
      <div style={{
        display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center',
        height: '100%', color: 'var(--text-secondary)', fontSize: FONT_SIZE.lg,
      }}>
        {t(locale, 'fileBrowser.empty')}
      </div>
    );
  }

  return (
    <div
      ref={(node) => {
        containerRef.current = node;
        if (typeof ref === 'function') ref(node);
        else if (ref) (ref as React.MutableRefObject<HTMLDivElement | null>).current = node;
      }}
      style={{
        display: 'grid',
        gridTemplateColumns: `repeat(${columnCount}, minmax(0, 1fr))`,
        gap: GRID_GAP,
        padding: SPACING.md,
      }}
    >
      {visibleRows.map((rowEntries, rowIndex) => {
        const rowAbs = startRow + rowIndex;
        return rowEntries.map((entry, colIndex) => {
          const index = rowAbs * Math.max(1, columnCount) + colIndex;
          return (
            <FileCard
              key={entry.path}
              entry={entry}
              locale={locale}
              onSelect={(ent, ev) => onSelect(ent, ev)}
              onContextMenu={onContextMenu}
              selected={selectedPaths ? selectedPaths.has(entry.path) : index === selectedIndex}
              onDoubleClick={() => onEditRequest?.(entry)}
              isFavorite={favorites?.includes(entry.path)}
              onFavoriteToggle={onFavoriteToggle}
              dimmed={cutPaths?.has(entry.path)}
              onMoveDrop={onMoveDrop}
              dragPaths={dragPaths}
              flash={flashPaths?.has(entry.path)}
            />
          );
        });
      })}
      {/* 占位撑高：让滚动位置正确，且不创建无界 DOM（R-P4） */}
      <div aria-hidden data-virtual-window-start={rowRange.start} data-virtual-window-end={rowRange.end} style={{ display: 'none' }} />
      <div aria-hidden style={{ gridColumn: '1 / -1', height: Math.max(0, rows.length * GRID_ROW_HEIGHT - (startRow + visibleRowCount) * GRID_ROW_HEIGHT) }} />
    </div>
  );
});

export default FileGrid;
