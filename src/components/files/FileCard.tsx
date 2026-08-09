'use client';

import { useState, useEffect, useRef } from 'react';
import { Star, Play } from 'lucide-react';
import { type FileEntry } from '@/types/file';
import { t } from '@/i18n';
import { EXT_BADGES, getBadgeExt } from '@/lib/file-badges';
import { getFileIcon, getIconColor, FbFolder, FbImage } from '@/lib/file-icons';
import { useThumbnail } from '@/lib/use-thumbnail';
import { SPACING, FONT_SIZE, BORDER_RADIUS } from '@/lib/design-tokens';

interface FileCardProps {
  entry: FileEntry;
  locale: string;
  onSelect: (entry: FileEntry, e?: { shiftKey?: boolean; metaKey?: boolean; ctrlKey?: boolean }) => void;
  onContextMenu?: (e: React.MouseEvent, entry: FileEntry) => void;
  selected?: boolean;
  onDoubleClick?: () => void;
  isFavorite?: boolean;
  onFavoriteToggle?: (entry: FileEntry) => void;
  dimmed?: boolean;
  onMoveDrop?: (sourcePaths: string[], destDir: string) => void;
  dragPaths?: string[];
  flash?: boolean;
}

const BADGE_LABELS: Record<string, string> = {
  node: 'node', web: 'web', python: 'py', rust: 'rs', go: 'go', git: 'git',
};

const BADGE_COLORS: Record<string, { bg: string; text: string; border: string }> = {
  node: { bg: 'color-mix(in srgb, var(--lang-node) 10%, transparent)', text: 'var(--lang-node)', border: 'color-mix(in srgb, var(--lang-node) 40%, transparent)' },
  web: { bg: 'var(--primary-soft)', text: 'var(--primary)', border: 'color-mix(in srgb, var(--primary) 50%, transparent)' },
  python: { bg: 'color-mix(in srgb, var(--lang-python) 10%, transparent)', text: 'var(--lang-python)', border: 'color-mix(in srgb, var(--lang-python) 40%, transparent)' },
  rust: { bg: 'color-mix(in srgb, var(--lang-rust) 10%, transparent)', text: 'var(--lang-rust)', border: 'color-mix(in srgb, var(--lang-rust) 40%, transparent)' },
  go: { bg: 'color-mix(in srgb, var(--lang-go) 10%, transparent)', text: 'var(--lang-go)', border: 'color-mix(in srgb, var(--lang-go) 40%, transparent)' },
  git: { bg: 'transparent', text: 'var(--text-secondary)', border: 'var(--border)' },
};

