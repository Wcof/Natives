'use client';

import { useEffect, useRef, useState, type CSSProperties } from 'react';
import {
  ArrowLeft,
  ArrowRight,
  ArrowUp,
  RefreshCw,
  Star,
  Clock,
  Search,
  FolderSearch,
  CornerDownLeft,
} from 'lucide-react';
import { t, type Locale } from '@/i18n';
import { FONT_SIZE, SPACING, BORDER_RADIUS } from '@/lib/design-tokens';

export interface FileNavShellProps {
  currentPath: string;
  canGoBack: boolean;
  canGoForward: boolean;
  canGoUp: boolean;
  isFavorite: boolean;
  recentMode: boolean;
  searchQuery: string;
  sortBy: 'name' | 'mtime' | 'size';
  sortDir: 'asc' | 'desc';
  loading?: boolean;
  onBack: () => void;
  onForward: () => void;
  onUp: () => void;
  onRefresh: () => void;
  onToggleFavorite: () => void;
  onToggleRecent: () => void;
  onSearchChange: (query: string) => void;
  onOpenGlobalSearch: () => void;
  onPathSubmit: (path: string) => void | Promise<void>;
}

export default function FileNavShell({
  currentPath,
  canGoBack,
  canGoForward,
  canGoUp,
  isFavorite,
  recentMode,
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
  onSearchChange,
  onOpenGlobalSearch,
  onPathSubmit,
}: FileNavShellProps) {
  const [locale, setLocale] = useState<Locale>('zh');
  const [editingPath, setEditingPath] = useState(false);
  const [pathDraft, setPathDraft] = useState(currentPath);
  const pathInputRef = useRef<HTMLInputElement>(null);
  const filterRef = useRef<HTMLInputElement>(null);

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

  const sortLabel = (() => {
    const field =
      sortBy === 'mtime'
        ? t(locale, 'fileBrowser.sortByModified')
        : sortBy === 'size'
          ? t(locale, 'fileBrowser.sortBySize')
          : t(locale, 'fileBrowser.sortByName');
    const dir = sortDir === 'asc' ? '↑' : '↓';
    return `${field} ${dir}`;
  })();

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
            {recentMode ? t(locale, 'fileBrowser.recentMode') : currentPath}
          </button>
        )}
      </div>

      {/* Sort status chip */}
      <span
        style={{
          fontSize: FONT_SIZE.xs,
          color: 'var(--text-secondary)',
          padding: '3px 8px',
          borderRadius: 999,
          border: '1px solid var(--border)',
          background: 'var(--bg-2)',
          whiteSpace: 'nowrap',
        }}
        title={t(locale, 'fileBrowser.sort')}
      >
        {sortLabel}
      </span>

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
