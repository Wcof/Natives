'use client';

import { useState, useEffect, useRef } from 'react';
import { type FileEntry } from '@/types/file';
import { Star, Play } from 'lucide-react';
import { fmtSize, fmtTime } from '@/lib/format';
import { EXT_BADGES, getBadgeExt } from '@/lib/file-badges';
import { getFileIcon, getIconColor, FbFolder, FbImage } from '@/lib/file-icons';
import { SPACING, FONT_SIZE, BORDER_RADIUS } from '@/lib/design-tokens';

interface FileRowProps {
  entry: FileEntry;
  onSelect: (entry: FileEntry, e?: { shiftKey?: boolean; metaKey?: boolean; ctrlKey?: boolean }) => void;
  onContextMenu?: (e: React.MouseEvent, entry: FileEntry) => void;
  showDir?: boolean;
  selected?: boolean;
  onDoubleClick?: () => void;
  isFavorite?: boolean;
  onFavoriteToggle?: (entry: FileEntry) => void;
  dimmed?: boolean;
  onMoveDrop?: (sourcePaths: string[], destDir: string) => void;
  dragPaths?: string[];
  flash?: boolean;
}

export default function FileRow({ entry, onSelect, onContextMenu, showDir, selected, onDoubleClick, isFavorite, onFavoriteToggle, dimmed, onMoveDrop, dragPaths, flash = false }: FileRowProps) {
  const [dropTarget, setDropTarget] = useState(false);
  const clickTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => () => {
    if (clickTimerRef.current) clearTimeout(clickTimerRef.current);
  }, []);

  const tint = getIconColor(entry);
  const ext = getBadgeExt(entry.name);
  const badge = entry.isDir ? null : EXT_BADGES[ext];

  const renderIcon = () => {
    // Folder: Natives2 style
    if (entry.isDir) {
      return <FbFolder size={28} />;
    }
    // Extension badge + Natives2 icon overlay (TS, JS, PY, etc.)
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
    // Natives2-style icon based on file type
    const Icon = getFileIcon(entry);
    return <Icon size={28} color={tint} />;
  };

  return (
    <div
      className={flash ? 'anim-liveZapRow' : ''}
      data-file-entry={entry.path}
      onClick={(ev) => {
        if (clickTimerRef.current) clearTimeout(clickTimerRef.current);
        const mods = { shiftKey: ev.shiftKey, metaKey: ev.metaKey, ctrlKey: ev.ctrlKey };
        if (mods.shiftKey || mods.metaKey || mods.ctrlKey) {
          onSelect(entry, mods);
          return;
        }
        clickTimerRef.current = setTimeout(() => {
          onSelect(entry, mods);
          clickTimerRef.current = null;
        }, 200);
      }}
      onDoubleClick={() => {
        if (clickTimerRef.current) {
          clearTimeout(clickTimerRef.current);
          clickTimerRef.current = null;
        }
        onDoubleClick?.();
      }}
      onContextMenu={(e) => onContextMenu?.(e, entry)}
      draggable
      onDragStart={(e) => {
        const paths = dragPaths && dragPaths.length > 0 ? dragPaths : [entry.path];
        e.dataTransfer.setData('application/x-natives-paths', JSON.stringify(paths));
        e.dataTransfer.effectAllowed = 'move';
      }}
      onDragOver={(e) => {
        if (!entry.isDir) return;
        e.preventDefault();
        e.dataTransfer.dropEffect = 'move';
        if (!dropTarget) setDropTarget(true);
      }}
      onDragLeave={() => { if (dropTarget) setDropTarget(false); }}
      onDrop={(e) => {
        if (!entry.isDir || !onMoveDrop) return;
        e.preventDefault();
        e.stopPropagation();
        setDropTarget(false);
        try {
          const raw = e.dataTransfer.getData('application/x-natives-paths');
          const paths = raw ? JSON.parse(raw) as string[] : [];
          const filtered = paths.filter(p => p !== entry.path);
          if (filtered.length) onMoveDrop(filtered, entry.path);
        } catch { /* ignore */ }
      }}
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
        color: 'var(--text)',
        borderBottom: '1px solid var(--border)',
        background: dropTarget ? 'var(--accent-soft, rgba(205,242,75,0.12))' : selected ? 'var(--primary-soft)' : flash ? 'var(--primary-soft)' : 'transparent',
        outline: dropTarget || selected ? '1px solid var(--primary)' : 'none',
        outlineOffset: -1,
        transition: 'background 0.12s, opacity 0.12s',
        opacity: dimmed ? 0.45 : entry.hidden ? 0.5 : 1,
      }}
      onMouseEnter={(e) => { (e.currentTarget as HTMLElement).style.background = 'var(--surface)'; }}
      onMouseLeave={(e) => { (e.currentTarget as HTMLElement).style.background = selected ? 'var(--primary-soft)' : 'transparent'; }}
    >
      {/* Icon */}
      <span style={{ display: 'inline-flex', alignItems: 'center', justifyContent: 'center', position: 'relative' }}>
        {renderIcon()}
        {/* Video play badge — small overlay */}
        {entry.kind === 'video' && !entry.isDir && (
          <span style={{
            position: 'absolute', left: '50%', top: '50%', transform: 'translate(-50%, -50%)',
            width: 18, height: 18, borderRadius: '50%',
            background: 'rgba(0,0,0,0.6)', display: 'flex', alignItems: 'center', justifyContent: 'center',
            pointerEvents: 'none',
          }}>
            <Play size={8} fill="#fff" color="#fff" />
          </span>
        )}
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
            background: 'var(--primary-soft)', color: 'var(--primary)',
            lineHeight: '14px', textTransform: 'uppercase',
          }}>
            {entry.projectBadge}
          </span>
        )}
        {showDir && entry.dirHint && (
          <span style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-secondary)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', maxWidth: 200 }}>
            — {entry.dirHint}
          </span>
        )}
        {entry.hidden && <span style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-secondary)' }}>(hidden)</span>}
        {entry.symlink && <span style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-secondary)' }}>→ link</span>}
      </div>

      {/* Modified time (relative) */}
      <div style={{ color: 'var(--text-secondary)', fontFamily: 'var(--font-mono)', fontSize: FONT_SIZE.md }}>
        {fmtTime(entry.mtime)}
      </div>

      {/* Size */}
      <div style={{ color: 'var(--text-secondary)', fontFamily: 'var(--font-mono)', fontSize: FONT_SIZE.md }}>
        {entry.isDir ? '—' : fmtSize(entry.size)}
      </div>

      {/* Favorite star */}
      <button
        type="button"
        onClick={(e) => {
          e.stopPropagation();
          onFavoriteToggle?.(entry);
        }}
        title={isFavorite ? '取消收藏' : '收藏'}
        style={{
          display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
          color: isFavorite ? 'var(--yellow, #e3b341)' : 'var(--text-disabled)',
          cursor: 'pointer', background: 'none', border: 'none', padding: 0,
          transition: 'color 0.12s, transform 0.12s',
          opacity: isFavorite ? 1 : 0.5,
        }}
        onMouseEnter={(e) => { (e.currentTarget as HTMLElement).style.opacity = '1'; (e.currentTarget as HTMLElement).style.transform = 'scale(1.2)'; }}
        onMouseLeave={(e) => { (e.currentTarget as HTMLElement).style.opacity = isFavorite ? '1' : '0.5'; (e.currentTarget as HTMLElement).style.transform = 'scale(1)'; }}
      >
        <Star size={12} fill={isFavorite ? 'currentColor' : 'none'} />
      </button>
    </div>
  );
}