export default function FileCard({ entry, locale, onSelect, onContextMenu, selected, onDoubleClick, isFavorite, onFavoriteToggle, dimmed, onMoveDrop, dragPaths, flash = false }: FileCardProps) {
  const [dropTarget, setDropTarget] = useState(false);
  const [heat, setHeat] = useState(0);
  const [showRipple, setShowRipple] = useState(false);
  const isChanged = heat > 0;
  const cardRef = useRef<HTMLDivElement>(null);
  const clickTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const heatDecayRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const rippleTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    if (!flash) return;
    setHeat((prev) => Math.min(1, prev + 0.15));
    setShowRipple(true);
    if (rippleTimerRef.current) clearTimeout(rippleTimerRef.current);
    rippleTimerRef.current = setTimeout(() => setShowRipple(false), 800);
    if (heatDecayRef.current) clearTimeout(heatDecayRef.current);
    heatDecayRef.current = setTimeout(() => setHeat(0), 8000);
  }, [flash]);

  useEffect(() => () => {
    if (clickTimerRef.current) clearTimeout(clickTimerRef.current);
    if (rippleTimerRef.current) clearTimeout(rippleTimerRef.current);
    if (heatDecayRef.current) clearTimeout(heatDecayRef.current);
  }, []);

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

  return (
    <div
      ref={cardRef}
      className={'file-card' + (flash ? ' anim-liveZap' : '')}
      data-file-entry={entry.path}
      data-heat={heat.toFixed(2)}
      onClick={(ev) => {
        // 立即选择（不再因可能双击而固定延迟 200ms；p95≤100ms 目标）。
        // 双击时第一次 click 的即时选择本来就是 Finder 的正常反馈。
        if (clickTimerRef.current) clearTimeout(clickTimerRef.current);
        const mods = { shiftKey: ev.shiftKey, metaKey: ev.metaKey, ctrlKey: ev.ctrlKey };
        onSelect(entry, mods);
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
        padding: SPACING.md,
        borderRadius: 'var(--radius, 4px)',
        cursor: 'pointer',
        border: `1px solid ${dropTarget ? 'var(--primary)' : selected ? 'var(--primary)' : isChanged ? 'var(--primary)' : 'var(--border)'}`,
        background: dropTarget ? 'var(--accent-soft)' : selected ? 'var(--primary-soft)' : flash ? 'var(--primary-soft)' : 'transparent',
        boxShadow: isChanged
          ? `0 0 calc(6px + 20px * ${heat}) color-mix(in srgb, var(--primary) calc(55% * ${heat}), transparent)`
          : selected ? '0 0 0 1px var(--primary)' : 'none',
        transition: 'background 0.12s, border-color 0.12s, transform 0.12s, box-shadow 0.3s, opacity 0.12s',
        position: 'relative',
        overflow: 'hidden',
        opacity: dimmed ? 0.45 : entry.hidden ? 0.5 : 1,
        animation: isChanged ? 'changedBreath 2.2s ease-in-out infinite' : undefined,
      }}
      onMouseEnter={(e) => {
        (e.currentTarget as HTMLElement).style.background = 'var(--surface)';
        (e.currentTarget as HTMLElement).style.borderColor = 'var(--primary)';
        (e.currentTarget as HTMLElement).style.transform = 'translateY(-1px)';
      }}
      onMouseLeave={(e) => {
        (e.currentTarget as HTMLElement).style.background = selected ? 'var(--primary-soft)' : isChanged ? 'var(--primary-soft)' : 'transparent';
        (e.currentTarget as HTMLElement).style.borderColor = selected ? 'var(--primary)' : isChanged ? 'var(--primary)' : 'var(--border)';
        (e.currentTarget as HTMLElement).style.transform = 'translateY(0)';
      }}
    >
      {/* Thumbnail / Icon */}
      <div className="file-card-thumb" style={{
        width: '100%', aspectRatio: '1', display: 'flex', alignItems: 'center', justifyContent: 'center',
        fontSize: 56, marginBottom: SPACING.sm, borderRadius: 'calc(var(--radius, 4px) - 2px)',
        background: 'var(--surface)', overflow: 'hidden', position: 'relative',
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
                background: 'var(--surface)',
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
            background: 'var(--overlay)', display: 'flex', alignItems: 'center', justifyContent: 'center',
            pointerEvents: 'none',
          }}>
            <Play size={16} fill="var(--neutral-1000)" color="var(--neutral-1000)" />
          </span>
        )}

        {/* Edit ripple — expanding ring from icon center */}
      </div>

      {/* File name — up to 2 lines */}
      <div style={{
        fontSize: FONT_SIZE.md, color: 'var(--text)', lineHeight: 1.35,
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
        title={isFavorite ? t(locale, 'fileBrowser.unfavorite') : t(locale, 'fileBrowser.favorite')}
        style={{
          position: 'absolute', top: 6, right: 6,
          color: isFavorite ? 'var(--warning)' : 'var(--text-disabled)',
          cursor: 'pointer',
          background: 'none', border: 'none', padding: 0,
          lineHeight: 0,
        }}
      >
        <Star size={15} fill={isFavorite ? 'currentColor' : 'none'} />
      </button>

      {/* Symlink indicator */}
      {entry.symlink && (
        <div style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-secondary)' }}>→ {entry.symlink}</div>
      )}

      {showRipple && <span style={{ position: 'absolute', inset: 0, borderRadius: '50%', border: '2px solid var(--primary)', animation: 'editRipple 0.8s ease-out forwards', pointerEvents: 'none' }} />}

      {/* Changed count badge — shows when heat > 0 */}
      {isChanged && (
        <div style={{
          position: 'absolute', top: 6, left: 6,
          width: 16, height: 16, borderRadius: '50%',
          background: 'var(--primary)', color: 'var(--surface)',
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
          width: 6, height: 6, borderRadius: '50%', background: 'var(--text-secondary)',
        }} />
      )}
    </div>
  );
}

// ── Image Thumbnail with loading + error states ──

function ImageThumb({ entry, size }: { entry: FileEntry; size: number }) {
  const hostRef = useRef<HTMLDivElement>(null);
  const [visible, setVisible] = useState(false);
  useEffect(() => {
    if (!hostRef.current || typeof IntersectionObserver === 'undefined') {
      setVisible(true);
      return;
    }
    const observer = new IntersectionObserver(([item]) => setVisible(Boolean(item?.isIntersecting)), { rootMargin: '240px' });
    observer.observe(hostRef.current);
    return () => observer.disconnect();
  }, []);
  const { dataUrl, loading, error } = useThumbnail(entry.path, size, visible);

  if (!visible || loading) {
    // 加载中显示文件图标，避免空白闪烁
    const Icon = getFileIcon(entry);
    const tint = getIconColor(entry);
    // eslint-disable-next-line react-hooks/static-components
    return <div ref={hostRef}><Icon size={size} color={tint} /></div>;
  }
  if (error || !dataUrl) {
    return <div ref={hostRef}><FbImage size={size} /></div>;
  }
  return <div ref={hostRef}>
    <img src={dataUrl} alt={entry.name} style={{ width: '100%', height: '100%', objectFit: 'cover' }} loading="lazy" />
  </div>;
}
