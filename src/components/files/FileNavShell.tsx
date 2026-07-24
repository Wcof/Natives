'use client';

import { useEffect, useRef, useState, type CSSProperties } from 'react';
import {
  ArrowLeft,
  ArrowRight,
  ArrowUp,
  ArrowDown,
  ArrowUpDown,
  RefreshCw,
  Star,
  Clock,
  History,
  Search,
  FolderSearch,
  CornerDownLeft,
} from 'lucide-react';
import { t, type Locale } from '@/i18n';
import { FONT_SIZE, SPACING, BORDER_RADIUS } from '@/lib/design-tokens';
import {
  nextSortForField,
  type FileSortBy,
  type FileSortDir,
} from './file-sort';

export type { FileSortBy, FileSortDir };

export interface FileNavShellProps {
  currentPath: string;
  canGoBack: boolean;
  canGoForward: boolean;
  canGoUp: boolean;
  isFavorite: boolean;
  recentMode: boolean;
  recentOpenedMode: boolean;
  searchQuery: string;
  sortBy: FileSortBy;
  sortDir: FileSortDir;
  loading?: boolean;
  onBack: () => void;
  onForward: () => void;
  onUp: () => void;
  onRefresh: () => void;
  onToggleFavorite: () => void;
  onToggleRecent: () => void;
  onToggleRecentOpened: () => void;
  onSearchChange: (query: string) => void;
  onOpenGlobalSearch: () => void;
  onPathSubmit: (path: string) => void | Promise<void>;
  /** Optional local sort control; when omitted, chip is display-only. */
  onSortChange?: (sortBy: FileSortBy, sortDir: FileSortDir) => void;
}

