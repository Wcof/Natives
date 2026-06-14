'use client';

import { type FileEntry } from '@/types/file';

interface FileRowProps {
  entry: FileEntry;
  onSelect: (entry: FileEntry) => void;
  onContextMenu?: (e: React.MouseEvent, entry: FileEntry) => void;
}

const FILE_TYPE_ICONS: Record<string, string> = {
  text: '📄',
  image: '🖼️',
  video: '🎬',
  audio: '🎵',
  pdf: '📕',
  archive: '📦',
  other: '📎',
};

function formatSize(bytes: number): string {
  const units = ['B', 'KB', 'MB', 'GB'];
  let size = bytes;
  let unitIdx = 0;
  while (size >= 1024 && unitIdx < units.length - 1) {
    size /= 1024;
    unitIdx++;
  }
  return `${size.toFixed(1)} ${units[unitIdx]}`;
}

function formatTime(ms: number): string {
  const date = new Date(ms);
  const now = new Date();
  const diffDays = Math.floor((now.getTime() - date.getTime()) / (1000 * 60 * 60 * 24));

  if (diffDays === 0) return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
  if (diffDays < 7) return `${diffDays}d ago`;
  return date.toLocaleDateString([], { month: 'short', day: 'numeric' });
}

export default function FileRow({ entry, onSelect, onContextMenu }: FileRowProps) {
  const icon = entry.isDir ? '📁' : FILE_TYPE_ICONS[entry.kind] || '📄';

  return (
    <div
      onClick={() => onSelect(entry)}
      onContextMenu={(e) => onContextMenu?.(e, entry)}
      role="button"
      tabIndex={0}
      onKeyDown={(e) => { if (e.key === 'Enter') onSelect(entry); }}
      style={{
        display: 'grid',
        gridTemplateColumns: '24px 1fr 100px 80px 160px',
        gap: 8,
        padding: '6px 12px',
        alignItems: 'center',
        cursor: 'pointer',
        fontSize: 13,
        color: 'var(--text, #f2f2ea)',
        borderBottom: '1px solid var(--border, #262920)',
        transition: 'background 0.08s',
      }}
      onMouseEnter={(e) => { (e.currentTarget as HTMLElement).style.background = 'var(--bg-2, #131410)'; }}
      onMouseLeave={(e) => { (e.currentTarget as HTMLElement).style.background = 'transparent'; }}
    >
      <span style={{ fontSize: 16 }}>{icon}</span>
      <div style={{
        overflow: 'hidden',
        textOverflow: 'ellipsis',
        whiteSpace: 'nowrap',
        display: 'flex',
        alignItems: 'center',
        gap: 6,
      }}>
        <span>{entry.name}</span>
        {entry.hidden && <span style={{ fontSize: 10, color: 'var(--text-faint, #62655a)' }}>(hidden)</span>}
        {entry.symlink && <span style={{ fontSize: 10, color: 'var(--text-faint, #62655a)' }}>→ link</span>}
      </div>
      <div style={{ color: 'var(--text-dim, #9b9d8c)' }}>
        {entry.isDir ? '—' : formatSize(entry.size)}
      </div>
      <div style={{ color: 'var(--text-dim, #9b9d8c)' }}>
        {formatTime(entry.mtime)}
      </div>
      <div style={{ color: 'var(--text-faint, #62655a)', fontSize: 11 }}>
        {entry.kind}
        {entry.projectBadge && (
          <span style={{ marginLeft: 6, color: 'var(--accent, #cdf24b)' }}>
            [{entry.projectBadge}]
          </span>
        )}
      </div>
    </div>
  );
}
