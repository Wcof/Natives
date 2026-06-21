'use client';

import { forwardRef } from 'react';
import { type FileEntry } from '@/types/file';
import { t, useLocale } from '@/i18n';
import FileCard from './FileCard';
import { FONT_SIZE, SPACING } from '@/lib/design-tokens';

interface FileGridProps {
  entries: FileEntry[];
  onSelect: (entry: FileEntry) => void;
  onContextMenu?: (e: React.MouseEvent, entry: FileEntry) => void;
  selectedIndex?: number;
  gridSize?: 'sm' | 'md' | 'lg';
  onEditRequest?: (entry: FileEntry) => void;
  favorites?: string[];
  onFavoriteToggle?: (entry: FileEntry) => void;
}

const GRID_COLS: Record<string, string> = {
  sm: 'repeat(auto-fill, minmax(100px, 1fr))',
  md: 'repeat(auto-fill, minmax(140px, 1fr))',
  lg: 'repeat(auto-fill, minmax(200px, 1fr))',
};

const FileGrid = forwardRef<HTMLDivElement, FileGridProps>(function FileGrid(
  { entries, onSelect, onContextMenu, selectedIndex = -1, gridSize = 'md', onEditRequest, favorites, onFavoriteToggle },
  ref,
) {
  const locale = useLocale();

  if (entries.length === 0) {
    return (
      <div style={{
        display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center',
        height: '100%', color: 'var(--vibe-btn-text)', fontSize: FONT_SIZE.lg,
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
      {entries.map((entry, index) => (
        <FileCard
          key={entry.path}
          entry={entry}
          onSelect={onSelect}
          onContextMenu={onContextMenu}
          selected={index === selectedIndex}
          onDoubleClick={() => onEditRequest?.(entry)}
          isFavorite={favorites?.includes(entry.path)}
          onFavoriteToggle={onFavoriteToggle}
        />
      ))}
    </div>
  );
});

export default FileGrid;
