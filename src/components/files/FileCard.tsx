'use client';

import { useState, useEffect } from 'react';
import { type FileEntry } from '@/types/file';

interface FileCardProps {
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

const BADGE_LABELS: Record<string, string> = {
  node: 'Node',
  web: 'Web',
  python: 'Py',
  rust: 'Rust',
  go: 'Go',
  git: 'Git',
};

const BADGE_COLORS: Record<string, { bg: string; text: string }> = {
  node: { bg: '#33993320', text: '#339933' },
  web: { bg: '#E44D2620', text: '#E44D26' },
  python: { bg: '#3776AB20', text: '#3776AB' },
  rust: { bg: '#DEA58420', text: '#DEA584' },
  go: { bg: '#00ADD820', text: '#00ADD8' },
  git: { bg: '#F0503320', text: '#F05033' },
};

export default function FileCard({ entry, onSelect, onContextMenu }: FileCardProps) {
  const [flash, setFlash] = useState(false);

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

  const icon = entry.isDir ? '📁' : FILE_TYPE_ICONS[entry.kind] || '📄';
  const badge = entry.projectBadge;
  const badgeStyle = badge ? BADGE_COLORS[badge] : null;

  return (
    <div
      className={'file-card' + (flash ? ' anim-liveZap' : '')}
      onClick={() => onSelect(entry)}
      onContextMenu={(e) => onContextMenu?.(e, entry)}
      role="button"
      tabIndex={0}
      onKeyDown={(e) => { if (e.key === 'Enter') onSelect(entry); }}
      style={{
        padding: 12,
        borderRadius: 'var(--radius, 4px)',
        cursor: 'pointer',
        border: '1px solid var(--border, #262920)',
        background: flash ? 'var(--accent-soft, #cdf24b1f)' : 'var(--bg-2, #131410)',
        boxShadow: flash ? '0 0 12px 4px var(--accent-soft, #cdf24b33)' : 'none',
        transition: 'background 0.12s, border-color 0.12s, transform 0.12s, box-shadow 0.12s',
        position: 'relative',
        overflow: 'hidden',
      }}
      onMouseEnter={(e) => {
        (e.currentTarget as HTMLElement).style.background = 'var(--bg-3, #1c1e17)';
        (e.currentTarget as HTMLElement).style.borderColor = 'var(--accent, #cdf24b)';
        (e.currentTarget as HTMLElement).style.transform = 'translateY(-1px)';
      }}
      onMouseLeave={(e) => {
        (e.currentTarget as HTMLElement).style.background = 'var(--bg-2, #131410)';
        (e.currentTarget as HTMLElement).style.borderColor = 'var(--border, #262920)';
        (e.currentTarget as HTMLElement).style.transform = 'translateY(0)';
      }}
    >
      {/* Thumbnail / Icon */}
      <div style={{
        width: '100%',
        aspectRatio: '1',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        fontSize: 36,
        marginBottom: 8,
        borderRadius: 'calc(var(--radius, 4px) - 2px)',
        background: 'var(--bg, #0b0c0a)',
        overflow: 'hidden',
      }}>
        {entry.kind === 'image' && !entry.isDir ? (
          <img
            src={`http://localhost:${window.__nativesHttpPort || 3001}/api/fs/thumb?path=${encodeURIComponent(entry.path)}&w=160`}
            alt={entry.name}
            style={{ width: '100%', height: '100%', objectFit: 'cover' }}
            loading="lazy"
          />
        ) : (
          <span>{icon}</span>
        )}
      </div>

      {/* File name */}
      <div style={{
        fontSize: 12,
        color: 'var(--text, #f2f2ea)',
        lineHeight: 1.3,
        overflow: 'hidden',
        textOverflow: 'ellipsis',
        whiteSpace: 'nowrap',
      }}>
        {entry.name}
      </div>

      {/* Symlink indicator */}
      {entry.symlink && (
        <div style={{ fontSize: 10, color: 'var(--text-faint, #62655a)' }}>
          → {entry.symlink}
        </div>
      )}

      {/* Badge */}
      {badge && badgeStyle && (
        <div style={{
          position: 'absolute',
          top: 8,
          right: 8,
          padding: '1px 5px',
          borderRadius: 3,
          fontSize: 9,
          fontWeight: 600,
          background: badgeStyle.bg,
          color: badgeStyle.text,
          lineHeight: '16px',
        }}>
          {BADGE_LABELS[badge]}
        </div>
      )}

      {/* Hidden file dot */}
      {entry.hidden && (
        <div style={{
          position: 'absolute',
          top: 8,
          left: 8,
          width: 6,
          height: 6,
          borderRadius: '50%',
          background: 'var(--text-faint, #62655a)',
        }} />
      )}
    </div>
  );
}
