'use client';

import { useState, useRef, useEffect, useCallback } from 'react';

interface SlashCommand {
  id: string;
  label: string;
  description: string;
  category: 'system' | 'skill' | 'mcp';
  icon?: string;
}

interface SlashCommandPopoverProps {
  isOpen: boolean;
  query: string;
  onSelect: (command: SlashCommand) => void;
  onClose: () => void;
  disabled?: boolean;
}

const SYSTEM_COMMANDS: SlashCommand[] = [
  { id: '/create-app', label: '/create-app', description: 'Create a new module from description', category: 'system' },
  { id: '/modify-app', label: '/modify-app', description: 'Modify an existing module', category: 'system' },
  { id: '/list-apps', label: '/list-apps', description: 'List all installed modules', category: 'system' },
  { id: '/uninstall-app', label: '/uninstall-app', description: 'Uninstall a module', category: 'system' },
];

export default function SlashCommandPopover({
  isOpen,
  query,
  onSelect,
  onClose,
  disabled = false,
}: SlashCommandPopoverProps) {
  const [selectedIndex, setSelectedIndex] = useState(0);
  const popoverRef = useRef<HTMLDivElement>(null);

  const filteredCommands = SYSTEM_COMMANDS.filter((cmd) =>
    cmd.id.toLowerCase().includes(query.toLowerCase())
  );

  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      if (!isOpen) return;

      if (filteredCommands.length === 0) {
        if (e.key === 'Escape') {
          e.preventDefault();
          onClose();
        }
        return;
      }

      switch (e.key) {
        case 'ArrowDown':
          e.preventDefault();
          setSelectedIndex((prev) => (prev + 1) % filteredCommands.length);
          break;
        case 'ArrowUp':
          e.preventDefault();
          setSelectedIndex((prev) => (prev - 1 + filteredCommands.length) % filteredCommands.length);
          break;
        case 'Enter':
          e.preventDefault();
          if (filteredCommands[selectedIndex]) {
            onSelect(filteredCommands[selectedIndex]);
          }
          break;
        case 'Escape':
          e.preventDefault();
          onClose();
          break;
      }
    },
    [isOpen, filteredCommands, selectedIndex, onSelect, onClose]
  );

  useEffect(() => {
    if (isOpen) {
      setSelectedIndex(0);
      document.addEventListener('keydown', handleKeyDown, true);
      return () => document.removeEventListener('keydown', handleKeyDown, true);
    }
  }, [isOpen, handleKeyDown]);

  // Click outside to close
  useEffect(() => {
    if (!isOpen) return;
    const handleClick = (e: MouseEvent) => {
      if (popoverRef.current && !popoverRef.current.contains(e.target as Node)) {
        onClose();
      }
    };
    // Delay to avoid immediate close from the input click
    const timer = setTimeout(() => {
      document.addEventListener('mousedown', handleClick);
    }, 0);
    return () => {
      clearTimeout(timer);
      document.removeEventListener('mousedown', handleClick);
    };
  }, [isOpen, onClose]);

  if (!isOpen || disabled) return null;

  return (
    <div
      ref={popoverRef}
      className="absolute bottom-full left-0 z-50 mb-2 min-w-[280px] max-w-[400px] rounded-xl border border-[var(--border)] bg-[var(--surface)] p-1.5 shadow-popup"
    >
      {/* Category header */}
      <div className="px-2.5 pb-1 pt-1 text-[0.625rem] font-semibold uppercase tracking-[0.06em] text-[var(--text-disabled)]">
        System Commands
      </div>

      {filteredCommands.length === 0 ? (
        <div className="px-2.5 py-2 text-xs text-[var(--text-disabled)]">No matching commands</div>
      ) : <div className="flex flex-col gap-0.5">
        {filteredCommands.map((cmd, index) => (
          <button
            key={cmd.id}
            type="button"
            onClick={() => onSelect(cmd)}
            onMouseEnter={() => setSelectedIndex(index)}
            className={`flex items-center gap-2.5 w-full rounded-lg px-2.5 py-2 text-left transition-all ${
              index === selectedIndex
                ? 'bg-[var(--primary-soft)] text-[var(--primary)]'
                : 'text-[var(--text-secondary)] hover:bg-[var(--surface-hover)]'
            }`}
          >
            <span className="text-sm font-mono font-medium">{cmd.label}</span>
            <span className="text-[0.6875rem] text-[var(--text-disabled)] truncate">{cmd.description}</span>
          </button>
        ))}
      </div>}

      {disabled && (
        <div className="px-2.5 py-2 text-[0.6875rem] text-amber-400 border-t border-[var(--border-subtle)] mt-1">
          Open a project to use these commands
        </div>
      )}
    </div>
  );
}
