'use client';

import { useState, useEffect, useRef } from 'react';
import { Star, Play } from 'lucide-react';
import { type FileEntry } from '@/types/file';
import { EXT_BADGES, KIND_COLORS, getBadgeExt } from '@/lib/file-badges';
import { getFileIcon, getIconColor, FbFolder, FbImage } from '@/lib/file-icons';
import { useThumbnail } from '@/lib/use-thumbnail';
import { SPACING, FONT_SIZE, BORDER_RADIUS } from '@/lib/design-tokens';

interface FileCardProps {
  entry: FileEntry;
  onSelect: (entry: FileEntry) => void;
  onContextMenu?: (e: React.MouseEvent, entry: FileEntry) => void;
  selected?: boolean;
  onDoubleClick?: () => void;
  isFavorite?: boolean;
  onFavoriteToggle?: (entry: FileEntry) => void;
}

const BADGE_LABELS: Record<string, string> = {
  node: 'node', web: 'web', python: 'py', rust: 'rs', go: 'go', git: 'git',
};

const BADGE_COLORS: Record<string, { bg: string; text: string; border: string }> = {
  node: { bg: '#3fb95018', text: '#3fb950', border: '#3fb95066' },
  web: { bg: 'var(--accent-soft)', text: 'var(--accent)', border: 'color-mix(in srgb, var(--accent) 50%, transparent)' },
  python: { bg: '#4b8bbe18', text: '#4b8bbe', border: '#4b8bbe66' },
  rust: { bg: '#d2691e18', text: '#d2691e', border: '#d2691e66' },
  go: { bg: '#00add818', text: '#00add8', border: '#00add866' },
  git: { bg: 'transparent', text: 'var(--text-dim)', border: 'var(--border)' },
};

