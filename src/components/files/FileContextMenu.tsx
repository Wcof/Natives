'use client';

import { useEffect, useRef, useState, useCallback } from 'react';
import { createPortal } from 'react-dom';
import { type FileEntry } from '@/types/file';
import { t, type Locale } from '@/i18n';
import { useHydrated } from '@/hooks/useHydrated';

type MenuMode = 'file' | 'dir' | 'blank';

interface ContextMenuItemBase {
  label: string;
  action: () => void;
  danger?: boolean;
  shortcut?: string;
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
  onEditImage?: (entry: FileEntry) => void;
  isFavorite?: boolean;
}

export default function FileContextMenu({
  entry, x, y, mode, parentDir, onClose,
  onOpen, onOpenInTerminal, onRevealInFinder, onOpenInEditor,
  onPreview, onDiskUsage, onRename, onTrash,
  onNewFile, onNewFolder, onFavorite, onUnfavorite,
  isFavorite, onEditImage,
}: FileContextMenuProps) {
  const ref = useRef<HTMLDivElement>(null);
  const [locale, setLocale] = useState<Locale>('zh');

  useEffect(() => {
    window.nativesAPI?.getLocale?.().then((l) => { if (l === 'en') setLocale('en'); }).catch(() => {});
  }, []);

  // Click outside + Escape → close
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

  // Viewport clamping — reposition if menu overflows any edge
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const rect = el.getBoundingClientRect();
    const MARGIN = 8;
    let menuTop = y;
    let menuLeft = x;

    // Try to read any previously clamped position
    const curTop = el.style.top ? parseFloat(el.style.top) : NaN;
    const curLeft = el.style.left ? parseFloat(el.style.left) : NaN;
    if (!isNaN(curTop)) menuTop = curTop;
    if (!isNaN(curLeft)) menuLeft = curLeft;

    // Right overflow → flip left
    if (menuLeft + rect.width > window.innerWidth - MARGIN) {
      menuLeft = Math.max(MARGIN, window.innerWidth - rect.width - MARGIN);
    }
    // Left overflow → flip right
    if (menuLeft < MARGIN) {
      menuLeft = MARGIN;
    }
    // Bottom overflow → flip up
    if (menuTop + rect.height > window.innerHeight - MARGIN) {
      menuTop = Math.max(MARGIN, window.innerHeight - rect.height - MARGIN);
    }
    // Top overflow → flip down
    if (menuTop < MARGIN) {
      menuTop = MARGIN;
    }

    el.style.left = `${Math.round(menuLeft)}px`;
    el.style.top = `${Math.round(menuTop)}px`;
  }, [x, y]);

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
        mkItem({ label: t(locale, 'fileBrowser.open'), action: () => onOpen?.(entry), shortcut: '↵' }),
        mkItem({ label: t(locale, 'fileBrowser.openInTerminal'), action: () => onOpenInTerminal?.(p) }),
        mkItem({ label: t(locale, 'fileBrowser.diskUsage'), action: () => onDiskUsage?.(p) }),
        mkItem({ label: t(locale, 'fileBrowser.revealInFinder'), action: () => onRevealInFinder?.(entry) }),
        'sep',
        mkItem({ label: t(locale, 'fileBrowser.copyPath'), action: () => { navigator.clipboard.writeText(p); } }),
        'sep',
        mkItem({ label: t(locale, isFavorite ? 'fileBrowser.unfavorite' : 'fileBrowser.favorite'), action: () => {
          if (isFavorite) onUnfavorite?.(entry); else onFavorite?.(entry);
        }, shortcut: 'Space' }),
        mkItem({ label: t(locale, 'fileBrowser.rename'), action: () => onRename?.(entry), shortcut: t(locale, 'fileBrowser.shortcutRename') }),
        mkItem({ label: t(locale, 'fileBrowser.newFile'), action: () => onNewFile?.(p) }),
        mkItem({ label: t(locale, 'fileBrowser.newFolder'), action: () => onNewFolder?.(p) }),
        mkItem({ label: t(locale, 'fileBrowser.moveToTrash'), action: () => onTrash?.(entry), danger: true, shortcut: t(locale, 'fileBrowser.shortcutTrash') }),
      ];
    }

    // file mode
    const items: (ContextMenuItemBase | 'sep')[] = [
      mkItem({ label: t(locale, 'fileBrowser.openInPreview'), action: () => onPreview?.(entry), shortcut: '↵' }),
      mkItem({ label: t(locale, 'fileBrowser.openInEditor'), action: () => onOpenInEditor?.(entry), shortcut: t(locale, 'fileBrowser.shortcutOpenInEditor') }),
    ];
    if (entry.kind === 'image') {
      items.push(mkItem({ label: t(locale, 'fileBrowser.editImage'), action: () => onEditImage?.(entry) }));
    }
    items.push(
      mkItem({ label: t(locale, 'fileBrowser.revealInFinder'), action: () => onRevealInFinder?.(entry) }),
      'sep',
      mkItem({ label: t(locale, 'fileBrowser.copyPath'), action: () => { navigator.clipboard.writeText(p); } }),
      'sep',
      mkItem({ label: t(locale, isFavorite ? 'fileBrowser.unfavorite' : 'fileBrowser.favorite'), action: () => {
        if (isFavorite) onUnfavorite?.(entry); else onFavorite?.(entry);
      }, shortcut: 'Space' }),
      mkItem({ label: t(locale, 'fileBrowser.rename'), action: () => onRename?.(entry), shortcut: t(locale, 'fileBrowser.shortcutRename') }),
      mkItem({ label: t(locale, 'fileBrowser.moveToTrash'), action: () => onTrash?.(entry), danger: true, shortcut: t(locale, 'fileBrowser.shortcutTrash') }),
    );
    return items;
  };

  const items = buildItems();

  const menuContent = (
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
            style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 16 }}
          >
            <span>{item.label}</span>
            {item.shortcut && (
              <span style={{
                fontSize: 11, fontFamily: 'var(--font-mono)', color: 'var(--vibe-btn-text)',
                opacity: 0.6, flexShrink: 0,
              }}>
                {item.shortcut}
              </span>
            )}
          </div>
        )
      )}
    </div>
  );

  const mounted = useHydrated();
  

  if (!mounted) return null;
  return createPortal(menuContent, document.body);
}
