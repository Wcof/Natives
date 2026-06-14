'use client';

import { useState, useEffect, useRef, useCallback } from 'react';
import { applyTheme } from '@/lib/theme-engine';
import { t, type Locale } from '@/i18n';

interface CommandItem {
  id: string;
  label: string;
  category: 'module' | 'action' | 'setting' | 'navigation';
  icon?: string;
  description?: string;
}

interface CommandPaletteProps {
  isOpen: boolean;
  onClose: () => void;
  onSelect: (id: string) => void;
  onToggleTerminal?: () => void;
  terminalSessionId?: string | null;
}

const STATIC_COMMANDS: CommandItem[] = [
  { id: '__settings__', label: 'Open Settings', category: 'navigation', icon: '⚙️' },
  { id: '__workshop__', label: 'Open Workshop', category: 'navigation', icon: '🔧' },
  { id: '__store__', label: 'Open Store', category: 'navigation', icon: '🛒' },
  { id: '__notifications__', label: 'Open Notifications', category: 'navigation', icon: '🔔' },
  { id: 'files', label: 'File Browser', category: 'navigation', icon: '📁' },
  { id: 'ai', label: 'AI Workbench', category: 'navigation', icon: '🤖' },
  { id: 'tools', label: 'Tools', category: 'navigation', icon: '🛠' },
  { id: 'terminal:toggle', label: 'Toggle Terminal', category: 'action', icon: '⬛' },
  { id: 'theme:terminal-volt', label: 'Theme: Terminal Volt', category: 'setting', icon: '🌙' },
  { id: 'theme:warm-archive', label: 'Theme: Warm Archive', category: 'setting', icon: '☀️' },
];

