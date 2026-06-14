'use client';

import { useState, useEffect, useRef } from 'react';
import { applyTheme } from '@/lib/theme-engine';

interface CommandItem {
  id: string;
  label: string;
  category: 'module' | 'action' | 'setting';
  icon?: string;
}

interface CommandPaletteProps {
  isOpen: boolean;
  onClose: () => void;
  onSelect: (id: string) => void;
  onToggleTerminal?: () => void;
}

const DEFAULT_COMMANDS: CommandItem[] = [
  { id: '__settings__', label: 'Open Settings', category: 'action' },
  { id: '__workshop__', label: 'Open Workshop', category: 'action' },
  { id: 'terminal:toggle', label: 'Toggle Terminal', category: 'action' },
  { id: 'theme:terminal-volt', label: 'Theme: Terminal Volt', category: 'setting' },
  { id: 'theme:warm-archive', label: 'Theme: Warm Archive', category: 'setting' },
  { id: 'theme:editorial-index', label: 'Theme: Editorial Index', category: 'setting' },
];

export default function CommandPalette({ isOpen, onClose, onSelect, onToggleTerminal }: CommandPaletteProps) {
  const [query, setQuery] = useState('');
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [results, setResults] = useState<CommandItem[]>(DEFAULT_COMMANDS);
  const inputRef = useRef<HTMLInputElement>(null);

  // Focus input when opened
  useEffect(() => {
    if (isOpen) {
      setTimeout(() => inputRef.current?.focus(), 50);
      setQuery('');
      setSelectedIndex(0);
    }
  }, [isOpen]);

  // Filter results
  useEffect(() => {
    if (!query.trim()) {
      setResults(DEFAULT_COMMANDS);
      return;
    }
    const q = query.toLowerCase();
    setResults(
      DEFAULT_COMMANDS.filter(
        (cmd) => cmd.label.toLowerCase().includes(q) || cmd.id.toLowerCase().includes(q),
      ),
    );
    setSelectedIndex(0);
  }, [query]);

  const handleKeyDown = (e: React.KeyboardEvent) => {
    switch (e.key) {
      case 'Tab': {
        e.preventDefault();
        const focusable =
          results.length > 0
            ? document.querySelector('[role="option"]') as HTMLElement | null
            : null;
        if (e.shiftKey) {
          // Shift+Tab: focus the input
          inputRef.current?.focus();
        } else if (focusable) {
          // Tab: focus the first result
          focusable.focus();
        } else {
          // No results, loop back to input
          inputRef.current?.focus();
        }
        break;
      }
      case 'ArrowDown':
        e.preventDefault();
        setSelectedIndex((prev) => Math.min(prev + 1, results.length - 1));
        break;
      case 'ArrowUp':
        e.preventDefault();
        setSelectedIndex((prev) => Math.max(prev - 1, 0));
        break;
      case 'Enter':
        e.preventDefault();
        if (results[selectedIndex]) {
          handleSelect(results[selectedIndex]!.id);
        }
        break;
      case 'Escape':
        e.preventDefault();
        onClose();
        break;
    }
  };

  const handleSelect = (id: string) => {
    if (id.startsWith('theme:')) {
      const themeId = id.slice(6);
      applyTheme(themeId);
    } else if (id === 'terminal:toggle') {
      onToggleTerminal?.();
    } else {
      onSelect(id);
    }
    onClose();
  };

  if (!isOpen) return null;

  return (
    <>
      {/* Overlay */}
      <div
        style={{
          position: 'fixed', inset: 0, zIndex: 100,
          background: 'rgba(0,0,0,0.4)',
          backdropFilter: 'blur(4px)',
        }}
        onClick={onClose}
        aria-hidden="true"
      />

      {/* Command Palette */}
      <div
        role="dialog"
        aria-label="Command palette"
        aria-modal="true"
        onKeyDown={handleKeyDown}
        style={{
          position: 'fixed', top: '20%', left: '50%', transform: 'translateX(-50%)',
          width: 520, maxWidth: '90vw',
          background: 'color-mix(in srgb, var(--bg-2,#131410) 82%, transparent)',
          backdropFilter: 'blur(24px) saturate(1.5)',
          border: '1px solid var(--border,#262920)',
          borderRadius: 12,
          boxShadow: '0 12px 40px rgba(0,0,0,0.22)',
          zIndex: 101,
          overflow: 'hidden',
        }}
      >
        <div style={{ padding: '12px 16px', borderBottom: '1px solid var(--border,#262920)' }}>
          <input
            ref={inputRef}
            type="text"
            placeholder="Search modules, settings, actions..."
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={handleKeyDown}
            style={{
              width: '100%',
              background: 'transparent',
              border: 'none',
              outline: 'none',
              color: 'var(--text,#f2f2ea)',
              fontSize: 15,
              fontFamily: 'inherit',
            }}
            aria-label="Search commands"
          />
        </div>

        <div style={{ maxHeight: 320, overflowY: 'auto', padding: '4px 0' }}>
          {results.length === 0 ? (
            <div style={{ padding: '20px 16px', textAlign: 'center', color: 'var(--text-faint,#62655a)', fontSize: 13 }}>
              No results found
            </div>
          ) : (
            results.map((cmd, index) => {
              const categoryColors: Record<string, string> = {
                module: 'var(--accent,#cdf24b)',
                action: '#5b9cf5',
                setting: '#e6b800',
              };
              return (
                <div
                  key={cmd.id}
                  role="option"
                  aria-selected={index === selectedIndex}
                  onClick={() => handleSelect(cmd.id)}
                  style={{
                    display: 'flex', alignItems: 'center', gap: 10,
                    padding: '8px 16px', cursor: 'pointer',
                    background: index === selectedIndex ? 'var(--bg-3,#1c1e17)' : 'transparent',
                    color: 'var(--text,#f2f2ea)',
                    fontSize: 13,
                    transition: 'background 0.1s',
                  }}
                >
                  <span style={{
                    width: 6, height: 6, borderRadius: '50%',
                    background: categoryColors[cmd.category] || '#888',
                    flexShrink: 0,
                  }} />
                  <span>{cmd.label}</span>
                  <span style={{
                    marginLeft: 'auto', fontSize: 10,
                    color: 'var(--text-faint,#62655a)',
                    textTransform: 'uppercase',
                  }}>
                    {cmd.category}
                  </span>
                </div>
              );
            })
          )}
        </div>

        <div style={{
          padding: '8px 16px', borderTop: '1px solid var(--border,#262920)',
          display: 'flex', gap: 12, fontSize: 11, color: 'var(--text-faint,#62655a)',
        }}>
          <span>↑↓ Navigate</span>
          <span>↵ Select</span>
          <span>Esc Close</span>
        </div>
      </div>
    </>
  );
}
