'use client';

import { useEffect, useRef, useState } from 'react';
import { type ContentSearchResult } from '@/types/file';
import { type SearchResult as GeneratedSearchResult } from '@/types/generated/SearchResult';
import { SPACING, FONT_SIZE, BORDER_RADIUS } from '@/lib/design-tokens';
import { useLocale, t as tr } from '@/i18n';
import { useToast } from '@/components/ui/Toast';
import { classifyError } from '@/lib/error-classifier';
import { fsApi, searchApi, hasNativeFiles } from '@/lib/files-api';

interface FileSearchProps {
  onClose: () => void;
  onNavigate: (path: string) => void | Promise<void>;
  /** Search root; defaults to home-ish "/" when omitted. */
  rootPath?: string;
}

export default function FileSearch({ onClose, onNavigate, rootPath = '/' }: FileSearchProps) {
  const locale = useLocale();
  const t = (key: string, params?: Record<string, string | number>) => tr(locale, key, params);
  const { toast } = useToast();
  const [query, setQuery] = useState('');
  const [results, setResults] = useState<ContentSearchResult[]>([]);
  const [searching, setSearching] = useState(false);
  const [mode, setMode] = useState<'name' | 'content'>('name');
  const [scope, setScope] = useState<'here' | 'home'>('here');
  const [activeIndex, setActiveIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const debounceRef = useRef<number | null>(null);
  const requestIdRef = useRef(0);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        onClose();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  const resolveRoot = async (): Promise<string> => {
    if (scope === 'here' && rootPath) return rootPath;
    try {
      const roots = await fsApi().roots() as Array<{ id?: string; path?: string }>;
      const home = Array.isArray(roots) ? roots.find((r) => r.id === 'home') : null;
      if (home?.path) return home.path;
    } catch { /* ignore */ }
    return rootPath || '/';
  };

  const runSearch = async (q: string, nextMode: 'name' | 'content' = mode) => {
    setQuery(q);
    setMode(nextMode);
    if (!q.trim()) {
      setResults([]);
      setSearching(false);
      return;
    }

    const rid = ++requestIdRef.current;
    setSearching(true);
    try {
      if (!hasNativeFiles()) {
        setResults([]);
        return;
      }
      const search = searchApi();

      const root = await resolveRoot();
      let data: GeneratedSearchResult[];
      if (nextMode === 'content') {
        data = (await search.grep(q, root, { maxResults: 80 })) as GeneratedSearchResult[];
      } else {
        data = (await search.files(q, root, { maxResults: 80 })) as GeneratedSearchResult[];
      }
      if (rid !== requestIdRef.current) return;

      // T217 (P2-001): consume the generated wire type directly — no `any`
      // manual compatibility fields. UI-only fields are derived, never read
      // from unknown/any.
      const items = Array.isArray(data)
        ? data.map((item) => ({
            path: item.path ?? '',
            name: item.path?.split('/').pop() ?? '',
            line: item.line ?? 0,
            preview: item.text ?? '',
            matchStart: 0,
            matchEnd: 0,
            score: item.score ?? undefined,
            mtime: item.mtime ?? undefined,
          }))
        : [];
      if (items.some((i) => i.score != null)) {
        items.sort((a, b) => (b.score ?? 0) - (a.score ?? 0));
      }
      setResults(items);
      setActiveIndex(0);
    } catch (err) {
      if (rid !== requestIdRef.current) return;
      toast(classifyError(err).userMessage, 'error');
      setResults([]);
    } finally {
      if (rid === requestIdRef.current) setSearching(false);
    }
  };

  const scheduleSearch = (q: string, nextMode: 'name' | 'content' = mode) => {
    setQuery(q);
    setMode(nextMode);
    if (debounceRef.current) window.clearTimeout(debounceRef.current);
    if (!q.trim()) {
      setResults([]);
      setSearching(false);
      return;
    }
    setSearching(true);
    debounceRef.current = window.setTimeout(() => {
      void runSearch(q, nextMode);
    }, 220);
  };

  useEffect(() => {
    return () => {
      if (debounceRef.current) window.clearTimeout(debounceRef.current);
    };
  }, []);

  // Re-run when scope changes with existing query
  useEffect(() => {
    if (query.trim()) void runSearch(query, mode);

  }, [scope]);

  const openResult = async (path: string) => {
    await onNavigate(path);
    onClose();
  };

  return (
    <div
      style={{
        position: 'fixed', inset: 0, zIndex: 999,
        display: 'flex', alignItems: 'flex-start', justifyContent: 'center',
        paddingTop: '12vh',
        background: 'var(--overlay-strong)',
      }}
      onClick={(e) => { if (e.target === e.currentTarget) onClose(); }}
    >
      <div
        style={{
          width: 620, maxHeight: '68vh', overflow: 'hidden',
          background: 'var(--panel)',
          border: '1px solid var(--border)',
          borderRadius: 'var(--radius, 8px)',
          display: 'flex', flexDirection: 'column',
          boxShadow: 'var(--shadow-modal)',
        }}
      >
        <div style={{ padding: 12, borderBottom: '1px solid var(--border)', display: 'flex', flexDirection: 'column', gap: 8 }}>
          <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
            <input
              ref={inputRef}
              className="input"
              type="text"
              placeholder={t('fileBrowser.searchByMode', {
                mode: mode === 'content' ? t('fileBrowser.searchModeContent') : t('fileBrowser.searchModeName'),
              })}
              value={query}
              onChange={(e) => {
                const val = e.target.value;
                if (val.startsWith('content:')) {
                  scheduleSearch(val.slice(8), 'content');
                } else {
                  scheduleSearch(val, 'name');
                }
              }}
              onKeyDown={(e) => {
                if (e.key === 'ArrowDown') {
                  e.preventDefault();
                  setActiveIndex((i) => Math.min(i + 1, Math.max(results.length - 1, 0)));
                } else if (e.key === 'ArrowUp') {
                  e.preventDefault();
                  setActiveIndex((i) => Math.max(i - 1, 0));
                } else if (e.key === 'Enter') {
                  e.preventDefault();
                  const hit = results[activeIndex];
                  if (hit?.path) void openResult(hit.path);
                } else if (e.key === 'Tab') {
                  e.preventDefault();
                  const next = mode === 'name' ? 'content' : 'name';
                  scheduleSearch(query, next);
                }
              }}
              style={{ flex: 1, fontSize: FONT_SIZE.xl, padding: `${SPACING.sm}px 10px` }}
            />
            <button
              type="button"
              onClick={() => scheduleSearch(query, mode === 'name' ? 'content' : 'name')}
              style={{
                fontSize: FONT_SIZE.xs, padding: '4px 8px', borderRadius: BORDER_RADIUS.sm,
                border: '1px solid var(--border)',
                background: mode === 'content' ? 'var(--primary)' : 'var(--surface)',
                color: mode === 'content' ? 'var(--text)' : 'var(--text-secondary)',
                cursor: 'pointer',
              }}
              title={t('fileBrowser.toggleSearchMode')}
            >
              {mode === 'content' ? t('fileBrowser.searchModeContent') : t('fileBrowser.searchModeName')}
            </button>
          </div>
          <div style={{ display: 'flex', alignItems: 'center', gap: 8, fontSize: FONT_SIZE.xs, color: 'var(--text-secondary)' }}>
            <span>{t('fileBrowser.searchScope')}:</span>
            <button
              type="button"
              onClick={() => setScope('here')}
              style={{
                border: '1px solid var(--border)', borderRadius: 999, padding: '2px 8px', cursor: 'pointer',
                background: scope === 'here' ? 'var(--primary-soft)' : 'transparent',
                color: scope === 'here' ? 'var(--primary)' : 'inherit',
              }}
            >
              {t('fileBrowser.searchScopeHere')}
            </button>
            <button
              type="button"
              onClick={() => setScope('home')}
              style={{
                border: '1px solid var(--border)', borderRadius: 999, padding: '2px 8px', cursor: 'pointer',
                background: scope === 'home' ? 'var(--primary-soft)' : 'transparent',
                color: scope === 'home' ? 'var(--primary)' : 'inherit',
              }}
            >
              {t('fileBrowser.searchScopeHome')}
            </button>
            <span style={{ marginLeft: 'auto', opacity: 0.75 }}>
              {scope === 'here' ? rootPath : '~'} · Tab {t('fileBrowser.toggleSearchMode')}
            </span>
          </div>
        </div>

        <div style={{ flex: 1, overflow: 'auto' }}>
          {searching ? (
            <div style={{ padding: SPACING.xl, textAlign: 'center', color: 'var(--text-secondary)' }}>
              {t('fileBrowser.searching')}
            </div>
          ) : results.length === 0 && query ? (
            <div style={{ padding: SPACING.xl, textAlign: 'center', color: 'var(--text-secondary)' }}>
              {t('fileBrowser.noResultsFor', { query })}
            </div>
          ) : (
            results.slice(0, 80).map((r, idx) => (
              <div
                key={`${r.path}-${r.line}-${idx}`}
                onClick={() => { void openResult(r.path); }}
                style={{
                  padding: '8px 12px',
                  cursor: 'pointer',
                  borderBottom: '1px solid var(--border)',
                  background: idx === activeIndex ? 'var(--surface)' : 'transparent',
                  transition: 'background 0.08s',
                }}
                onMouseEnter={() => setActiveIndex(idx)}
              >
                <div style={{ fontSize: FONT_SIZE.md, color: 'var(--text)', marginBottom: 2, display: 'flex', alignItems: 'center', gap: 8 }}>
                  {r.name}
                  {mode === 'content' && <span style={{ color: 'var(--text-secondary)' }}>{t('fileBrowser.lineNumber', { line: r.line })}</span>}
                  {r.score != null && r.score > 0 && (
                    <span style={{
                      fontSize: FONT_SIZE.xs, color: 'var(--text-secondary)',
                      background: 'var(--bg-2)', padding: '1px 5px',
                      borderRadius: BORDER_RADIUS.sm, marginLeft: 'auto',
                    }}>
                      {r.score.toFixed(1)}
                    </span>
                  )}
                </div>
                <div style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-secondary)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                  {r.path}
                </div>
                {mode === 'content' && r.preview && (
                  <div style={{
                    fontSize: FONT_SIZE.sm, color: 'var(--text-secondary)',
                    marginTop: SPACING.xs, fontFamily: 'monospace',
                    overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
                  }}>
                    {r.preview}
                  </div>
                )}
              </div>
            ))
          )}
          {results.length > 80 && (
            <div style={{ padding: '6px 12px', fontSize: FONT_SIZE.sm, color: 'var(--text-secondary)', textAlign: 'center' }}>
              {t('fileBrowser.showingResults', { count: results.length })}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
