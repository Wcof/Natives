'use client';

import { useEffect, useRef, useState, useCallback } from 'react';
import { type FileEntry } from '@/types/file';
import { t, type Locale } from '@/i18n';

type MenuMode = 'file' | 'dir' | 'blank';

interface ContextMenuItemBase {
  label: string;
  action: () => void;
  danger?: boolean;
}

interface FileContextMenuProps {
  entry?: FileEntry;
  x: number;
  y: number;
  mode: MenuMode;
  parentDir?: string;
  onClose: () => void;
  onOpen?: (entry: FileEntry) => void;
  onOpenInTerminal?: (dir: string) => void;
  onRevealInFinder?: (entry: FileEntry) => void;
  onOpenInEditor?: (entry: FileEntry) => void;
  onPreview?: (entry: FileEntry) => void;
  onDiskUsage?: (dir: string) => void;
  onRename?: (entry: FileEntry) => void;
  onTrash?: (entry: FileEntry) => void;
  onNewFile?: (parentDir: string) => void;
  onNewFolder?: (parentDir: string) => void;
  onFavorite?: (entry: FileEntry) => void;
  onUnfavorite?: (entry: FileEntry) => void;
  isFavorite?: boolean;
}

export default function FileContextMenu({
  entry, x, y, mode, parentDir, onClose,
  onOpen, onOpenInTerminal, onRevealInFinder, onOpenInEditor,
  onPreview, onDiskUsage, onRename, onTrash,
  onNewFile, onNewFolder, onFavorite, onUnfavorite,
  isFavorite,
}: FileContextMenuProps) {
  const ref = useRef<HTMLDivElement>(null);
  const [locale, setLocale] = useState<Locale>('zh');

  useEffect(() => {
    window.nativesAPI?.getLocale?.().then((l) => { if (l === 'en') setLocale('en'); }).catch(() => {});
  }, []);

  // Click outside → close
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

  // Wrap each action to also close the menu
  const mkItem = useCallback((item: ContextMenuItemBase): ContextMenuItemBase => ({
    ...item,
    action: () => { item.action(); onClose(); },
  }), [onClose]);

  const buildItems = (): (ContextMenuItemBase | 'sep')[] => {
    if (mode === 'blank') {
      return [
        mkItem({ label: t(locale, 'fileBrowser.newFile'), action: () => onNewFile?.(parentDir || '/') }),
        mkItem({ label: t(locale, 'fileBrowser.newFolder'), action: () => onNewFolder?.(parentDir || '/') }),
        'sep',
        mkItem({ label: t(locale, 'fileBrowser.diskUsage'), action: () => onDiskUsage?.(parentDir || '/') }),
      ];
    }

    if (!entry) return [];

    const p = entry.path;
    const parent = entry.isDir ? p : p.substring(0, p.lastIndexOf('/')) || '/';

    if (mode === 'dir') {
      return [
        mkItem({ label: t(locale, 'fileBrowser.open'), action: () => onOpen?.(entry) }),
        mkItem({ label: t(locale, 'fileBrowser.openInTerminal'), action: () => onOpenInTerminal?.(p) }),
        mkItem({ label: t(locale, 'fileBrowser.diskUsage'), action: () => onDiskUsage?.(p) }),
        mkItem({ label: t(locale, 'fileBrowser.revealInFinder'), action: () => onRevealInFinder?.(entry) }),
        'sep',
        mkItem({ label: t(locale, 'fileBrowser.copyPath'), action: () => { navigator.clipboard.writeText(p); } }),
        mkItem({ label: t(locale, 'fileBrowser.copyAsCd'), action: () => { navigator.clipboard.writeText(`cd "${p}"`); } }),
        'sep',
        mkItem({ label: t(locale, isFavorite ? 'fileBrowser.unfavorite' : 'fileBrowser.favorite'), action: () => {
          if (isFavorite) onUnfavorite?.(entry); else onFavorite?.(entry);
        }}),
        mkItem({ label: t(locale, 'fileBrowser.rename'), action: () => onRename?.(entry) }),
        mkItem({ label: t(locale, 'fileBrowser.newFile'), action: () => onNewFile?.(p) }),
        mkItem({ label: t(locale, 'fileBrowser.newFolder'), action: () => onNewFolder?.(p) }),
        mkItem({ label: t(locale, 'fileBrowser.moveToTrash'), action: () => onTrash?.(entry), danger: true }),
      ];
    }

    // file mode
    return [
      mkItem({ label: t(locale, 'fileBrowser.openInPreview'), action: () => onPreview?.(entry) }),
      mkItem({ label: t(locale, 'fileBrowser.openInEditor'), action: () => onOpenInEditor?.(entry) }),
      mkItem({ label: t(locale, 'fileBrowser.revealInFinder'), action: () => onRevealInFinder?.(entry) }),
      'sep',
      mkItem({ label: t(locale, 'fileBrowser.copyPath'), action: () => { navigator.clipboard.writeText(p); } }),
      mkItem({ label: t(locale, 'fileBrowser.copyAsCd'), action: () => { navigator.clipboard.writeText(`cd "${parent}"`); } }),
      'sep',
      mkItem({ label: t(locale, isFavorite ? 'fileBrowser.unfavorite' : 'fileBrowser.favorite'), action: () => {
        if (isFavorite) onUnfavorite?.(entry); else onFavorite?.(entry);
      }}),
      mkItem({ label: t(locale, 'fileBrowser.rename'), action: () => onRename?.(entry) }),
      mkItem({ label: t(locale, 'fileBrowser.moveToTrash'), action: () => onTrash?.(entry), danger: true }),
    ];
  };

  const items = buildItems();

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
      {items.map((item, idx) =>
        item === 'sep' ? (
          <div key={`sep-${idx}`} className="context-menu-divider" />
        ) : (
          <div
            key={`${item.label}-${idx}`}
            className={`context-menu-item ${item.danger ? 'danger' : ''}`}
            onClick={item.action}
          >
            {item.label}
          </div>
        )
      )}
    </div>
  );
}
