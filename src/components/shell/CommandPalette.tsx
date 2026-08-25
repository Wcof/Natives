'use client';

import { startTransition, useState, useEffect, useRef, useCallback, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { useTheme } from '@/context/ThemeContext';
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
  Package,
  Search,
  FileText,
  Blocks,
  BookMarked,
  CalendarClock,
} from 'lucide-react';
import { SPACING, FONT_SIZE } from '@/lib/design-tokens';
import { useHydrated } from '@/hooks/useHydrated';
import { FILE_EVENTS, dispatchFileEvent } from '@/lib/file-events';
import { fsApi, searchApi } from '@/lib/files-api';
import { classifyError } from '@/lib/error-classifier';
import type { SearchResult } from '@/types/generated/SearchResult';

const MAX_SEARCH_QUERY_LENGTH = 256;

type FileRoot = { id: string; name: string; path: string };

export function resolvePaletteSearchRoot(roots: FileRoot[]): string | null {
  const path = roots.find((root) => root.id === 'home')?.path.trim();
  return path && path !== '/' ? path : null;
}

export function searchResultLabel(result: SearchResult): string {
  const normalized = result.path.replace(/\/+$/, '');
  const name = normalized.slice(normalized.lastIndexOf('/') + 1) || normalized;
  return result.line == null ? name : `${name}:${result.line}`;
}

export function createLatestRequestGate() {
  let generation = 0;
  return {
    next: () => ++generation,
    invalidate: () => { generation += 1; },
    isCurrent: (request: number) => request === generation,
  };
}

function parseSearchResults(value: unknown): SearchResult[] {
  if (!Array.isArray(value)) throw new Error('file read failed: invalid search response');
  const results = value.filter((item): item is SearchResult => {
    if (typeof item !== 'object' || item === null) return false;
    const result = item as Partial<SearchResult>;
    return typeof result.path === 'string'
      && (result.line === null || typeof result.line === 'number')
      && (result.text === null || typeof result.text === 'string')
      && (result.score === null || typeof result.score === 'number')
      && (result.mtime === null || typeof result.mtime === 'number');
  });
  if (results.length !== value.length) throw new Error('file read failed: invalid search result');
  return results;
}

function toSearchCommands(results: SearchResult[], icon: ReactNode): CommandItem[] {
  return results.map((result) => ({
    id: `__file__:${result.path}`,
    label: searchResultLabel(result),
    category: 'navigation',
    icon,
    description: result.text || result.path,
  }));
}

function mergeCommands(current: CommandItem[], incoming: CommandItem[]): CommandItem[] {
  const ids = new Set(current.map((command) => command.id));
  return [...current, ...incoming.filter((command) => !ids.has(command.id))];
}

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
    { id: 'modules', label: t(locale, 'nav.modules'), category: 'navigation', icon: <Wrench size={14} /> },
    { id: '__notifications__', label: t(locale, 'notifications.title'), category: 'navigation', icon: <Bell size={14} /> },
    { id: 'files', label: t(locale, 'nav.fileBrowser'), category: 'navigation', icon: <Folder size={14} /> },
    { id: 'ai', label: t(locale, 'nav.aiWorkbench'), category: 'navigation', icon: <Bot size={14} /> },
    { id: 'tools', label: t(locale, 'nav.tools'), category: 'navigation', icon: <Sliders size={14} /> },
    { id: 'capabilities', label: t(locale, 'nav.capabilities'), category: 'navigation', icon: <Blocks size={14} /> },
    { id: 'jobs', label: t(locale, 'nav.jobs'), category: 'navigation', icon: <CalendarClock size={14} /> },
    { id: 'library', label: t(locale, 'nav.library'), category: 'navigation', icon: <BookMarked size={14} /> },
    { id: 'terminal:toggle', label: t(locale, 'nav.terminalToggle'), category: 'action', icon: <Terminal size={14} /> },
    { id: 'theme:dark', label: t(locale, 'nav.themeTerminalVolt'), category: 'setting', icon: <Terminal size={14} /> },
    { id: 'theme:light', label: t(locale, 'nav.themeFrostedJasmine'), category: 'setting', icon: <Sun size={14} /> },
  ];
}

