'use client';

import { useEffect, useRef } from 'react';
import { type FileEntry } from '@/types/file';

interface FileContextMenuProps {
  entry: FileEntry;
  x: number;
  y: number;
  onClose: () => void;
}

export default function FileContextMenu({ entry, x, y, onClose }: FileContextMenuProps) {
  const ref = useRef<HTMLDivElement>(null);

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

  const items = [
    { label: 'Open', action: () => { /* handled by parent */ } },
    entry.isDir ? null : { label: 'Open With...', action: () => {} },
    { label: 'Rename', action: () => {} },
    { label: 'Move to Trash', action: () => {}, danger: true },
    null, // divider
    { label: 'Copy Path', action: () => { navigator.clipboard.writeText(entry.path); } },
    entry.isDir ? { label: 'Copy as Terminal cd', action: () => { navigator.clipboard.writeText(`cd ${entry.path}`); } } : null,
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
        minWidth: 160,
      }}
    >
      {items.map((item: any, idx) =>
        item === null ? (
          <div key={`div-${idx}`} className="context-menu-divider" />
        ) : (
          <div
            key={item.label}
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
