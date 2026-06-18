'use client';

import { useState, useEffect } from 'react';
import { FolderOpen } from 'lucide-react';
import { t, type Locale } from '@/i18n';
import { type FileEntry } from '@/types/file';
import FileRow from './FileRow';

interface FileListProps {
  entries: FileEntry[];
  sortBy: 'name' | 'mtime' | 'size';
  sortDir: 'asc' | 'desc';
  onSort: (sortBy: 'name' | 'mtime' | 'size') => void;
  onSelect: (entry: FileEntry) => void;
  onContextMenu?: (e: React.MouseEvent, entry: FileEntry) => void;
  showDir?: boolean;
}

export default function FileList({ entries, sortBy, sortDir, onSort, onSelect, onContextMenu, showDir }: FileListProps) {
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
        gap: 8,
        padding: '8px 12px',
        borderBottom: '1px solid var(--vibe-btn-border)',
        fontSize: 11,
        fontWeight: 600,
        fontFamily: 'var(--font-mono)',
        color: 'var(--vibe-btn-text)',
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
              color: sortBy === key ? 'var(--accent)' : undefined,
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
          color: 'var(--vibe-btn-text)',
          fontSize: 13,
        }}>
          <FolderOpen size={20} style={{ color: 'var(--text-faint)', marginBottom: 4 }} />
          <div>{t(locale, 'fileBrowser.empty')}</div>
        </div>
      ) : (
        entries.map((entry) => (
          <FileRow
            key={entry.path}
            entry={entry}
            onSelect={onSelect}
            onContextMenu={onContextMenu}
            showDir={showDir}
          />
        ))
      )}
    </div>
  );
}
