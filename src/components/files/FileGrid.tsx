'use client';

import { forwardRef, useEffect, useRef, useState } from 'react';
import { type FileEntry } from '@/types/file';
import { t, useLocale } from '@/i18n';
import FileCard from './FileCard';
import { FONT_SIZE, SPACING } from '@/lib/design-tokens';

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
}

const GRID_COLS: Record<string, string> = {
  sm: 'repeat(auto-fill, minmax(100px, 1fr))',
  md: 'repeat(auto-fill, minmax(140px, 1fr))',
  lg: 'repeat(auto-fill, minmax(200px, 1fr))',
};

const FileGrid = forwardRef<HTMLDivElement, FileGridProps>(function FileGrid(
  { entries, onSelect, onContextMenu, selectedIndex = -1, selectedPaths, gridSize = 'md', onEditRequest, favorites, onFavoriteToggle, cutPaths, onMoveDrop, dragPaths, flashPaths },
  ref,
) {
  const locale = useLocale();
  const [count, setCount] = useState(200);
  const sentinelRef = useRef<HTMLDivElement>(null);
  useEffect(() => { setCount(200); }, [entries]);
  useEffect(() => {
    const node = sentinelRef.current;
    if (!node || typeof IntersectionObserver === 'undefined') return;
    const observer = new IntersectionObserver(([item]) => {
      if (item?.isIntersecting) setCount((value) => Math.min(value + 200, entries.length));
    });
    observer.observe(node);
    return () => observer.disconnect();
  }, [entries.length]);

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
      ref={ref}
      style={{
        display: 'grid',
        gridTemplateColumns: GRID_COLS[gridSize] || GRID_COLS.md,
        gap: 10,
        padding: SPACING.md,
      }}
    >
      {entries.slice(0, count).map((entry, index) => (
        <FileCard
          key={entry.path}
          entry={entry}
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
      ))}
      {count < entries.length && <div ref={sentinelRef} style={{ minHeight: 1, gridColumn: '1 / -1' }} />}
    </div>
  );
});

export default FileGrid;
