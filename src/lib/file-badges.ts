// ── Shared file extension badge system ──
// Used by FileCard and FileRow to display colored extension labels.

import type { FileKind } from '@/types/file';

export interface ExtBadge { label: string; bg: string; fg: string }

export const EXT_BADGES: Record<string, ExtBadge> = {
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

  // ── Archive formats — distinct per-type colors ──
  zip: { label: 'ZIP', bg: '#F0C75E', fg: '#1A1A1A' },
  rar: { label: 'RAR', bg: '#7B68EE', fg: '#fff' },
  '7z': { label: '7Z', bg: '#2F2F2F', fg: '#D4AF37' },
  tar: { label: 'TAR', bg: '#8B6914', fg: '#fff' },
  gz: { label: 'GZ', bg: '#5A8F3C', fg: '#fff' },
  bz2: { label: 'BZ2', bg: '#4A7A5A', fg: '#fff' },
  xz: { label: 'XZ', bg: '#3A6A7A', fg: '#fff' },
  tgz: { label: 'TGZ', bg: '#6B8E23', fg: '#fff' },
  tbz2: { label: 'TBZ', bg: '#4A7A5A', fg: '#fff' },
};

export const KIND_COLORS: Record<FileKind | 'dir', string> = {
  dir: '#6d8bff', text: '#9aa3b2', image: '#5bd6a0', video: '#9a7be8',
  audio: '#e85b9a', pdf: '#e85b5b', archive: '#e8c95b', other: '#7a8294',
};

export function getExt(name: string): string {
  const i = name.lastIndexOf('.');
  return i > 0 ? name.slice(i + 1).toLowerCase() : '';
}

/**
 * Get the full extension key for badge lookup.
 * Handles compound extensions like .tar.gz → 'tar.gz'.
 * Falls back to simple extension if no compound match.
 */
export function getBadgeExt(name: string): string {
  const simple = getExt(name);
  if (!simple) return '';
  // Check compound extension (e.g. file.tar.gz → 'tar.gz')
  const dot = name.lastIndexOf('.');
  if (dot > 0) {
    const secondDot = name.lastIndexOf('.', dot - 1);
    if (secondDot > 0) {
      const compound = name.slice(secondDot + 1).toLowerCase();
      if (compound in EXT_BADGES) return compound;
    }
  }
  return simple;
}
