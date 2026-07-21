'use client';

import { useState, useEffect } from 'react';
import { t, type Locale } from '@/i18n';
import { type FileEntry } from '@/types/file';
import { SPACING, FONT_SIZE } from '@/lib/design-tokens';
import FileRow from './FileRow';

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
}

export default function FileList({ entries, sortBy, sortDir, onSort, onSelect, onContextMenu, showDir, selectedIndex = -1, selectedPaths, onEditRequest, favorites, onFavoriteToggle, cutPaths, onMoveDrop, dragPaths }: FileListProps) {
  const [locale, setLocale] = useState<Locale>('zh');

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

      {/* Rows */}
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
        entries.map((entry, index) => (
          <FileRow
            key={entry.path}
            entry={entry}
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
          />
        ))
      )}
    </div>
  );
}
