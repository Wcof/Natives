'use client';

import { useState, useEffect, useRef, useMemo, useCallback } from 'react';
import { t, type Locale } from '@/i18n';
import { type FileEntry } from '@/types/file';
import { SPACING, FONT_SIZE } from '@/lib/design-tokens';
import type { VirtualFileViewHandle } from '@/lib/preview/contracts';
import FileRow from './FileRow';
import { computeRowRange, rowScrollTop, VIRTUAL_ROW_HEIGHT } from './virtual-file-view-core';

interface FileListProps {
  entries: FileEntry[];
  sortBy: 'name' | 'mtime' | 'size';
  sortDir: 'asc' | 'desc';
  onSort: (sortBy: 'name' | 'mtime' | 'size') => void;
  onSelect: (entry: FileEntry, e?: { shiftKey?: boolean; metaKey?: boolean; ctrlKey?: boolean }) => void;
  onContextMenu?: (e: React.MouseEvent, entry: FileEntry) => void;
  showDir?: boolean;
  selectedIndex?: number;
  selectedPaths?: Set<string>;
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

export default function FileList({ entries, sortBy, sortDir, onSort, onSelect, onContextMenu, showDir, selectedIndex = -1, selectedPaths, onEditRequest, favorites, onFavoriteToggle, cutPaths, onMoveDrop, dragPaths, flashPaths, scrollContainerRef, onViewHandleReady }: FileListProps) {
  const [locale, setLocale] = useState<Locale>('zh');
  const [scroll, setScroll] = useState({ scrollTop: 0, viewportHeight: 0 });

  // 有界窗口：固定行高（R-P4，DOM 与 viewport 近似 O(viewport)）
  const range = useMemo(
    () => computeRowRange(scroll.scrollTop, scroll.viewportHeight || 800, entries.length, VIRTUAL_ROW_HEIGHT),
    [scroll.scrollTop, scroll.viewportHeight, entries.length],
  );

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

  // 冻结的 VirtualFileViewHandle 接缝（C0）
  const handle = useMemo<VirtualFileViewHandle>(
    () => ({
      scrollToIndex(index, options) {
        const target = rowScrollTop(index, 1, VIRTUAL_ROW_HEIGHT);
        const el = scrollContainerRef?.current;
        const align = options?.align ?? 'auto';
        if (el) {
          if (align === 'start') el.scrollTop = target;
          else if (align === 'center') el.scrollTop = Math.max(0, target - el.clientHeight / 2);
          else if (align === 'end') el.scrollTop = Math.max(0, target - el.clientHeight + VIRTUAL_ROW_HEIGHT);
          else el.scrollTo({ top: target, behavior: 'auto' });
        } else if (typeof window !== 'undefined') {
          const doc = document.scrollingElement;
          if (doc) doc.scrollTop = target;
        }
      },
      getColumnCount() {
        return 1;
      },
    }),
    [scrollContainerRef],
  );
  useEffect(() => {
    onViewHandleReady?.(handle);
  }, [handle, onViewHandleReady]);

  useEffect(() => {
    async function loadLocale() {
      try {
        const saved = await window.nativesAPI?.getLocale?.();
        if (saved === 'en') setLocale('en'); else setLocale('zh');
      } catch { /* ignore */ }
    }
    loadLocale();
  }, []);

  const SORT_LABELS: Record<string, string> = {
    name: t(locale, 'fileBrowser.name'),
    mtime: t(locale, 'fileBrowser.modified'),
    size: t(locale, 'fileBrowser.size'),
  };

  const visibleEntries = entries.slice(range.start, range.end);

  return (
    <div style={{ width: '100%', overflowX: 'auto' }} className="file-list-with-counters">
      {/* Header */}
      <div style={{
        display: 'grid',
        gridTemplateColumns: '24px 1fr 120px 80px 24px',
        gap: SPACING.sm,
        padding: '8px 12px',
        borderBottom: '1px solid var(--border)',
        fontSize: FONT_SIZE.sm,
        fontWeight: 600,
        fontFamily: 'var(--font-mono)',
        color: 'var(--text-secondary)',
        textTransform: 'uppercase',
        letterSpacing: '0.5px',
      }}>
        <div />
        {(['name', 'mtime', 'size'] as const).map((key) => (
          <div
            key={key}
            onClick={() => onSort(key)}
            style={{
              cursor: 'pointer',
              userSelect: 'none',
              color: sortBy === key ? 'var(--primary)' : undefined,
            }}
          >
            {SORT_LABELS[key]}
            {sortBy === key && (sortDir === 'asc' ? ' ▲' : ' ▼')}
          </div>
        ))}
        <div />
      </div>

      {/* Rows (windowed) */}
      {entries.length === 0 ? (
        <div style={{
          padding: 40,
          textAlign: 'center',
          color: 'var(--text-secondary)',
          fontSize: FONT_SIZE.lg,
        }}>
          {t(locale, 'fileBrowser.empty')}
        </div>
      ) : (
        <div style={{ position: 'relative', height: entries.length * VIRTUAL_ROW_HEIGHT }}>
          {visibleEntries.map((entry, offset) => {
            const index = range.start + offset;
            return (
              <div key={entry.path} style={{ position: 'absolute', top: index * VIRTUAL_ROW_HEIGHT, left: 0, right: 0 }}>
                <FileRow
                  entry={entry}
                  locale={locale}
                  onSelect={(ent, ev) => onSelect(ent, ev)}
                  onContextMenu={onContextMenu}
                  showDir={showDir}
                  selected={selectedPaths ? selectedPaths.has(entry.path) : index === selectedIndex}
                  onDoubleClick={() => onEditRequest?.(entry)}
                  isFavorite={favorites?.includes(entry.path)}
                  onFavoriteToggle={onFavoriteToggle}
                  dimmed={cutPaths?.has(entry.path)}
                  onMoveDrop={onMoveDrop}
                  dragPaths={dragPaths}
                  flash={flashPaths?.has(entry.path)}
                />
              </div>
            );
          })}
        </div>
      )}
      <div aria-hidden data-virtual-window-start={range.start} data-virtual-window-end={range.end} style={{ display: 'none' }} />
    </div>
  );
}
