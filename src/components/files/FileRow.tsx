'use client';

import { useState, useEffect } from 'react';
import { type FileEntry, type FileKind } from '@/types/file';
import { Folder, File, Star } from 'lucide-react';
import { fmtSize, fmtTime } from '@/lib/format';

interface FileRowProps {
  entry: FileEntry;
  onSelect: (entry: FileEntry) => void;
  onContextMenu?: (e: React.MouseEvent, entry: FileEntry) => void;
  showDir?: boolean;
}

// ── Extension badge (shared with FileCard) ──

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

function getIconColor(entry: FileEntry): string {
  if (entry.isDir) return KIND_COLORS.dir;
  const ext = getExt(entry.name);
  if (EXT_BADGES[ext]) return EXT_BADGES[ext].bg;
  return KIND_COLORS[entry.kind];
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
  const ext = getExt(entry.name);
  const badge = entry.isDir ? null : EXT_BADGES[ext];

  const renderIcon = () => {
    if (entry.isDir) {
      return <Folder size={16} style={{ color: tint }} />;
    }
    // Image thumbnail
    if (entry.kind === 'image') {
      const port = window.__nativesHttpPort || 3001;
      const thumbUrl = `http://localhost:${port}/api/fs/thumb?path=${encodeURIComponent(entry.path)}&w=48`;
      return (
        <img
          src={thumbUrl}
          alt={entry.name}
          style={{
            width: 24, height: 24, borderRadius: 2,
            objectFit: 'cover', display: 'block',
          }}
        />
      );
    }
    if (badge) {
      return (
        <span style={{
          display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
          width: 20, height: 20, borderRadius: 3,
          background: badge.bg, color: badge.fg,
          fontSize: 7, fontWeight: 700, fontFamily: 'var(--font-mono)',
        }}>
          {badge.label}
        </span>
      );
    }
    return <File size={16} style={{ color: tint }} />;
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
        gridTemplateColumns: '24px 1fr 120px 80px 24px',
        gap: 8,
        padding: '6px 12px',
        alignItems: 'center',
        cursor: 'pointer',
        fontSize: 13,
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
        display: 'flex', alignItems: 'center', gap: 6,
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
          <span style={{ fontSize: 10, color: 'var(--vibe-btn-text)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', maxWidth: 200 }}>
            — {entry.dirHint}
          </span>
        )}
        {entry.hidden && <span style={{ fontSize: 10, color: 'var(--vibe-btn-text)' }}>(hidden)</span>}
        {entry.symlink && <span style={{ fontSize: 10, color: 'var(--vibe-btn-text)' }}>→ link</span>}
      </div>

      {/* Modified time (relative) */}
      <div style={{ color: 'var(--vibe-btn-text)', fontFamily: 'var(--font-mono)', fontSize: 12 }}>
        {fmtTime(entry.mtime)}
      </div>

      {/* Size */}
      <div style={{ color: 'var(--vibe-btn-text)', fontFamily: 'var(--font-mono)', fontSize: 12 }}>
        {entry.isDir ? '—' : fmtSize(entry.size)}
      </div>

      {/* Favorite star */}
      <span style={{ display: 'inline-flex', color: 'var(--vibe-btn-text)' }}>
        <Star size={12} />
      </span>
    </div>
  );
}
