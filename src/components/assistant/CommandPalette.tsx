'use client';

import { useEffect, useMemo, useState } from 'react';
import { t } from '@/i18n';

export interface AssistantCommand {
  id: string;
  label: string;
  description?: string;
  shortcut?: string;
  disabledReason?: string;
  run: () => void;
}

interface CommandPaletteProps {
  open: boolean;
  onClose: () => void;
  commands: AssistantCommand[];
  locale: string;
}

export default function CommandPalette({ open, onClose, commands, locale }: CommandPaletteProps) {
  const [query, setQuery] = useState('');
  const [index, setIndex] = useState(0);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return commands;
    return commands.filter(
      (c) =>
        c.label.toLowerCase().includes(q) ||
        c.id.toLowerCase().includes(q) ||
        (c.description ?? '').toLowerCase().includes(q),
    );
  }, [commands, query]);

  useEffect(() => {
    if (!open) {
      setQuery('');
      setIndex(0);
    }
  }, [open]);

  useEffect(() => {
    setIndex(0);
  }, [query]);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        onClose();
      } else if (e.key === 'ArrowDown') {
        e.preventDefault();
        setIndex((i) => Math.min(i + 1, Math.max(0, filtered.length - 1)));
      } else if (e.key === 'ArrowUp') {
        e.preventDefault();
        setIndex((i) => Math.max(0, i - 1));
      } else if (e.key === 'Enter') {
        e.preventDefault();
        const cmd = filtered[index];
        if (cmd && !cmd.disabledReason) {
          cmd.run();
          onClose();
        }
      }
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, [open, filtered, index, onClose]);

  if (!open) return null;

  return (
    <div
      className="fixed inset-0 z-[80] flex items-start justify-center bg-[var(--overlay)] pt-[12vh]"
      role="dialog"
      aria-modal="true"
      aria-label={t(locale, 'assistantCommandPalette.title')}
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="w-full max-w-lg overflow-hidden rounded-xl border border-[var(--border)] bg-[var(--surface)] shadow-2xl">
        <input
          autoFocus
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder={t(locale, 'assistantCommandPalette.searchPlaceholder')}
          className="w-full border-b border-[var(--border)] bg-transparent px-4 py-3 text-sm outline-none"
        />
        <ul className="max-h-80 overflow-y-auto py-1">
          {filtered.length === 0 && (
            <li className="px-4 py-6 text-center text-xs text-[var(--text-disabled)]">
              {t(locale, 'assistantCommandPalette.noMatch')}
            </li>
          )}
          {filtered.map((cmd, i) => (
            <li key={cmd.id}>
              <button
                type="button"
                disabled={Boolean(cmd.disabledReason)}
                onClick={() => {
                  if (cmd.disabledReason) return;
                  cmd.run();
                  onClose();
                }}
                className={`flex w-full items-center gap-3 px-4 py-2 text-left text-sm ${
                  i === index ? 'bg-[var(--surface-hover)]' : ''
                } ${cmd.disabledReason ? 'opacity-50' : 'hover:bg-[var(--surface-hover)]'}`}
              >
                <div className="min-w-0 flex-1">
                  <div className="truncate font-medium">{cmd.label}</div>
                  {(cmd.description || cmd.disabledReason) && (
                    <div className="truncate text-[11px] text-[var(--text-disabled)]">
                      {cmd.disabledReason ?? cmd.description}
                    </div>
                  )}
                </div>
                {cmd.shortcut && (
                  <kbd className="rounded border border-[var(--border)] px-1.5 py-0.5 text-[10px] text-[var(--text-disabled)]">
                    {cmd.shortcut}
                  </kbd>
                )}
              </button>
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}