export default function FileCard({ entry, onSelect, onContextMenu, selected, onDoubleClick, isFavorite, onFavoriteToggle }: FileCardProps) {
  const [flash, setFlash] = useState(false);
  const [heat, setHeat] = useState(0);
  const [showRipple, setShowRipple] = useState(false);
  const cardRef = useRef<HTMLDivElement>(null);
  const heatDecayRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const clickTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    const handler = (e: Event) => {
      const detail = (e as CustomEvent).detail;
      if (detail === entry.path) {
        // Increment heat (0 → 1 max)
        setHeat(prev => Math.min(1, prev + 0.15));
        setFlash(true);
        setShowRipple(true);

        // Clear flash after animation
        setTimeout(() => setFlash(false), 1200);
        // Clear ripple after animation
        setTimeout(() => setShowRipple(false), 800);

        // Decay heat over time
        if (heatDecayRef.current) clearTimeout(heatDecayRef.current);
        heatDecayRef.current = setTimeout(() => setHeat(0), 8000);
      }
    };
    window.addEventListener('file-flash', handler);
    return () => {
      window.removeEventListener('file-flash', handler);
      if (clickTimerRef.current) clearTimeout(clickTimerRef.current);
    };
  }, [entry.path]);

  const ext = !entry.isDir ? getBadgeExt(entry.name) : '';
  const extBadge = !entry.isDir ? EXT_BADGES[ext] : null;

  const renderThumbContent = () => {
    // Image thumbnail with broken-image fallback
    if (entry.kind === 'image' && !entry.isDir) {
      return <ImageThumb entry={entry} size={256} />;
    }

    // Folder icon (Natives2 style)
    if (entry.isDir) {
      return <FbFolder size={64} />;
    }

    // Extension badge (TS, JS, PY, etc.)
    if (extBadge) {
      const Icon = getFileIcon(entry);
      return (
        <span style={{
          display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
          width: 60, height: 60, borderRadius: BORDER_RADIUS.md,
          background: extBadge.bg + '20', color: extBadge.bg,
        }}>
          <Icon size={44} color={extBadge.bg} />
        </span>
      );
    }

    // Natives2-style icon based on file type
    const Icon = getFileIcon(entry);
    const iconColor = getIconColor(entry);
    return <Icon size={64} color={iconColor} />;
  };

  const badge = entry.projectBadge;
  const badgeStyle = badge ? BADGE_COLORS[badge] : null;
  const isChanged = heat > 0;

  return (
    <div
      ref={cardRef}
      className={'file-card' + (flash ? ' anim-liveZap' : '') + (isChanged ? ' changed' : '')}
      data-file-entry={entry.path}
      data-heat={heat.toFixed(2)}
      onClick={() => {
        // Delay single-click to distinguish from double-click
        if (clickTimerRef.current) clearTimeout(clickTimerRef.current);
        clickTimerRef.current = setTimeout(() => {
          onSelect(entry);
          clickTimerRef.current = null;
        }, 200);
      }}
      onDoubleClick={() => {
        // Cancel pending single-click
        if (clickTimerRef.current) {
          clearTimeout(clickTimerRef.current);
          clickTimerRef.current = null;
        }
        onDoubleClick?.();
      }}
      onContextMenu={(e) => onContextMenu?.(e, entry)}
      role="button"
      tabIndex={0}
      onKeyDown={(e) => { if (e.key === 'Enter') onSelect(entry); }}
      style={{
        padding: SPACING.md,
        borderRadius: 'var(--radius, 4px)',
        cursor: 'pointer',
        border: `1px solid ${selected ? 'var(--accent)' : isChanged ? 'var(--accent)' : 'var(--vibe-btn-border)'}`,
        background: selected ? 'var(--accent-soft)' : flash ? 'var(--accent-soft)' : 'transparent',
        boxShadow: isChanged
          ? `0 0 calc(6px + 20px * ${heat}) color-mix(in srgb, var(--accent) calc(55% * ${heat}), transparent)`
          : selected ? '0 0 0 1px var(--accent)' : 'none',
        transition: 'background 0.12s, border-color 0.12s, transform 0.12s, box-shadow 0.3s, opacity 0.12s',
        position: 'relative',
        overflow: 'hidden',
        opacity: entry.hidden ? 0.5 : 1,
        animation: isChanged ? 'changedBreath 2.2s ease-in-out infinite' : undefined,
      }}
      onMouseEnter={(e) => {
        (e.currentTarget as HTMLElement).style.background = 'var(--vibe-btn-bg)';
        (e.currentTarget as HTMLElement).style.borderColor = 'var(--accent)';
        (e.currentTarget as HTMLElement).style.transform = 'translateY(-1px)';
      }}
      onMouseLeave={(e) => {
        (e.currentTarget as HTMLElement).style.background = selected ? 'var(--accent-soft)' : isChanged ? 'var(--accent-soft)' : 'transparent';
        (e.currentTarget as HTMLElement).style.borderColor = selected ? 'var(--accent)' : isChanged ? 'var(--accent)' : 'var(--vibe-btn-border)';
        (e.currentTarget as HTMLElement).style.transform = 'translateY(0)';
      }}
    >
      {/* Thumbnail / Icon */}
      <div className="file-card-thumb" style={{
        width: '100%', aspectRatio: '1', display: 'flex', alignItems: 'center', justifyContent: 'center',
        fontSize: 56, marginBottom: SPACING.sm, borderRadius: 'calc(var(--radius, 4px) - 2px)',
        background: 'var(--vibe-content-bg)', overflow: 'hidden', position: 'relative',
      }}>
        {renderThumbContent()}

        {/* Combined badges — top-right of icon area: project type + file extension */}
        {(badge && badgeStyle || extBadge) && (
          <div style={{
            position: 'absolute', top: 4, right: 4, zIndex: 2,
            display: 'flex', flexDirection: 'column', gap: 3,
          }}>
            {badge && badgeStyle && (
              <div style={{
                fontFamily: 'var(--font-mono)', fontSize: 9, fontWeight: 700,
                lineHeight: 1, letterSpacing: '0.02em', textTransform: 'lowercase',
                padding: '2px 4px', borderRadius: 4,
                background: 'color-mix(in srgb, var(--vibe-content-bg) 78%, transparent)',
                backdropFilter: 'blur(3px)',
                WebkitBackdropFilter: 'blur(3px)',
                color: badgeStyle.text, border: `1px solid ${badgeStyle.border}`,
                alignSelf: 'flex-end',
              }}>
                {BADGE_LABELS[badge]}
              </div>
            )}
            {extBadge && (
              <div style={{
                fontFamily: 'var(--font-mono)', fontSize: 9, fontWeight: 700,
                lineHeight: 1, padding: '2px 4px', borderRadius: 4,
                color: extBadge.fg, background: extBadge.bg,
                alignSelf: 'flex-end',
              }}>
                {extBadge.label}
              </div>
            )}
          </div>
        )}

        {/* Video play badge — centered frosted circle */}
        {entry.kind === 'video' && !entry.isDir && (
          <span style={{
            position: 'absolute', left: '50%', top: '50%', transform: 'translate(-50%, -50%)',
            width: 34, height: 34, borderRadius: '50%',
            background: 'rgba(0,0,0,0.5)', display: 'flex', alignItems: 'center', justifyContent: 'center',
            backdropFilter: 'blur(2px)', pointerEvents: 'none',
          }}>
            <Play size={16} fill="#fff" color="#fff" />
          </span>
        )}

        {/* Edit ripple — expanding ring from icon center */}
        {showRipple && (
          <span style={{
            position: 'absolute', inset: 0, borderRadius: '50%',
            border: '2px solid var(--accent)',
            animation: 'editRipple 0.8s ease-out forwards',
            pointerEvents: 'none',
          }} />
        )}
      </div>

      {/* File name — up to 2 lines */}
      <div style={{
        fontSize: FONT_SIZE.md, color: 'var(--vibe-brand-text)', lineHeight: 1.35,
        display: '-webkit-box', WebkitLineClamp: 2, WebkitBoxOrient: 'vertical',
        overflow: 'hidden',
      }}>
        {entry.name}
      </div>

      {/* Favorite star — below filename, hidden until hover */}
      <button
        type="button"
        className="file-card-fav"
        data-fav={isFavorite ? 'on' : 'off'}
        onClick={(e) => {
          e.stopPropagation();
          onFavoriteToggle?.(entry);
        }}
        title={isFavorite ? '取消收藏' : '收藏'}
        style={{
          position: 'absolute', top: 6, right: 6,
          color: isFavorite ? 'var(--yellow, #e3b341)' : 'var(--text-faint)',
          cursor: 'pointer',
          background: 'none', border: 'none', padding: 0,
          lineHeight: 0,
        }}
      >
        <Star size={15} fill={isFavorite ? 'currentColor' : 'none'} />
      </button>

      {/* Symlink indicator */}
      {entry.symlink && (
        <div style={{ fontSize: FONT_SIZE.xs, color: 'var(--vibe-btn-text)' }}>→ {entry.symlink}</div>
      )}

      {/* Changed count badge — shows when heat > 0 */}
      {isChanged && (
        <div style={{
          position: 'absolute', top: 6, left: 6,
          width: 16, height: 16, borderRadius: '50%',
          background: 'var(--accent)', color: 'var(--vibe-content-bg)',
          fontSize: 9, fontWeight: 700, display: 'flex', alignItems: 'center', justifyContent: 'center',
          animation: 'changedPulse 0.5s ease-out',
        }}>
          {Math.ceil(heat / 0.15)}
        </div>
      )}

      {/* Hidden file dot */}
      {entry.hidden && (
        <div style={{
          position: 'absolute', bottom: 8, right: 8,
          width: 6, height: 6, borderRadius: '50%', background: 'var(--vibe-btn-text)',
        }} />
      )}
    </div>
  );
}

// ── Image Thumbnail with loading + error states ──

function ImageThumb({ entry, size }: { entry: FileEntry; size: number }) {
  const { dataUrl, loading, error } = useThumbnail(entry.path, size);

  if (loading) {
    // 加载中显示文件图标，避免空白闪烁
    const Icon = getFileIcon(entry);
    const tint = getIconColor(entry);
    // eslint-disable-next-line react-hooks/static-components
    return <Icon size={size} color={tint} />;
  }
  if (error || !dataUrl) {
    return <FbImage size={size} />;
  }
  return (
    <img
      src={dataUrl}
      alt={entry.name}
      style={{ width: '100%', height: '100%', objectFit: 'cover' }}
      loading="lazy"
    />
  );
}