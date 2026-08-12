'use client';

import { useEffect, useRef } from 'react';
import type { SlashCommand } from '@/lib/assistant-slash';

interface SlashCommandPopoverProps {
  isOpen: boolean;
  query: string;
  /** Runtime-provided commands (Native currently always empty). */
  commands: SlashCommand[];
  selectedIndex: number;
  onSelect: (command: SlashCommand) => void;
  onHoverIndex: (index: number) => void;
  onClose: () => void;
  emptyMessage: string;
  headerLabel: string;
}

/**
 * Presentational slash menu.
 * Keyboard ownership lives in MessageInput's textarea onKeyDown —
 * this component must NOT register document-level keydown listeners.
 */
export default function SlashCommandPopover({
  isOpen,
  query,
  commands,
  selectedIndex,
  onSelect,
  onHoverIndex,
  onClose,
  emptyMessage,
  headerLabel,
}: SlashCommandPopoverProps) {
  const popoverRef = useRef<HTMLDivElement>(null);

  // Click outside to close (mousedown only — no keydown)
  useEffect(() => {
    if (!isOpen) return;
    const handleClick = (e: MouseEvent) => {
      if (popoverRef.current && !popoverRef.current.contains(e.target as Node)) {
        onClose();
      }
    };
    const timer = setTimeout(() => {
      document.addEventListener('mousedown', handleClick);
    }, 0);
    return () => {
      clearTimeout(timer);
      document.removeEventListener('mousedown', handleClick);
    };
  }, [isOpen, onClose]);

  if (!isOpen) return null;

  return (
    <div
      ref={popoverRef}
      data-slash-popover="1"
      data-slash-query={query}
      className="absolute bottom-full left-0 z-50 mb-2 min-w-[280px] max-w-[400px] rounded-xl border border-[var(--border)] bg-[var(--surface)] p-1.5 shadow-popup"
    >
      <div className="px-2.5 pb-1 pt-1 text-[0.625rem] font-semibold uppercase tracking-[0.06em] text-[var(--text-disabled)]">
        {headerLabel}
      </div>

      {commands.length === 0 ? (
        <div
          data-slash-empty="1"
          className="px-2.5 py-2 text-xs text-[var(--text-disabled)]"
        >
          {emptyMessage}
        </div>
      ) : (
        <div className="flex flex-col gap-0.5" role="listbox" aria-label={headerLabel}>
          {commands.map((cmd, index) => (
            <button
              key={cmd.id}
              type="button"
              role="option"
              aria-selected={index === selectedIndex}
              onClick={() => onSelect(cmd)}
              onMouseEnter={() => onHoverIndex(index)}
              className={`flex items-center gap-2.5 w-full rounded-lg px-2.5 py-2 text-left transition-[color,background-color,border-color,opacity,transform] ${
                index === selectedIndex
                  ? 'bg-[var(--primary-soft)] text-[var(--primary)]'
                  : 'text-[var(--text-secondary)] hover:bg-[var(--surface-hover)]'
              }`}
            >
              <span className="text-sm font-mono font-medium">{cmd.label}</span>
              <span className="text-[0.6875rem] text-[var(--text-disabled)] truncate">{cmd.description}</span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
