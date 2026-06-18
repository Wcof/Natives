'use client';

import { useState, useEffect, useRef } from 'react';
import { type FileEntry, type FileKind } from '@/types/file';
import { Folder, FileText, Video, Music, BookOpen, Archive, File } from 'lucide-react';

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

// ── Extension badge (shared with FileRow) ──

interface ExtBadge { label: string; bg: string; fg: string }

const EXT_BADGES: Record<string, ExtBadge> = {
  js: { label: 'JS', bg: '#F0DB4F', fg: '#1A1A1A' },
  jsx: { label: 'JSX', bg: '#61DAFB', fg: '#1A1A1A' },
  ts: { label: 'TS', bg: '#3178C6', fg: '#fff' },
  tsx: { label: 'TSX', bg: '#3178C6', fg: '#fff' },
  py: { label: 'PY', bg: '#3776AB', fg: '#FFE05B' },
  go: { label: 'GO', bg: '#00ACD7', fg: '#fff' },
  rs: { label: 'RS', bg: '#CE7B43', fg: '#fff' },
  java: { label: 'JV', bg: '#ED8B00', fg: '#fff' },
  rb: { label: 'RB', bg: '#CC342D', fg: '#fff' },
  sh: { label: '>_', bg: '#33373D', fg: '#3FD46A' },
  bash: { label: '>_', bg: '#33373D', fg: '#3FD46A' },
  json: { label: '{ }', bg: '#A6824C', fg: '#fff' },
  yml: { label: 'YML', bg: '#9C5BD6', fg: '#fff' },
  yaml: { label: 'YML', bg: '#9C5BD6', fg: '#fff' },
  toml: { label: 'TML', bg: '#9C4121', fg: '#fff' },
  html: { label: '<>', bg: '#E8662A', fg: '#fff' },
  css: { label: 'CSS', bg: '#2D6FD6', fg: '#fff' },
  scss: { label: 'SCS', bg: '#CF649A', fg: '#fff' },
  md: { label: 'MD', bg: '#3B82F6', fg: '#fff' },
  sql: { label: 'SQL', bg: '#C77D2E', fg: '#fff' },
  csv: { label: 'CSV', bg: '#1FAE5A', fg: '#fff' },
  txt: { label: 'TXT', bg: '#7A8290', fg: '#fff' },
  log: { label: 'LOG', bg: '#7A8290', fg: '#fff' },
  pdf: { label: 'PDF', bg: '#E64A3B', fg: '#fff' },
  svg: { label: 'SVG', bg: '#FFB13B', fg: '#fff' },
  env: { label: 'ENV', bg: '#ECD53F', fg: '#1A1A1A' },
};

const KIND_COLORS: Record<FileKind | 'dir', string> = {
  dir: '#6d8bff', text: '#9aa3b2', image: '#5bd6a0', video: '#9a7be8',
  audio: '#e85b9a', pdf: '#e85b5b', archive: '#e8c95b', other: '#7a8294',
};

function getExt(name: string): string {
  const i = name.lastIndexOf('.');
  return i > 0 ? name.slice(i + 1).toLowerCase() : '';
}

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
  }, [entry.path]);

  const ext = !entry.isDir ? getExt(entry.name) : '';
  const extBadge = !entry.isDir ? EXT_BADGES[ext] : null;

  const renderThumbContent = () => {
    // Image thumbnail
    if (entry.kind === 'image' && !entry.isDir) {
      return (
        <img
          src={`http://localhost:${window.__nativesHttpPort || 3001}/api/fs/thumb?path=${encodeURIComponent(entry.path)}&w=160`}
          alt={entry.name}
          style={{ width: '100%', height: '100%', objectFit: 'cover' }}
          loading="lazy"
        />
      );
    }

    // Folder icon
    if (entry.isDir) {
      return <Folder size={36} style={{ color: KIND_COLORS.dir }} />;
    }

    // Extension badge (TS, JS, PY, etc.)
    if (extBadge) {
      return (
        <span style={{
          display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
          width: 40, height: 40, borderRadius: 6,
          background: extBadge.bg, color: extBadge.fg,
          fontSize: 11, fontWeight: 700, fontFamily: 'var(--font-mono)',
        }}>
          {extBadge.label}
        </span>
      );
    }

    // Kind-based Lucide icon with color
    const iconProps = { size: 36, style: { color: KIND_COLORS[entry.kind] } };
    switch (entry.kind) {
      case 'text': return <FileText {...iconProps} />;
      case 'video': return <Video {...iconProps} />;
      case 'audio': return <Music {...iconProps} />;
      case 'pdf': return <BookOpen {...iconProps} />;
      case 'archive': return <Archive {...iconProps} />;
      default: return <File {...iconProps} />;
    }
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
        padding: 12,
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
        fontSize: 36, marginBottom: 8, borderRadius: 'calc(var(--radius, 4px) - 2px)',
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

      {/* File name */}
      <div style={{
        fontSize: 12, color: 'var(--vibe-brand-text)', lineHeight: 1.3,
        overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
      }}>
        {entry.name}
      </div>

      {/* Symlink indicator */}
      {entry.symlink && (
        <div style={{ fontSize: 10, color: 'var(--vibe-btn-text)' }}>→ {entry.symlink}</div>
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
