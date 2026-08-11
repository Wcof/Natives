'use client';

import { useEffect, useRef, useState, useCallback } from 'react';
import { createPortal } from 'react-dom';
import { type FileEntry } from '@/types/file';
import { t, type Locale } from '@/i18n';
import { useHydrated } from '@/hooks/useHydrated';
import { isArchiveFile } from '@/lib/follow-mode';

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
  onDuplicate?: (entry: FileEntry) => void;
  onCopy?: (entry: FileEntry) => void;
  onCut?: (entry: FileEntry) => void;
  onPaste?: () => void;
  canPaste?: boolean;
  onCopyPath?: (entry: FileEntry) => void;
  onCopyImage?: (entry: FileEntry) => void;
  onOpenDefault?: (entry: FileEntry) => void;
  onNewFile?: (parentDir: string) => void;
  onNewFolder?: (parentDir: string) => void;
  onFavorite?: (entry: FileEntry) => void;
  onUnfavorite?: (entry: FileEntry) => void;
  onEditImage?: (entry: FileEntry) => void;
  /** 解压压缩包到所在目录（W7；仅 archive kind 显示） */
  onExtract?: (entry: FileEntry) => void;
  /** 压缩为 zip（多选时 FileBrowser 侧按选中集打包） */
  onCompress?: (entry: FileEntry) => void;
  isFavorite?: boolean;
}

export default function FileContextMenu({
  entry, x, y, mode, parentDir, onClose,
  onOpen, onOpenInTerminal, onRevealInFinder, onOpenInEditor,
  onPreview, onDiskUsage, onRename, onTrash,
  onDuplicate, onCopy, onCut, onPaste, canPaste, onCopyPath, onCopyImage, onOpenDefault,
  onNewFile, onNewFolder, onFavorite, onUnfavorite,
  isFavorite, onEditImage, onExtract, onCompress,
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
      const items: (ContextMenuItemBase | 'sep')[] = [
        mkItem({ label: t(locale, 'fileBrowser.newFile'), action: () => onNewFile?.(parentDir || '/') }),
        mkItem({ label: t(locale, 'fileBrowser.newFolder'), action: () => onNewFolder?.(parentDir || '/') }),
      ];
      if (canPaste) {
        items.push(mkItem({ label: t(locale, 'fileBrowser.paste'), action: () => onPaste?.(), shortcut: '⌘V' }));
      }
      items.push('sep');
      items.push(mkItem({ label: t(locale, 'fileBrowser.diskUsage'), action: () => onDiskUsage?.(parentDir || '/') }));
      return items;
    }

    if (!entry) return [];

    const p = entry.path;

    if (mode === 'dir') {
      return [
        mkItem({ label: t(locale, 'fileBrowser.open'), action: () => onOpen?.(entry), shortcut: '↵' }),
        mkItem({ label: t(locale, 'fileBrowser.openInTerminal'), action: () => onOpenInTerminal?.(p) }),
        mkItem({ label: t(locale, 'fileBrowser.diskUsage'), action: () => onDiskUsage?.(p) }),
        mkItem({ label: t(locale, 'fileBrowser.revealInFinder'), action: () => onRevealInFinder?.(entry) }),
        'sep',
        mkItem({ label: t(locale, 'fileBrowser.copyPath'), action: () => { if (onCopyPath) onCopyPath(entry); else navigator.clipboard.writeText(p); } }),
        mkItem({ label: t(locale, 'fileBrowser.duplicate'), action: () => onDuplicate?.(entry), shortcut: '⌘D' }),
        mkItem({ label: t(locale, 'fileBrowser.compressZip'), action: () => onCompress?.(entry) }),
        mkItem({ label: t(locale, 'fileBrowser.copy'), action: () => onCopy?.(entry), shortcut: '⌘C' }),
        mkItem({ label: t(locale, 'fileBrowser.cut'), action: () => onCut?.(entry), shortcut: '⌘X' }),
        ...(canPaste ? [mkItem({ label: t(locale, 'fileBrowser.paste'), action: () => onPaste?.(), shortcut: '⌘V' })] : []),
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
      mkItem({ label: t(locale, 'fileBrowser.openDefault'), action: () => onOpenDefault?.(entry) }),
    ];
    if (entry.kind === 'image') {
      items.push(mkItem({ label: t(locale, 'fileBrowser.editImage'), action: () => onEditImage?.(entry) }));
      // 后端 fs_clipboard_copy_image 仅实现了 macOS（osascript）；
      // 其余平台隐藏菜单项，不给用户一个必然报错的入口
      const isMac = typeof navigator !== 'undefined' && /mac/i.test(navigator.platform || navigator.userAgent);
      if (isMac) {
        items.push(mkItem({ label: t(locale, 'fileBrowser.copyImage'), action: () => onCopyImage?.(entry) }));
      }
    }
    if (isArchiveFile(entry.name)) {
      items.push(mkItem({ label: t(locale, 'fileBrowser.extractHere'), action: () => onExtract?.(entry) }));
    }
    items.push(
      mkItem({ label: t(locale, 'fileBrowser.revealInFinder'), action: () => onRevealInFinder?.(entry) }),
      'sep',
      mkItem({ label: t(locale, 'fileBrowser.copyPath'), action: () => { if (onCopyPath) onCopyPath(entry); else navigator.clipboard.writeText(p); } }),
      mkItem({ label: t(locale, 'fileBrowser.duplicate'), action: () => onDuplicate?.(entry), shortcut: '⌘D' }),
      mkItem({ label: t(locale, 'fileBrowser.compressZip'), action: () => onCompress?.(entry) }),
      mkItem({ label: t(locale, 'fileBrowser.copy'), action: () => onCopy?.(entry), shortcut: '⌘C' }),
      mkItem({ label: t(locale, 'fileBrowser.cut'), action: () => onCut?.(entry), shortcut: '⌘X' }),
    );
    if (canPaste) {
      items.push(mkItem({ label: t(locale, 'fileBrowser.paste'), action: () => onPaste?.(), shortcut: '⌘V' }));
    }
    items.push(
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
            role="menuitem"
            tabIndex={0}
            onClick={item.action}
            onKeyDown={(e) => {
              if (e.key === 'Enter' || e.key === ' ') {
                e.preventDefault();
                item.action();
              }
            }}
            style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 16 }}
          >
            <span>{item.label}</span>
            {item.shortcut && (
              <span style={{
                fontSize: 11, fontFamily: 'var(--font-mono)', color: 'var(--text-secondary)',
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
