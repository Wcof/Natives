'use client';

import { useState, useEffect, useRef, useCallback, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { applyTheme } from '@/lib/theme-engine';
import { t, type Locale } from '@/i18n';
import { useFocusTrap } from '@/lib/useFocusTrap';
import {
  Settings,
  Wrench,
  Bell,
  Folder,
  Bot,
  Sliders,
  Terminal,
  Sun,
  BookOpen,
  Package,
  Search,
  FileText,
  Globe,
} from 'lucide-react';

interface CommandItem {
  id: string;
  label: string;
  category: 'module' | 'action' | 'setting' | 'navigation';
  icon?: ReactNode;
  description?: string;
}

interface CommandPaletteProps {
  isOpen: boolean;
  onClose: () => void;
  onSelect: (id: string) => void;
  onToggleTerminal?: () => void;
  terminalSessionId?: string | null;
}

function getStaticCommands(locale: Locale): CommandItem[] {
  return [
    { id: '__settings__', label: t(locale, 'nav.settings'), category: 'navigation', icon: <Settings size={14} /> },
    { id: '__workshop__', label: t(locale, 'nav.workshop'), category: 'navigation', icon: <Wrench size={14} /> },
    { id: '__notifications__', label: t(locale, 'notifications.title'), category: 'navigation', icon: <Bell size={14} /> },
    { id: 'files', label: t(locale, 'nav.fileBrowser'), category: 'navigation', icon: <Folder size={14} /> },
    { id: 'ai', label: t(locale, 'nav.aiWorkbench'), category: 'navigation', icon: <Bot size={14} /> },
    { id: 'tools', label: t(locale, 'nav.tools'), category: 'navigation', icon: <Sliders size={14} /> },
    { id: 'terminal:toggle', label: t(locale, 'nav.terminalToggle'), category: 'action', icon: <Terminal size={14} /> },
    { id: 'theme:terminal-volt', label: t(locale, 'nav.themeTerminalVolt'), category: 'setting', icon: <Terminal size={14} /> },
    { id: 'theme:frosted-jasmine', label: t(locale, 'nav.themeFrostedJasmine'), category: 'setting', icon: <Sun size={14} /> },
  ];
}

export default function CommandPalette({ isOpen, onClose, onSelect, onToggleTerminal }: CommandPaletteProps) {
  const [query, setQuery] = useState('');
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [locale, setLocale] = useState<Locale>('zh');
  const [allCommands, setAllCommands] = useState<CommandItem[]>(() => getStaticCommands(locale));
  const [results, setResults] = useState<CommandItem[]>(() => getStaticCommands(locale));
  const [searchScope, setSearchScope] = useState<'global' | 'local'>('global');
  const [mounted, setMounted] = useState(false);
  useEffect(() => { setMounted(true); }, []);
  const inputRef = useRef<HTMLInputElement>(null);

  // Load locale
  const { dialogRef, handleKeyDown: trapKeyDown } = useFocusTrap();
  useEffect(() => {
    async function loadLocale() {
      try {
        const saved = await window.nativesAPI?.getLocale?.();
        if (saved) setLocale(saved === 'en' ? 'en' : 'zh');
      } catch { /* ignore */ }
    }
    loadLocale();
  }, []);

  // Update static commands when locale changes
  useEffect(() => {
    setAllCommands(() => {
      const statics = getStaticCommands(locale);
      // Re-add module commands if any
      return [...statics];
    });
    setResults(() => getStaticCommands(locale));
  }, [locale]);

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
              icon: <Package size={14} />,
              description: m.id,
            }));
            setAllCommands([...getStaticCommands(locale), ...moduleCommands]);
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

    // content: prefix — full-text search (PRD v2 story 47)
    if (query.startsWith('content:') && query.length > 8) {
      const searchTerm = query.slice(8).trim();
      if (searchTerm.length >= 2) {
        const root = searchScope === 'local' ? (process.env.HOME || '/') : '/';
        window.nativesAPI?.search?.grep?.(searchTerm, root, { maxResults: 8 }).then((results) => {
          if (Array.isArray(results) && results.length > 0) {
            const contentCommands: CommandItem[] = results.map((r: { path: string; name: string; line?: number; match?: string }) => ({
              id: `__file__:${r.path}`,
              label: `${r.name}${r.line ? `:${r.line}` : ''}`,
              category: 'navigation' as const,
              icon: <Search size={14} />,
              description: r.match || r.path,
            }));
            setResults((prev) => {
              const cmdIds = new Set(prev.map((c) => c.id));
              const newItems = contentCommands.filter((f) => !cmdIds.has(f.id));
              return [...prev, ...newItems];
            });
          }
        }).catch(() => { /* ignore */ });
      }
      return;
    }

    // Also search files if query looks like a filename (has extension or starts with /)
    if (query.length >= 2 && (query.includes('.') || query.startsWith('/') || query.startsWith('~'))) {
      const root = query.startsWith('~') || query.startsWith('/') ? '/' : (searchScope === 'local' ? (process.env.HOME || '/') : '/');
      window.nativesAPI?.search?.files?.(query, root, { maxResults: 8 }).then((fileResults) => {
        if (Array.isArray(fileResults) && fileResults.length > 0) {
          const fileCommands: CommandItem[] = fileResults.map((f: { path: string; name: string }) => ({
            id: `__file__:${f.path}`,
            label: f.name,
            category: 'navigation' as const,
            icon: <FileText size={14} />,
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

  const handleKeyDown = (e: React.KeyboardEvent) => {
    // Tab handled by shared useFocusTrap hook on the dialog container
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
    module: 'var(--accent)',
    action: 'var(--diff-mod)',
    setting: 'var(--warning)',
    navigation: 'var(--info)',
  };

  if (!isOpen) return null;
  if (!mounted) return null;
  const root = document.getElementById('content-overlay-root');
  if (!root) return null;

  return createPortal((
    <>
      {/* Overlay */}
      <div
        style={{
          position: 'absolute', inset: 0, zIndex: 60,
          display: 'flex', alignItems: 'center', justifyContent: 'center',
          background: 'rgba(0,0,0,0.45)',
          backdropFilter: 'blur(24px) saturate(150%)',
          WebkitBackdropFilter: 'blur(24px) saturate(150%)',
          animation: 'fadeIn 0.18s cubic-bezier(0.16, 1, 0.3, 1)',
        }}
        onClick={onClose}
        aria-hidden="true"
      />

      {/* Command Palette — vibe-* glassmorphic style */}
      <div
        ref={dialogRef}
        role="dialog"
        aria-label={t(locale, 'commandPalette.placeholder')}
        aria-modal="true"
        onKeyDown={(e) => { trapKeyDown(e); handleKeyDown(e); }}
        className="anim-dropIn"
        style={{
          position: 'absolute', top: '20%', left: '50%', transform: 'translateX(-50%)',
          width: 520, maxWidth: '90vw',
          background: 'var(--vibe-toolbar-bg)',
          backdropFilter: 'blur(28px) saturate(145%)',
          WebkitBackdropFilter: 'blur(28px) saturate(145%)',
          border: '0.0625rem solid var(--vibe-toolbar-border)',
          borderRadius: '1rem',
          boxShadow: 'var(--vibe-toolbar-shadow)',
          zIndex: 101,
          overflow: 'hidden',
        }}
      >
        {/* Input area with search icon */}
        <div style={{
          display: 'flex', alignItems: 'center', gap: 8,
          padding: '12px 16px',
          borderBottom: '0.0625rem solid var(--vibe-toolbar-border)',
        }}>
          <Search size={14} className="shrink-0" style={{ color: 'var(--vibe-search-placeholder)' }} />
          <input
            ref={inputRef}
            type="text"
            placeholder={t(locale, 'commandPalette.placeholder')}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => { trapKeyDown(e); handleKeyDown(e); }}
            style={{
              flex: 1,
              background: 'transparent',
              border: 'none',
              outline: 'none',
              color: 'var(--vibe-brand-text)',
              fontSize: 15,
              fontFamily: 'inherit',
            }}
            aria-label={t(locale, 'commandPalette.placeholder')}
          />
        </div>

        <div style={{ maxHeight: 360, overflowY: 'auto', overflowX: 'hidden', padding: '4px 0' }}>
          {results.length === 0 ? (
            <div style={{ padding: '24px 16px', textAlign: 'center', color: 'var(--vibe-btn-text)', fontSize: 13 }}>
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
                  background: index === selectedIndex ? 'var(--vibe-active-bg)' : 'transparent',
                  color: index === selectedIndex ? 'var(--vibe-active-color)' : 'var(--vibe-brand-text)',
                  fontSize: 13,
                  transition: 'background 0.1s',
                }}
              >
                {/* Icon or category dot */}
                <span style={{
                  width: 20, display: 'inline-flex', alignItems: 'center', justifyContent: 'center', flexShrink: 0, fontSize: 14,
                }}>
                  {cmd.icon || (
                    <span style={{
                      display: 'inline-block', width: 6, height: 6, borderRadius: '50%',
                      background: categoryColors[cmd.category] || 'var(--vibe-btn-text)',
                    }} />
                  )}
                </span>
                <span style={{ flex: 1, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{cmd.label}</span>
                {cmd.description && (
                  <span style={{ fontSize: 10, color: 'var(--vibe-btn-text)', fontFamily: 'var(--font-mono)', maxWidth: 180, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', flexShrink: 0 }}>
                    {cmd.description}
                  </span>
                )}
                <span style={{
                  fontSize: 10, color: 'var(--vibe-btn-text)',
                  textTransform: 'uppercase', letterSpacing: 0.5, flexShrink: 0,
                }}>
                  {cmd.category}
                </span>
              </div>
            ))
          )}
        </div>

        <div style={{
          padding: '8px 16px', borderTop: '0.0625rem solid var(--vibe-toolbar-border)',
          display: 'flex', gap: 12, fontSize: 11, color: 'var(--vibe-btn-text)',
          alignItems: 'center',
        }}>
          <span>{t(locale, 'commandPalette.navigate')}</span>
          <span>{t(locale, 'commandPalette.select')}</span>
          <span>Esc {t(locale, 'commandPalette.close')}</span>
          <span>Tab {t(locale, 'commandPalette.cycle')}</span>
          <div style={{ flex: 1 }} />
          {/* Search scope toggle */}
          <button
            onClick={() => setSearchScope((s) => s === 'global' ? 'local' : 'global')}
            style={{
              background: 'var(--vibe-btn-bg)',
              border: '0.0625rem solid var(--vibe-btn-border)',
              borderRadius: '0.5rem',
              padding: '2px 6px', fontSize: 10, cursor: 'pointer',
              color: searchScope === 'local' ? 'var(--vibe-active-color)' : 'var(--vibe-btn-text)',
              display: 'inline-flex', alignItems: 'center', gap: 4,
            }}
            title={searchScope === 'global' ? t(locale, 'commandPalette.searchScopeGlobal') : t(locale, 'commandPalette.searchScopeLocal')}
          >
            {searchScope === 'global' ? (
              <>
                <Globe size={11} />
                <span>{t(locale, 'commandPalette.globalLabel')}</span>
              </>
            ) : (
              <>
                <Folder size={11} />
                <span>{t(locale, 'commandPalette.localLabel')}</span>
              </>
            )}
          </button>
        </div>
      </div>
    </>
  ), root);
}
