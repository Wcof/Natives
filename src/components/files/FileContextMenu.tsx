'use client';

import { useEffect, useRef, useState } from 'react';
import { type FileEntry } from '@/types/file';
import { t, type Locale } from '@/i18n';

interface FileContextMenuProps {
  entry: FileEntry;
  x: number;
  y: number;
  onClose: () => void;
  onOpen?: (entry: FileEntry) => void;
  onRename?: (entry: FileEntry) => void;
  onTrash?: (entry: FileEntry) => void;
  onNewFile?: (parentDir: string) => void;
  onNewFolder?: (parentDir: string) => void;
}

export default function FileContextMenu({
  entry, x, y, onClose,
  onOpen, onRename, onTrash, onNewFile, onNewFolder,
}: FileContextMenuProps) {
  const ref = useRef<HTMLDivElement>(null);
  const [locale, setLocale] = useState<Locale>('zh');

  useEffect(() => {
    window.nativesAPI?.getLocale?.().then((l) => { if (l === 'en') setLocale('en'); }).catch(() => {});
  }, []);

  useEffect(() => {
    const handleClick = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        onClose();
      }
    };
    const handleEsc = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    document.addEventListener('mousedown', handleClick);
    document.addEventListener('keydown', handleEsc);
    return () => {
      document.removeEventListener('mousedown', handleClick);
      document.removeEventListener('keydown', handleEsc);
    };
  }, [onClose]);

  const parentDir = entry.isDir ? entry.path : entry.path.substring(0, entry.path.lastIndexOf('/')) || '/';

  const items = [
    { label: t(locale, 'fileBrowser.open'), action: () => onOpen?.(entry) },
    entry.isDir ? null : { label: t(locale, 'fileBrowser.openWith'), action: () => onOpen?.(entry) },
    null, // divider
    { label: t(locale, 'fileBrowser.rename'), action: () => onRename?.(entry) },
    { label: t(locale, 'fileBrowser.moveToTrash'), action: () => onTrash?.(entry), danger: true },
    null, // divider
    entry.isDir ? { label: t(locale, 'fileBrowser.newFile'), action: () => onNewFile?.(entry.path) } : null,
    entry.isDir ? { label: t(locale, 'fileBrowser.newFolder'), action: () => onNewFolder?.(entry.path) } : null,
    entry.isDir ? null : { label: t(locale, 'fileBrowser.newFile'), action: () => onNewFile?.(parentDir) },
    entry.isDir ? null : { label: t(locale, 'fileBrowser.newFolder'), action: () => onNewFolder?.(parentDir) },
    null, // divider
    { label: t(locale, 'fileBrowser.copyPath'), action: () => { navigator.clipboard.writeText(entry.path); } },
    entry.isDir ? { label: t(locale, 'fileBrowser.copyAsCd'), action: () => { navigator.clipboard.writeText(`cd "${entry.path}"`); } } : null,
  ].filter(Boolean);

  return (
    <div
      ref={ref}
      className="context-menu"
      style={{
        position: 'fixed',
        top: y,
        left: x,
        zIndex: 1000,
        minWidth: 168,
      }}
    >
      {items.map((item: { label: string; action: () => void; danger?: boolean } | null, idx) =>
        item === null ? (
          <div key={`div-${idx}`} className="context-menu-divider" />
        ) : (
          <div
            key={`${item.label}-${idx}`}
            className={`context-menu-item ${item.danger ? 'danger' : ''}`}
            onClick={() => { item.action(); onClose(); }}
          >
            {item.label}
          </div>
        )
      )}
    </div>
  );
}