export default function CommandPalette({ isOpen, onClose, onSelect, onToggleTerminal }: CommandPaletteProps) {
  const [query, setQuery] = useState('');
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [allCommands, setAllCommands] = useState<CommandItem[]>(STATIC_COMMANDS);
  const [results, setResults] = useState<CommandItem[]>(STATIC_COMMANDS);
  const [locale, setLocale] = useState<Locale>('zh');
  const inputRef = useRef<HTMLInputElement>(null);
  const dialogRef = useRef<HTMLDivElement>(null);

  // Load locale
  useEffect(() => {
    async function loadLocale() {
      try {
        const saved = await window.nativesAPI?.getLocale?.();
        if (saved) setLocale(saved === 'en' ? 'en' : 'zh');
      } catch { /* ignore */ }
    }
    loadLocale();
  }, []);

  // Load dynamic module commands when opened
  useEffect(() => {
    if (!isOpen) return;

    async function loadModuleCommands() {
      try {
        const api = window.nativesAPI;
        if (api?.module?.list) {
          const modules = await api.module.list();
          if (Array.isArray(modules)) {
            const moduleCommands: CommandItem[] = (modules as Array<{ id: string; name: string }>).map((m) => ({
              id: `module:${m.id}`,
              label: m.name,
              category: 'module' as const,
              icon: '📦',
              description: m.id,
            }));
            setAllCommands([...STATIC_COMMANDS, ...moduleCommands]);
          }
        }
      } catch { /* ignore */ }
    }
    loadModuleCommands();
  }, [isOpen]);

  // Focus input when opened
  useEffect(() => {
    if (isOpen) {
      setTimeout(() => inputRef.current?.focus(), 50);
      setQuery('');
      setSelectedIndex(0);
      setResults(allCommands);
    }
  }, [isOpen, allCommands]);

  // Filter results + file search
  useEffect(() => {
    if (!query.trim()) {
      setResults(allCommands);
      setSelectedIndex(0);
      return;
    }
    const q = query.toLowerCase();
    const filtered = allCommands.filter(
      (cmd) =>
        cmd.label.toLowerCase().includes(q) ||
        cmd.id.toLowerCase().includes(q) ||
        (cmd.description && cmd.description.toLowerCase().includes(q)),
    );
    setResults(filtered);
    setSelectedIndex(0);

    // Also search files if query looks like a filename (has extension or starts with /)
    if (query.length >= 2 && (query.includes('.') || query.startsWith('/') || query.startsWith('~'))) {
      const root = query.startsWith('~') || query.startsWith('/') ? '/' : (process.env.HOME || '/');
      window.nativesAPI?.search?.files?.(query, root, { maxResults: 8 }).then((fileResults) => {
        if (Array.isArray(fileResults) && fileResults.length > 0) {
          const fileCommands: CommandItem[] = fileResults.map((f: { path: string; name: string }) => ({
            id: `__file__:${f.path}`,
            label: f.name,
            category: 'navigation' as const,
            icon: '📄',
            description: f.path,
          }));
          // Merge: commands first, then files
          setResults((prev) => {
            const cmdIds = new Set(prev.map((c) => c.id));
            const newFiles = fileCommands.filter((f) => !cmdIds.has(f.id));
            return [...prev, ...newFiles];
          });
        }
      }).catch(() => { /* ignore */ });
    }
  }, [query, allCommands]);

  // Focus trap
  const getFocusableElements = useCallback(() => {
    if (!dialogRef.current) return [];
    return Array.from(
      dialogRef.current.querySelectorAll<HTMLElement>(
        'input, [role="option"], button, [tabindex]:not([tabindex="-1"])',
      ),
    );
  }, []);

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Tab') {
      const focusable = getFocusableElements();
      if (focusable.length === 0) return;
      e.preventDefault();
      const currentIndex = focusable.indexOf(document.activeElement as HTMLElement);
      if (e.shiftKey) {
        const prev = currentIndex <= 0 ? focusable.length - 1 : currentIndex - 1;
        focusable[prev]?.focus();
      } else {
        const next = currentIndex >= focusable.length - 1 ? 0 : currentIndex + 1;
        focusable[next]?.focus();
      }
      return;
    }

    switch (e.key) {
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
          handleSelect(results[selectedIndex]!);
        }
        break;
      case 'Escape':
        e.preventDefault();
        onClose();
        break;
    }
  };

  const handleSelect = (cmd: CommandItem) => {
    if (cmd.id.startsWith('theme:')) {
      const themeId = cmd.id.slice(6);
      applyTheme(themeId);
      window.nativesAPI?.setTheme?.(themeId);
    } else if (cmd.id === 'terminal:toggle') {
      onToggleTerminal?.();
    } else if (cmd.id.startsWith('module:')) {
      const moduleId = cmd.id.slice(7);
      onSelect(`module:${moduleId}`);
    } else if (cmd.id.startsWith('__file__:')) {
      // Navigate to file's directory and select it
      const filePath = cmd.id.slice(9);
      const dir = filePath.substring(0, filePath.lastIndexOf('/')) || '/';
      onSelect('files');
      window.dispatchEvent(new CustomEvent('navigate-files', { detail: dir }));
    } else {
      onSelect(cmd.id);
    }
    onClose();
  };

  const categoryColors: Record<string, string> = {
    module: 'var(--accent,#cdf24b)',
    action: '#5b9cf5',
    setting: '#e6b800',
    navigation: '#a78bfa',
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
        ref={dialogRef}
        role="dialog"
        aria-label={t(locale, 'commandPalette.placeholder')}
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
            placeholder={t(locale, 'commandPalette.placeholder')}
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
            aria-label={t(locale, 'commandPalette.placeholder')}
          />
        </div>

        <div style={{ maxHeight: 360, overflowY: 'auto', padding: '4px 0' }}>
          {results.length === 0 ? (
            <div style={{ padding: '24px 16px', textAlign: 'center', color: 'var(--text-faint,#62655a)', fontSize: 13 }}>
              {t(locale, 'commandPalette.noResults')}
            </div>
          ) : (
            results.map((cmd, index) => (
              <div
                key={cmd.id}
                role="option"
                aria-selected={index === selectedIndex}
                tabIndex={0}
                onClick={() => handleSelect(cmd)}
                onFocus={() => setSelectedIndex(index)}
                style={{
                  display: 'flex', alignItems: 'center', gap: 10,
                  padding: '8px 16px', cursor: 'pointer',
                  background: index === selectedIndex ? 'var(--bg-3,#1c1e17)' : 'transparent',
                  color: 'var(--text,#f2f2ea)',
                  fontSize: 13,
                  transition: 'background 0.1s',
                }}
              >
                {/* Icon or category dot */}
                <span style={{
                  width: 20, textAlign: 'center', flexShrink: 0, fontSize: 14,
                }}>
                  {cmd.icon || (
                    <span style={{
                      display: 'inline-block', width: 6, height: 6, borderRadius: '50%',
                      background: categoryColors[cmd.category] || '#888',
                    }} />
                  )}
                </span>
                <span style={{ flex: 1 }}>{cmd.label}</span>
                {cmd.description && (
                  <span style={{ fontSize: 10, color: 'var(--text-faint)', fontFamily: 'var(--font-mono)' }}>
                    {cmd.description}
                  </span>
                )}
                <span style={{
                  fontSize: 10, color: 'var(--text-faint,#62655a)',
                  textTransform: 'uppercase', letterSpacing: 0.5,
                }}>
                  {cmd.category}
                </span>
              </div>
            ))
          )}
        </div>

        <div style={{
          padding: '8px 16px', borderTop: '1px solid var(--border,#262920)',
          display: 'flex', gap: 12, fontSize: 11, color: 'var(--text-faint,#62655a)',
        }}>
          <span>↑↓ Navigate</span>
          <span>↵ Select</span>
          <span>Esc Close</span>
          <span>Tab Cycle</span>
        </div>
      </div>
    </>
  );
}
