'use client';

import { useState, useEffect, useRef } from 'react';
import { type FileEntry } from '@/types/file';
import { EXT_BADGES, KIND_COLORS, getBadgeExt } from '@/lib/file-badges';
import { getFileIcon, getIconColor, FbFolder, FbImage } from '@/lib/file-icons';
import { SPACING, FONT_SIZE, BORDER_RADIUS } from '@/lib/design-tokens';

interface FileCardProps {
  entry: FileEntry;
  onSelect: (entry: FileEntry) => void;
  onContextMenu?: (e: React.MouseEvent, entry: FileEntry) => void;
}

const BADGE_LABELS: Record<string, string> = {
  node: 'Node', web: 'Web', python: 'Py', rust: 'Rust', go: 'Go', git: 'Git',
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
  const [heat, setHeat] = useState(0);
  const [showRipple, setShowRipple] = useState(false);
  const cardRef = useRef<HTMLDivElement>(null);
  const heatDecayRef = useRef<ReturnType<typeof setTimeout> | null>(null);

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
    return () => window.removeEventListener('file-flash', handler);
  }, [entry.path]);

  const ext = !entry.isDir ? getBadgeExt(entry.name) : '';
  const extBadge = !entry.isDir ? EXT_BADGES[ext] : null;

  const renderThumbContent = () => {
    // Image thumbnail with broken-image fallback
    if (entry.kind === 'image' && !entry.isDir) {
      return (
        <ImageThumb
          src={`/api/fs/thumb?path=${encodeURIComponent(entry.path)}&w=160`}
          alt={entry.name}
        />
      );
    }

    // Folder icon (fanbox style)
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
          position: 'relative',
        }}>
          <Icon size={44} color={extBadge.bg} />
          <span style={{
            position: 'absolute', bottom: 2, right: 4,
            fontSize: 10, fontWeight: 700, fontFamily: 'var(--font-mono)',
            color: extBadge.fg, background: extBadge.bg,
            padding: '0 3px', borderRadius: 2, lineHeight: '13px',
          }}>
            {extBadge.label}
          </span>
        </span>
      );
    }

    // Fanbox-style icon based on file type
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
      onClick={() => onSelect(entry)}
      onContextMenu={(e) => onContextMenu?.(e, entry)}
      role="button"
      tabIndex={0}
      onKeyDown={(e) => { if (e.key === 'Enter') onSelect(entry); }}
      style={{
        padding: SPACING.md,
        borderRadius: 'var(--radius, 4px)',
        cursor: 'pointer',
        border: `1px solid ${isChanged ? 'var(--accent)' : 'var(--vibe-btn-border)'}`,
        background: flash ? 'var(--accent-soft)' : 'var(--vibe-toolbar-bg)',
        boxShadow: isChanged
          ? `0 0 calc(6px + 20px * ${heat}) color-mix(in srgb, var(--accent) calc(55% * ${heat}), transparent)`
          : 'none',
        transition: 'background 0.12s, border-color 0.12s, transform 0.12s, box-shadow 0.3s',
        position: 'relative',
        overflow: 'hidden',
        animation: isChanged ? 'changedBreath 2.2s ease-in-out infinite' : undefined,
      }}
      onMouseEnter={(e) => {
        (e.currentTarget as HTMLElement).style.background = 'var(--vibe-btn-bg)';
        (e.currentTarget as HTMLElement).style.borderColor = 'var(--accent)';
        (e.currentTarget as HTMLElement).style.transform = 'translateY(-1px)';
      }}
      onMouseLeave={(e) => {
        (e.currentTarget as HTMLElement).style.background = isChanged ? 'var(--accent-soft)' : 'var(--vibe-toolbar-bg)';
        (e.currentTarget as HTMLElement).style.borderColor = isChanged ? 'var(--accent)' : 'var(--vibe-btn-border)';
        (e.currentTarget as HTMLElement).style.transform = 'translateY(0)';
      }}
    >
      {/* Thumbnail / Icon */}
      <div style={{
        width: '100%', aspectRatio: '1', display: 'flex', alignItems: 'center', justifyContent: 'center',
        fontSize: 56, marginBottom: SPACING.sm, borderRadius: 'calc(var(--radius, 4px) - 2px)',
        background: 'var(--vibe-content-bg)', overflow: 'hidden', position: 'relative',
      }}>
        {renderThumbContent()}

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

      {/* Symlink indicator */}
      {entry.symlink && (
        <div style={{ fontSize: FONT_SIZE.xs, color: 'var(--vibe-btn-text)' }}>→ {entry.symlink}</div>
      )}

      {/* Badge */}
      {badge && badgeStyle && (
        <div style={{
          position: 'absolute', top: 8, right: 8, padding: '1px 5px', borderRadius: BORDER_RADIUS.sm,
          fontSize: 9, fontWeight: 600, background: badgeStyle.bg, color: badgeStyle.text, lineHeight: '16px',
        }}>
          {BADGE_LABELS[badge]}
        </div>
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

// ── Image Thumbnail with broken-image fallback ──

function ImageThumb({ src, alt }: { src: string; alt: string }) {
  const [error, setError] = useState(false);
  if (error) {
    return <FbImage size={64} />;
  }
  return (
    <img
      src={src}
      alt={alt}
      onError={() => setError(true)}
      style={{ width: '100%', height: '100%', objectFit: 'cover' }}
      loading="lazy"
    />
  );
}
