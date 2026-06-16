'use client';

import { useState, useEffect } from 'react';
import { type FileEntry } from '@/types/file';
import { Folder, FileText, Image as ImageIcon, Video, Music, BookOpen, Archive, File } from 'lucide-react';

interface FileRowProps {
  entry: FileEntry;
  onSelect: (entry: FileEntry) => void;
  onContextMenu?: (e: React.MouseEvent, entry: FileEntry) => void;
  showDir?: boolean;
}

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

export default function FileRow({ entry, onSelect, onContextMenu, showDir }: FileRowProps) {
  const [flash, setFlash] = useState(false);
  const renderIcon = () => {
    const size = 16;
    if (entry.isDir) return <Folder size={size} style={{ color: 'var(--accent,#FFF5E6)' }} />;
    switch (entry.kind) {
      case 'text': return <FileText size={size} />;
      case 'image': return <ImageIcon size={size} />;
      case 'video': return <Video size={size} />;
      case 'audio': return <Music size={size} />;
      case 'pdf': return <BookOpen size={size} />;
      case 'archive': return <Archive size={size} />;
      default: return <File size={size} />;
    }
  };

  useEffect(() => {
    const handler = (e: Event) => {
      const detail = (e as CustomEvent).detail;
      if (detail === entry.path) {
        setFlash(true);
        setTimeout(() => setFlash(false), 1200);
      }
    };
    window.addEventListener('file-flash', handler);
    return () => window.removeEventListener('file-flash', handler);
  }, [entry.path]);

  return (
    <div
      className={flash ? 'anim-liveZapRow' : ''}
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
        background: flash ? 'var(--accent-soft, #FFF5E61f)' : 'transparent',
        transition: 'background 0.12s',
      }}
      onMouseEnter={(e) => { (e.currentTarget as HTMLElement).style.background = 'var(--bg-2, #131410)'; }}
      onMouseLeave={(e) => { (e.currentTarget as HTMLElement).style.background = 'transparent'; }}
    >
      <span className="file-row-counter" />
      <span style={{ display: 'inline-flex', alignItems: 'center', justifyContent: 'center', color: 'var(--text-dim,#9b9d8c)' }}>
        {renderIcon()}
      </span>
      <div style={{
        overflow: 'hidden',
        textOverflow: 'ellipsis',
        whiteSpace: 'nowrap',
        display: 'flex',
        alignItems: 'center',
        gap: 6,
      }}>
        <span style={{ fontFamily: 'var(--font-mono)' }}>{entry.name}</span>
        {showDir && entry.dirHint && (
          <span style={{ fontSize: 10, color: 'var(--text-faint, #62655a)', marginLeft: 4, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', maxWidth: 200 }}>
            — {entry.dirHint}
          </span>
        )}
        {entry.hidden && <span style={{ fontSize: 10, color: 'var(--text-faint, #62655a)' }}>(hidden)</span>}
        {entry.symlink && <span style={{ fontSize: 10, color: 'var(--text-faint, #62655a)' }}>→ link</span>}
      </div>
      <div style={{ color: 'var(--text-dim, #9b9d8c)', fontFamily: 'var(--font-mono)', fontSize: 12 }}>
        {entry.isDir ? '—' : formatSize(entry.size)}
      </div>
      <div style={{ color: 'var(--text-dim, #9b9d8c)', fontFamily: 'var(--font-mono)', fontSize: 12 }}>
        {formatTime(entry.mtime)}
      </div>
      <div style={{ color: 'var(--text-faint, #62655a)', fontSize: 11, fontFamily: 'var(--font-mono)' }}>
        {entry.kind}
        {entry.projectBadge && (
          <span style={{ marginLeft: 6, color: 'var(--accent, #FFF5E6)' }}>
            [{entry.projectBadge}]
          </span>
        )}
      </div>
    </div>
  );
}
