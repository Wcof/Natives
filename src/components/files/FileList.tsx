'use client';

import { type FileEntry } from '@/types/file';
import FileRow from './FileRow';

interface FileListProps {
  entries: FileEntry[];
  sortBy: 'name' | 'mtime' | 'size';
  sortDir: 'asc' | 'desc';
  onSort: (sortBy: 'name' | 'mtime' | 'size') => void;
  onSelect: (entry: FileEntry) => void;
  onContextMenu?: (e: React.MouseEvent, entry: FileEntry) => void;
}

const SORT_LABELS: Record<string, string> = {
  name: 'Name',
  mtime: 'Modified',
  size: 'Size',
};

export default function FileList({ entries, sortBy, sortDir, onSort, onSelect, onContextMenu }: FileListProps) {
  return (
    <div style={{ width: '100%', overflowX: 'auto' }}>
      {/* Header */}
      <div style={{
        display: 'grid',
        gridTemplateColumns: '24px 1fr 100px 80px 160px',
        gap: 8,
        padding: '8px 12px',
        borderBottom: '1px solid var(--border, #262920)',
        fontSize: 11,
        fontWeight: 600,
        color: 'var(--text-dim, #9b9d8c)',
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
              color: sortBy === key ? 'var(--accent, #cdf24b)' : undefined,
            }}
          >
            {SORT_LABELS[key]}
            {sortBy === key && (sortDir === 'asc' ? ' ▲' : ' ▼')}
          </div>
        ))}
        <div style={{ color: 'var(--text-faint, #62655a)' }}>Type</div>
      </div>

      {/* Rows */}
      {entries.length === 0 ? (
        <div style={{
          padding: 40,
          textAlign: 'center',
          color: 'var(--text-faint, #62655a)',
          fontSize: 13,
        }}>
          This folder is empty
        </div>
      ) : (
        entries.map((entry) => (
          <FileRow
            key={entry.path}
            entry={entry}
            onSelect={onSelect}
            onContextMenu={onContextMenu}
          />
        ))
      )}
    </div>
  );
}
