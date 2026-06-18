'use client';

import { useState, useEffect } from 'react';
import { type FileEntry } from '@/types/file';
import { Star } from 'lucide-react';
import { fmtSize, fmtTime } from '@/lib/format';
import { EXT_BADGES, getBadgeExt } from '@/lib/file-badges';
import { getFileIcon, getIconColor, FbFolder, FbImage } from '@/lib/file-icons';
import { SPACING, FONT_SIZE, BORDER_RADIUS } from '@/lib/design-tokens';

interface FileRowProps {
  entry: FileEntry;
  onSelect: (entry: FileEntry) => void;
  onContextMenu?: (e: React.MouseEvent, entry: FileEntry) => void;
  showDir?: boolean;
}

export default function FileRow({ entry, onSelect, onContextMenu, showDir }: FileRowProps) {
  const [flash, setFlash] = useState(false);

  useEffect(() => {
    const handler = (e: Event) => {
      if ((e as CustomEvent).detail === entry.path) {
        setFlash(true);
        setTimeout(() => setFlash(false), 1200);
      }
    };
    window.addEventListener('file-flash', handler);
    return () => window.removeEventListener('file-flash', handler);
  }, [entry.path]);

  const tint = getIconColor(entry);
  const ext = getBadgeExt(entry.name);
  const badge = entry.isDir ? null : EXT_BADGES[ext];

  const renderIcon = () => {
    // Folder: fanbox style
    if (entry.isDir) {
      return <FbFolder size={28} />;
    }
    // Image thumbnail with broken-image fallback
    if (entry.kind === 'image') {
      const thumbUrl = `/api/fs/thumb?path=${encodeURIComponent(entry.path)}&w=64`;
      return <ThumbImg src={thumbUrl} alt={entry.name} tint={tint} />;
    }
    // Extension badge + fanbox icon overlay (TS, JS, PY, etc.)
    if (badge) {
      const Icon = getFileIcon(entry);
      return (
        <span style={{
          display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
          width: 32, height: 32, borderRadius: BORDER_RADIUS.sm,
          background: badge.bg + '20', color: badge.bg,
          position: 'relative',
        }}>
          <Icon size={22} color={badge.bg} />
          <span style={{
            position: 'absolute', bottom: 0, right: 1,
            fontSize: FONT_SIZE.sm, fontWeight: 700, fontFamily: 'var(--font-mono)',
            color: badge.fg, background: badge.bg,
            padding: `0 ${SPACING.xs}px`, borderRadius: 1, lineHeight: '12px',
          }}>
            {badge.label}
          </span>
        </span>
      );
    }
    // Fanbox-style icon based on file type
    const Icon = getFileIcon(entry);
    return <Icon size={28} color={tint} />;
  };

  return (
    <div
      className={flash ? 'anim-liveZapRow' : ''}
      data-file-entry={entry.path}
      onClick={() => onSelect(entry)}
      onContextMenu={(e) => onContextMenu?.(e, entry)}
      role="button"
      tabIndex={0}
      onKeyDown={(e) => { if (e.key === 'Enter') onSelect(entry); }}
      style={{
        display: 'grid',
        gridTemplateColumns: '36px 1fr 120px 80px 24px',
        gap: SPACING.sm,
        padding: '6px 12px',
        alignItems: 'center',
        cursor: 'pointer',
        fontSize: FONT_SIZE.lg,
        color: 'var(--vibe-brand-text)',
        borderBottom: '1px solid var(--vibe-btn-border)',
        background: flash ? 'var(--accent-soft)' : 'transparent',
        transition: 'background 0.12s',
      }}
      onMouseEnter={(e) => { (e.currentTarget as HTMLElement).style.background = 'var(--vibe-toolbar-bg)'; }}
      onMouseLeave={(e) => { (e.currentTarget as HTMLElement).style.background = 'transparent'; }}
    >
      {/* Icon */}
      <span style={{ display: 'inline-flex', alignItems: 'center', justifyContent: 'center' }}>
        {renderIcon()}
      </span>

      {/* Name + badges */}
      <div style={{
        overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
        display: 'flex', alignItems: 'center', gap: SPACING.xs,
      }}>
        <span style={{ fontFamily: 'var(--font-mono)' }}>{entry.name}</span>
        {entry.projectBadge && (
          <span style={{
            fontSize: 8, fontWeight: 600, padding: '0 3px', borderRadius: 2,
            background: 'var(--accent-soft)', color: 'var(--accent)',
            lineHeight: '14px', textTransform: 'uppercase',
          }}>
            {entry.projectBadge}
          </span>
        )}
        {showDir && entry.dirHint && (
          <span style={{ fontSize: FONT_SIZE.xs, color: 'var(--vibe-btn-text)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', maxWidth: 200 }}>
            — {entry.dirHint}
          </span>
        )}
        {entry.hidden && <span style={{ fontSize: FONT_SIZE.xs, color: 'var(--vibe-btn-text)' }}>(hidden)</span>}
        {entry.symlink && <span style={{ fontSize: FONT_SIZE.xs, color: 'var(--vibe-btn-text)' }}>→ link</span>}
      </div>

      {/* Modified time (relative) */}
      <div style={{ color: 'var(--vibe-btn-text)', fontFamily: 'var(--font-mono)', fontSize: FONT_SIZE.md }}>
        {fmtTime(entry.mtime)}
      </div>

      {/* Size */}
      <div style={{ color: 'var(--vibe-btn-text)', fontFamily: 'var(--font-mono)', fontSize: FONT_SIZE.md }}>
        {entry.isDir ? '—' : fmtSize(entry.size)}
      </div>

      {/* Favorite star */}
      <span style={{ display: 'inline-flex', color: 'var(--vibe-btn-text)' }}>
        <Star size={12} />
      </span>
    </div>
  );
}

// ── Small thumbnail with broken-image fallback ──

function ThumbImg({ src, alt, tint }: { src: string; alt: string; tint: string }) {
  const [error, setError] = useState(false);
  if (error) {
    return <FbImage size={28} />;
  }
  return (
    <img
      src={src}
      alt={alt}
      onError={() => setError(true)}
      style={{
        width: 36, height: 36, borderRadius: 4,
        objectFit: 'cover', display: 'block',
      }}
    />
  );
}