export default function FileNavShell({
  currentPath,
  canGoBack,
  canGoForward,
  canGoUp,
  isFavorite,
  recentMode,
  recentOpenedMode,
  searchQuery,
  sortBy,
  sortDir,
  loading,
  onBack,
  onForward,
  onUp,
  onRefresh,
  onToggleFavorite,
  onToggleRecent,
  onToggleRecentOpened,
  onSearchChange,
  onOpenGlobalSearch,
  onPathSubmit,
  onSortChange,
}: FileNavShellProps) {
  const [locale, setLocale] = useState<Locale>('zh');
  const [editingPath, setEditingPath] = useState(false);
  const [pathDraft, setPathDraft] = useState(currentPath);
  const [sortOpen, setSortOpen] = useState(false);
  const pathInputRef = useRef<HTMLInputElement>(null);
  const filterRef = useRef<HTMLInputElement>(null);
  const sortRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    async function loadLocale() {
      try {
        const saved = await window.nativesAPI?.getLocale?.();
        if (saved === 'en') setLocale('en');
        else setLocale('zh');
      } catch { /* ignore */ }
    }
    loadLocale();
  }, []);

  useEffect(() => {
    if (!editingPath) setPathDraft(currentPath);
  }, [currentPath, editingPath]);

  useEffect(() => {
    if (editingPath) {
      pathInputRef.current?.focus();
      pathInputRef.current?.select();
    }
  }, [editingPath]);

  // Close sort menu on outside click
  useEffect(() => {
    if (!sortOpen) return;
    const onDown = (e: MouseEvent) => {
      if (sortRef.current && !sortRef.current.contains(e.target as Node)) {
        setSortOpen(false);
      }
    };
    document.addEventListener('mousedown', onDown);
    return () => document.removeEventListener('mousedown', onDown);
  }, [sortOpen]);

  // ⌘L focuses path bar (browser-like); ⌘F focuses local filter; ⌘⇧F global search
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement;
      const typing = target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.isContentEditable;
      if (!(e.metaKey || e.ctrlKey)) return;

      if (e.key.toLowerCase() === 'l' && !e.shiftKey) {
        e.preventDefault();
        setEditingPath(true);
        setPathDraft(currentPath);
        return;
      }
      if (e.key.toLowerCase() === 'f' && e.shiftKey) {
        e.preventDefault();
        onOpenGlobalSearch();
        return;
      }
      if (e.key.toLowerCase() === 'f' && !e.shiftKey && !typing) {
        e.preventDefault();
        filterRef.current?.focus();
        filterRef.current?.select();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [currentPath, onOpenGlobalSearch]);

  const sortFieldLabel =
    sortBy === 'mtime'
      ? t(locale, 'fileBrowser.modified')
      : sortBy === 'size'
        ? t(locale, 'fileBrowser.size')
        : t(locale, 'fileBrowser.name');
  const sortLabel = `${sortFieldLabel} ${sortDir === 'asc' ? '↑' : '↓'}`;

  const applySort = (nextBy: FileSortBy, nextDir?: FileSortDir) => {
    if (!onSortChange) return;
    if (nextDir) {
      onSortChange(nextBy, nextDir);
      return;
    }
    const next = nextSortForField(sortBy, sortDir, nextBy);
    onSortChange(next.sortBy, next.sortDir);
  };

  const commitPath = async () => {
    const next = pathDraft.trim() || '/';
    setEditingPath(false);
    if (next !== currentPath) {
      await onPathSubmit(next);
    }
  };

  const btnStyle = (enabled: boolean): CSSProperties => ({
    display: 'inline-flex',
    alignItems: 'center',
    justifyContent: 'center',
    width: 28,
    height: 28,
    borderRadius: BORDER_RADIUS.sm,
    border: '1px solid var(--border)',
    background: 'var(--bg-2)',
    color: enabled ? 'var(--text)' : 'var(--text-disabled)',
    opacity: enabled ? 1 : 0.45,
    cursor: enabled ? 'pointer' : 'not-allowed',
    padding: 0,
  });

  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: SPACING.sm,
        padding: '6px 10px',
        borderBottom: '1px solid var(--border)',
        background: 'var(--surface)',
        flexWrap: 'wrap',
        minHeight: 40,
      }}
    >
      {/* History + up + refresh */}
      <div style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
        <button type="button" style={btnStyle(canGoBack)} disabled={!canGoBack} onClick={onBack} title={`${t(locale, 'fileBrowser.back')} ⌘[`}>
          <ArrowLeft size={14} />
        </button>
        <button type="button" style={btnStyle(canGoForward)} disabled={!canGoForward} onClick={onForward} title={`${t(locale, 'fileBrowser.forward')} ⌘]`}>
          <ArrowRight size={14} />
        </button>
        <button type="button" style={btnStyle(canGoUp)} disabled={!canGoUp} onClick={onUp} title={`${t(locale, 'fileBrowser.goUp')} ⌫`}>
          <ArrowUp size={14} />
        </button>
        <button
          type="button"
          style={btnStyle(true)}
          onClick={onRefresh}
          title={t(locale, 'fileBrowser.refresh')}
          aria-busy={loading}
        >
          <RefreshCw size={13} style={{ animation: loading ? 'spin 0.8s linear infinite' : undefined }} />
        </button>
      </div>

      {/* Editable path bar */}
      <div
        style={{
          flex: 1,
          minWidth: 180,
          display: 'flex',
          alignItems: 'center',
          gap: 6,
          height: 30,
          padding: '0 8px',
          borderRadius: BORDER_RADIUS.sm,
          border: editingPath ? '1px solid var(--primary)' : '1px solid var(--border)',
          background: 'var(--bg-2)',
          fontFamily: 'var(--font-mono)',
          fontSize: FONT_SIZE.sm,
        }}
        onDoubleClick={() => {
          setPathDraft(currentPath);
          setEditingPath(true);
        }}
        title={t(locale, 'fileBrowser.editPathHint')}
      >
        {editingPath ? (
          <>
            <input
              ref={pathInputRef}
              value={pathDraft}
              onChange={(e) => setPathDraft(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') {
                  e.preventDefault();
                  void commitPath();
                } else if (e.key === 'Escape') {
                  e.preventDefault();
                  setEditingPath(false);
                  setPathDraft(currentPath);
                }
              }}
              onBlur={() => {
                // slight delay so click on go button still works
                window.setTimeout(() => setEditingPath(false), 120);
              }}
              placeholder={t(locale, 'fileBrowser.pathPlaceholder')}
              style={{
                flex: 1,
                minWidth: 0,
                border: 'none',
                outline: 'none',
                background: 'transparent',
                color: 'var(--text)',
                fontFamily: 'inherit',
                fontSize: 'inherit',
              }}
              aria-label={t(locale, 'fileBrowser.addressBar')}
            />
            <button
              type="button"
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => { void commitPath(); }}
              style={{
                display: 'inline-flex',
                alignItems: 'center',
                gap: 2,
                border: 'none',
                background: 'transparent',
                color: 'var(--primary)',
                cursor: 'pointer',
                padding: 0,
                fontSize: FONT_SIZE.xs,
              }}
              title={t(locale, 'fileBrowser.goToPath')}
            >
              <CornerDownLeft size={12} />
            </button>
          </>
        ) : (
          <button
            type="button"
            onClick={() => {
              setPathDraft(currentPath);
              setEditingPath(true);
            }}
            style={{
              flex: 1,
              minWidth: 0,
              border: 'none',
              background: 'transparent',
              color: 'var(--text)',
              textAlign: 'left',
              cursor: 'text',
              overflow: 'hidden',
              textOverflow: 'ellipsis',
              whiteSpace: 'nowrap',
              padding: 0,
              fontFamily: 'inherit',
              fontSize: 'inherit',
            }}
            title={`${currentPath}\n${t(locale, 'fileBrowser.editPathHint')}`}
          >
            {recentOpenedMode
              ? t(locale, 'fileBrowser.recentOpened')
              : recentMode
                ? t(locale, 'fileBrowser.recentMode')
                : currentPath}
          </button>
        )}
      </div>

      {/* Sort control — interactive menu when onSortChange is provided */}
      <div ref={sortRef} style={{ position: 'relative' }}>
        <button
          type="button"
          onClick={() => {
            if (!onSortChange) return;
            setSortOpen((v) => !v);
          }}
          disabled={!onSortChange}
          title={t(locale, 'fileBrowser.sort')}
          aria-haspopup="menu"
          aria-expanded={sortOpen}
          style={{
            display: 'inline-flex',
            alignItems: 'center',
            gap: 4,
            fontSize: FONT_SIZE.xs,
            color: onSortChange ? 'var(--text)' : 'var(--text-secondary)',
            padding: '3px 8px',
            borderRadius: 999,
            border: '1px solid var(--border)',
            background: sortOpen ? 'var(--primary-soft)' : 'var(--bg-2)',
            whiteSpace: 'nowrap',
            cursor: onSortChange ? 'pointer' : 'default',
            opacity: onSortChange ? 1 : 0.85,
          }}
        >
          <ArrowUpDown size={11} style={{ opacity: 0.7 }} />
          {sortLabel}
        </button>
        {sortOpen && onSortChange && (
          <div
            role="menu"
            style={{
              position: 'absolute',
              right: 0,
              top: '100%',
              marginTop: 4,
              zIndex: 50,
              minWidth: 168,
              borderRadius: BORDER_RADIUS.md,
              border: '1px solid var(--border)',
              background: 'var(--surface)',
              boxShadow: '0 8px 24px rgba(0,0,0,0.18)',
              padding: 4,
            }}
          >
            <div
              style={{
                padding: '4px 8px',
                fontSize: 10,
                fontWeight: 600,
                letterSpacing: '0.04em',
                textTransform: 'uppercase',
                color: 'var(--text-disabled)',
              }}
            >
              {t(locale, 'fileBrowser.sort')}
            </div>
            {([
              { key: 'name' as const, label: t(locale, 'fileBrowser.sortByName') },
              { key: 'mtime' as const, label: t(locale, 'fileBrowser.sortByModified') },
              { key: 'size' as const, label: t(locale, 'fileBrowser.sortBySize') },
            ]).map((opt) => {
              const active = sortBy === opt.key;
              return (
                <button
                  key={opt.key}
                  type="button"
                  role="menuitemradio"
                  aria-checked={active}
                  onClick={() => {
                    applySort(opt.key);
                    if (active) setSortOpen(false);
                  }}
                  style={{
                    display: 'flex',
                    width: '100%',
                    alignItems: 'center',
                    justifyContent: 'space-between',
                    gap: 8,
                    border: 'none',
                    borderRadius: BORDER_RADIUS.sm,
                    padding: '6px 10px',
                    fontSize: FONT_SIZE.xs,
                    cursor: 'pointer',
                    background: active ? 'var(--primary-soft)' : 'transparent',
                    color: active ? 'var(--primary)' : 'var(--text-secondary)',
                  }}
                >
                  <span>{opt.label}</span>
                  {active && (sortDir === 'asc' ? <ArrowUp size={12} /> : <ArrowDown size={12} />)}
                </button>
              );
            })}
            <div style={{ margin: '4px 8px', borderTop: '1px solid var(--border)' }} />
            <div
              style={{
                padding: '4px 8px',
                fontSize: 10,
                fontWeight: 600,
                letterSpacing: '0.04em',
                textTransform: 'uppercase',
                color: 'var(--text-disabled)',
              }}
            >
              {t(locale, 'fileBrowser.sortDirection')}
            </div>
            {([
              { key: 'asc' as const, label: t(locale, 'fileBrowser.ascending'), Icon: ArrowUp },
              { key: 'desc' as const, label: t(locale, 'fileBrowser.descending'), Icon: ArrowDown },
            ]).map((opt) => {
              const active = sortDir === opt.key;
              return (
                <button
                  key={opt.key}
                  type="button"
                  role="menuitemradio"
                  aria-checked={active}
                  onClick={() => {
                    applySort(sortBy, opt.key);
                    setSortOpen(false);
                  }}
                  style={{
                    display: 'flex',
                    width: '100%',
                    alignItems: 'center',
                    gap: 8,
                    border: 'none',
                    borderRadius: BORDER_RADIUS.sm,
                    padding: '6px 10px',
                    fontSize: FONT_SIZE.xs,
                    cursor: 'pointer',
                    background: active ? 'var(--primary-soft)' : 'transparent',
                    color: active ? 'var(--primary)' : 'var(--text-secondary)',
                  }}
                >
                  <opt.Icon size={12} />
                  <span>{opt.label}</span>
                </button>
              );
            })}
          </div>
        )}
      </div>

      {/* Recent + favorite */}
      <div style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
        <button
          type="button"
          style={{
            ...btnStyle(true),
            color: recentMode ? 'var(--primary)' : 'var(--text)',
            borderColor: recentMode ? 'var(--primary)' : 'var(--border)',
          }}
          onClick={onToggleRecent}
          title={recentMode ? t(locale, 'fileBrowser.recentModeTitle') : t(locale, 'fileBrowser.recentMode')}
        >
          <Clock size={13} />
        </button>
        <button
          type="button"
          style={{
            ...btnStyle(true),
            color: recentOpenedMode ? 'var(--primary)' : 'var(--text)',
            borderColor: recentOpenedMode ? 'var(--primary)' : 'var(--border)',
          }}
          onClick={onToggleRecentOpened}
          title={recentOpenedMode ? t(locale, 'fileBrowser.recentOpenedTitle') : t(locale, 'fileBrowser.recentOpened')}
        >
          <History size={13} />
        </button>
        <button
          type="button"
          style={{
            ...btnStyle(true),
            color: isFavorite ? 'var(--warning, #f5a524)' : 'var(--text)',
          }}
          onClick={onToggleFavorite}
          title={isFavorite ? t(locale, 'fileBrowser.removeFromFavorites') : t(locale, 'fileBrowser.addToFavorites')}
        >
          <Star size={13} fill={isFavorite ? 'currentColor' : 'none'} />
        </button>
      </div>

      {/* Local filter + global search */}
      <div style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
        <div
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: 6,
            height: 30,
            padding: '0 8px',
            borderRadius: BORDER_RADIUS.sm,
            border: '1px solid var(--border)',
            background: 'var(--bg-2)',
            minWidth: 140,
          }}
        >
          <Search size={12} style={{ color: 'var(--text-disabled)', flexShrink: 0 }} />
          <input
            ref={filterRef}
            value={searchQuery}
            onChange={(e) => onSearchChange(e.target.value)}
            placeholder={t(locale, 'fileBrowser.filterCurrent')}
            style={{
              flex: 1,
              minWidth: 0,
              border: 'none',
              outline: 'none',
              background: 'transparent',
              color: 'var(--text)',
              fontSize: FONT_SIZE.sm,
            }}
            aria-label={t(locale, 'fileBrowser.filterCurrent')}
          />
        </div>
        <button
          type="button"
          style={btnStyle(true)}
          onClick={onOpenGlobalSearch}
          title={`${t(locale, 'fileBrowser.globalSearch')} ⌘⇧F`}
        >
          <FolderSearch size={13} />
        </button>
      </div>
    </div>
  );
}
