'use client';

import { useState, useEffect, useRef } from 'react';
import { type FileEntry } from '@/types/file';
import { Folder, FileText, Image as ImageIcon, Video, Music, BookOpen, Archive, File } from 'lucide-react';

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
    return () => {
      window.removeEventListener('file-flash', handler);
      if (heatDecayRef.current) clearTimeout(heatDecayRef.current);
    };
  }, [entry.path]);

  const renderIcon = () => {
    const size = 36;
    if (entry.isDir) return <Folder size={size} style={{ color: 'var(--accent,#cdf24b)' }} />;
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

  const badge = entry.projectBadge;
  const badgeStyle = badge ? BADGE_COLORS[badge] : null;
  const isChanged = heat > 0;

  return (
    <div
      ref={cardRef}
      className={'file-card' + (flash ? ' anim-liveZap' : '') + (isChanged ? ' changed' : '')}
      data-heat={heat.toFixed(2)}
      onClick={() => onSelect(entry)}
      onContextMenu={(e) => onContextMenu?.(e, entry)}
      role="button"
      tabIndex={0}
      onKeyDown={(e) => { if (e.key === 'Enter') onSelect(entry); }}
      style={{
        padding: 12,
        borderRadius: 'var(--radius, 4px)',
        cursor: 'pointer',
        border: `1px solid ${isChanged ? 'var(--accent, #cdf24b)' : 'var(--border, #262920)'}`,
        background: flash ? 'var(--accent-soft, #cdf24b1f)' : 'var(--bg-2, #131410)',
        boxShadow: isChanged
          ? `0 0 calc(6px + 20px * ${heat}) color-mix(in srgb, var(--accent, #cdf24b) calc(55% * ${heat}), transparent)`
          : 'none',
        transition: 'background 0.12s, border-color 0.12s, transform 0.12s, box-shadow 0.3s',
        position: 'relative',
        overflow: 'hidden',
        animation: isChanged ? 'changedBreath 2.2s ease-in-out infinite' : undefined,
      }}
      onMouseEnter={(e) => {
        (e.currentTarget as HTMLElement).style.background = 'var(--bg-3, #1c1e17)';
        (e.currentTarget as HTMLElement).style.borderColor = 'var(--accent, #cdf24b)';
        (e.currentTarget as HTMLElement).style.transform = 'translateY(-1px)';
      }}
      onMouseLeave={(e) => {
        (e.currentTarget as HTMLElement).style.background = isChanged ? 'var(--accent-soft, #cdf24b1f)' : 'var(--bg-2, #131410)';
        (e.currentTarget as HTMLElement).style.borderColor = isChanged ? 'var(--accent, #cdf24b)' : 'var(--border, #262920)';
        (e.currentTarget as HTMLElement).style.transform = 'translateY(0)';
      }}
    >
      {/* Thumbnail / Icon */}
      <div style={{
        width: '100%', aspectRatio: '1', display: 'flex', alignItems: 'center', justifyContent: 'center',
        fontSize: 36, marginBottom: 8, borderRadius: 'calc(var(--radius, 4px) - 2px)',
        background: 'var(--bg, #0b0c0a)', overflow: 'hidden', position: 'relative',
      }}>
        {entry.kind === 'image' && !entry.isDir ? (
          <img
            src={`http://localhost:${window.__nativesHttpPort || 3001}/api/fs/thumb?path=${encodeURIComponent(entry.path)}&w=160`}
            alt={entry.name}
            style={{ width: '100%', height: '100%', objectFit: 'cover' }}
            loading="lazy"
          />
        ) : (
          <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', color: 'var(--text-dim,#9b9d8c)' }}>
            {renderIcon()}
          </div>
        )}

        {/* Edit ripple — expanding ring from icon center */}
        {showRipple && (
          <span style={{
            position: 'absolute', inset: 0, borderRadius: '50%',
            border: '2px solid var(--accent, #cdf24b)',
            animation: 'editRipple 0.8s ease-out forwards',
            pointerEvents: 'none',
          }} />
        )}
      </div>

      {/* File name */}
      <div style={{
        fontSize: 12, color: 'var(--text, #f2f2ea)', lineHeight: 1.3,
        overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
      }}>
        {entry.name}
      </div>

      {/* Symlink indicator */}
      {entry.symlink && (
        <div style={{ fontSize: 10, color: 'var(--text-faint, #62655a)' }}>→ {entry.symlink}</div>
      )}

      {/* Badge */}
      {badge && badgeStyle && (
        <div style={{
          position: 'absolute', top: 8, right: 8, padding: '1px 5px', borderRadius: 3,
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
          background: 'var(--accent, #cdf24b)', color: 'var(--bg, #0b0c0a)',
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
          width: 6, height: 6, borderRadius: '50%', background: 'var(--text-faint, #62655a)',
        }} />
      )}
    </div>
  );
}