export default function CommandPalette({ isOpen, onClose, onSelect, onToggleTerminal }: CommandPaletteProps) {
  const { setTheme } = useTheme();
  const [query, setQuery] = useState('');
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [locale, setLocale] = useState<Locale>('zh');
  const [allCommands, setAllCommands] = useState<CommandItem[]>(() => getStaticCommands(locale));
  const [results, setResults] = useState<CommandItem[]>(() => getStaticCommands(locale));
  const [isSearching, setIsSearching] = useState(false);
  const [searchError, setSearchError] = useState<ReturnType<typeof classifyError> | null>(null);
  const mounted = useHydrated();

  const inputRef = useRef<HTMLInputElement>(null);
  const searchGateRef = useRef(createLatestRequestGate());

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
      startTransition(() => { setQuery(''); });

      setSelectedIndex(0);
      setResults(allCommands);
      setSearchError(null);
      setIsSearching(false);
    } else {
      searchGateRef.current.invalidate();
      setSearchError(null);
      setIsSearching(false);
    }
  }, [isOpen, allCommands]);

  const publishSearchError = useCallback((cause: unknown, request: number) => {
    if (!searchGateRef.current.isCurrent(request)) return;
    setSearchError(classifyError(cause, { locale }));
    setIsSearching(false);
  }, [locale]);

  const runSearch = useCallback(async (
    searchQuery: string,
    mode: 'files' | 'content' | 'both',
  ) => {
    const normalizedQuery = searchQuery.trim().slice(0, MAX_SEARCH_QUERY_LENGTH);
    if (normalizedQuery.length < 2) return;

    const request = searchGateRef.current.next();
    setSearchError(null);
    setIsSearching(true);

    try {
      const roots = await fsApi().roots();
      const root = resolvePaletteSearchRoot(roots);
      if (!root) throw new Error('file read failed: authorized home root unavailable');

      const search = searchApi();
      const pending: Array<Promise<CommandItem[]>> = [];
      if (mode === 'files' || mode === 'both') {
        pending.push(search.files(normalizedQuery, root, { maxResults: 8 })
          .then((value) => toSearchCommands(parseSearchResults(value), <FileText size={14} />)));
      }
      if (mode === 'content' || mode === 'both') {
        pending.push(search.grep(normalizedQuery, root, { maxResults: 8 })
          .then((value) => toSearchCommands(parseSearchResults(value), <Search size={14} />)));
      }

      const commandGroups = await Promise.all(pending);
      if (!searchGateRef.current.isCurrent(request)) return;
      setResults((current) => commandGroups.reduce(mergeCommands, current));
      setIsSearching(false);
    } catch (cause) {
      publishSearchError(cause, request);
    }
  }, [publishSearchError]);

  // Filter results + file search
  useEffect(() => {
    searchGateRef.current.invalidate();
    setSearchError(null);
    setIsSearching(false);

    if (!isOpen) return;

    if (!query.trim()) {
      startTransition(() => { setResults(allCommands); });

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
        void runSearch(searchTerm, 'content');
      }
      return;
    }

    // Also search files if query looks like a filename (has extension or starts with /)
    if (query.length >= 2 && (query.includes('.') || query.startsWith('/') || query.startsWith('~'))) {
      void runSearch(query, 'files');
    }
    return () => { searchGateRef.current.invalidate(); };
  }, [isOpen, query, allCommands, runSearch]);

  const handleSearchNow = useCallback(() => {
    const q = query.trim();
    if (!q) return;
    void runSearch(q, 'both');
  }, [query, runSearch]);

  const handleQueryChange = useCallback((nextQuery: string) => {
    searchGateRef.current.invalidate();
    setQuery(nextQuery);
  }, []);

  const closePalette = useCallback(() => {
    searchGateRef.current.invalidate();
    setSearchError(null);
    setIsSearching(false);
    onClose();
  }, [onClose]);

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
        } else if (query.trim().length >= 2) {
          // Spotlight-style: trigger search when no command is highlighted
          handleSearchNow();
        }
        break;
      case 'Escape':
        e.preventDefault();
        closePalette();
        break;
    }
  };

  const handleSelect = (cmd: CommandItem) => {
    if (cmd.id.startsWith('theme:')) {
      const themeId = cmd.id.slice(6);
      setTheme(themeId);
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
      dispatchFileEvent(FILE_EVENTS.navigateFiles, dir);
    } else {
      onSelect(cmd.id);
    }
    closePalette();
  };

  const categoryColors: Record<string, string> = {
    module: 'var(--primary)',
    action: 'var(--diff-mod)',
    setting: 'var(--warning)',
    navigation: 'var(--info)',
  };

  if (!isOpen) return null;
  if (!mounted) return null;

  return createPortal((
    <div
      style={{
        position: 'fixed', inset: 0, zIndex: 9999,
        display: 'flex', alignItems: 'flex-start', justifyContent: 'center', paddingTop: '20vh',
        background: 'var(--overlay)',
        animation: 'fadeIn 150ms ease',
      }}
      onClick={closePalette}
      aria-hidden="true"
    >
      {/* Command Palette — V1.0 纯色 Surface */}
      <div
        ref={dialogRef}
        role="dialog"
        aria-label={t(locale, 'commandPalette.placeholder')}
        aria-modal="true"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => { trapKeyDown(e); handleKeyDown(e); }}
        className="anim-dropIn"
        style={{
          position: 'relative', marginTop: 0,
          width: 520, maxWidth: '90vw',
          background: 'var(--surface)',
          border: '1px solid var(--border)',
          borderRadius: 'var(--radius-md)',
          boxShadow: 'var(--shadow-modal)',
          overflow: 'hidden',
        }}
      >
        {/* Input area with search icon */}
        <div style={{
          display: 'flex', alignItems: 'center', gap: SPACING.sm,
          padding: `${SPACING.md}px ${SPACING.lg}px`,
          borderBottom: '1px solid var(--border)',
          transition: 'box-shadow 150ms ease',
        }}
          className="focus-within:shadow-[0_0_0_2px_var(--primary)]">
          <Search size={16} className="shrink-0" style={{ color: 'var(--text-secondary)' }} />
          <input
            ref={inputRef}
            type="text"
            placeholder={t(locale, 'commandPalette.placeholder')}
            value={query}
            maxLength={MAX_SEARCH_QUERY_LENGTH}
            onChange={(e) => handleQueryChange(e.target.value)}
            onKeyDown={(e) => { trapKeyDown(e); handleKeyDown(e); }}
            style={{
              flex: 1,
              background: 'transparent',
              border: 'none',
              outline: 'none',
              color: 'var(--text)',
              fontSize: FONT_SIZE.lg,
              fontFamily: 'inherit',
            }}
            aria-label={t(locale, 'commandPalette.placeholder')}
          />
        </div>

        <div style={{ maxHeight: 360, overflowY: 'auto', overflowX: 'hidden', padding: `${SPACING.xs}px 0` }}>
          {searchError ? (
            <div role="alert" style={{ padding: `${SPACING.lg}px`, color: 'var(--danger)', fontSize: FONT_SIZE.sm }}>
              <div>{searchError.userMessage}</div>
              {searchError.actionHint && (
                <div style={{ marginTop: SPACING.xs, color: 'var(--text-secondary)' }}>{searchError.actionHint}</div>
              )}
              {searchError.retryable && query.trim().length >= 2 && (
                <button type="button" onClick={handleSearchNow} style={{ marginTop: SPACING.sm }}>
                  {t(locale, 'common.retry')}
                </button>
              )}
            </div>
          ) : isSearching ? (
            <div role="status" style={{ padding: `${SPACING.xxl}px ${SPACING.lg}px`, textAlign: 'center', color: 'var(--text-secondary)', fontSize: FONT_SIZE.sm }}>
              {t(locale, 'common.loading')}
            </div>
          ) : results.length === 0 ? (
            <div style={{ padding: `${SPACING.xxl}px ${SPACING.lg}px`, textAlign: 'center', color: 'var(--text-secondary)', fontSize: FONT_SIZE.sm }}>
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
                onKeyDown={(e) => {
                  if (e.key === 'Enter' || e.key === ' ') {
                    e.preventDefault();
                    handleSelect(cmd);
                  }
                }}
                onFocus={() => setSelectedIndex(index)}
                style={{
                  display: 'flex', alignItems: 'center', gap: SPACING.sm,
                  padding: `${SPACING.sm}px ${SPACING.lg}px`, cursor: 'pointer',
                  background: index === selectedIndex ? 'var(--surface-hover)' : 'transparent',
                  color: index === selectedIndex ? 'var(--text)' : 'var(--text-body)',
                  fontSize: FONT_SIZE.sm,
                  transition: 'background 150ms ease',
                }}
              >
                {/* Icon or category dot */}
                <span style={{
                  width: 20, display: 'inline-flex', alignItems: 'center', justifyContent: 'center', flexShrink: 0, fontSize: FONT_SIZE.md,
                }}>
                  {cmd.icon || (
                    <span style={{
                      display: 'inline-block', width: 6, height: 6, borderRadius: '50%',
                      background: categoryColors[cmd.category] || 'var(--text-secondary)',
                    }} />
                  )}
                </span>
                <span style={{ flex: 1, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{cmd.label}</span>
                {cmd.description && (
                  <span style={{ fontSize: FONT_SIZE.micro, color: 'var(--text-secondary)', fontFamily: 'var(--font-mono)', maxWidth: 180, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', flexShrink: 0 }}>
                    {cmd.description}
                  </span>
                )}
                <span style={{
                  fontSize: FONT_SIZE.micro, color: 'var(--text-disabled)',
                  textTransform: 'uppercase', letterSpacing: 0.5, flexShrink: 0,
                }}>
                  {cmd.category}
                </span>
              </div>
            ))
          )}
        </div>

        <div style={{
          padding: '8px 16px', borderTop: '1px solid var(--border)',
          display: 'flex', gap: SPACING.md, fontSize: FONT_SIZE.micro, color: 'var(--text-secondary)',
          alignItems: 'center',
        }}>
          <span>{t(locale, 'commandPalette.navigate')}</span>
          <span>{t(locale, 'commandPalette.select')}</span>
          <span>Esc {t(locale, 'commandPalette.close')}</span>
          <span>Tab {t(locale, 'commandPalette.cycle')}</span>
        </div>
      </div>
    </div>
  ), document.body);
}
