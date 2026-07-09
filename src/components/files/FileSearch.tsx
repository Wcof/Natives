'use client';

import { useState } from 'react';
import { type ContentSearchResult } from '@/types/file';
import { SPACING, FONT_SIZE, BORDER_RADIUS } from '@/lib/design-tokens';
import { useLocale, t as tr } from '@/i18n';
import { useToast } from '@/components/ui/Toast';
import { classifyError } from '@/lib/error-classifier';

interface FileSearchProps {
  onClose: () => void;
  onNavigate: (path: string) => void;
}

export default function FileSearch({ onClose, onNavigate }: FileSearchProps) {
  const locale = useLocale();
  const t = (key: string, params?: Record<string, string | number>) => tr(locale, key, params);
  const { toast } = useToast();
  const [query, setQuery] = useState('');
  const [results, setResults] = useState<ContentSearchResult[]>([]);
  const [searching, setSearching] = useState(false);
  const [mode, setMode] = useState<'name' | 'content'>('name');

  const handleSearch = async (q: string) => {
    setQuery(q);
    if (!q.trim()) {
      setResults([]);
      return;
    }

    setSearching(true);
    try {
      const api = window.nativesAPI;
      const searchApi = api?.search;
      if (!searchApi) {
        setResults([]);
        return;
      }

      let data: any;
      if (mode === 'content') {
        data = await searchApi.grep(q, '/', { maxResults: 50 });
      } else {
        data = await searchApi.files(q, '/', { maxResults: 50 });
      }

      // Normalize response to ContentSearchResult[]
      // Rust SearchResult: { path, line, text, score, mtime }
      const items = Array.isArray(data)
        ? data.map((item: any) => ({
            path: item.path ?? '',
            name: item.name ?? item.path?.split('/').pop() ?? '',
            line: item.line ?? 0,
            preview: item.preview ?? item.text ?? item.content ?? '',
            matchStart: item.matchStart ?? 0,
            matchEnd: item.matchEnd ?? 0,
            score: item.score ?? undefined,
            mtime: item.mtime ?? undefined,
          }))
        : [];
      // Sort by score descending when available (fuzzy search results)
      if (items.some((i) => i.score != null)) {
        items.sort((a, b) => (b.score ?? 0) - (a.score ?? 0));
      }
      setResults(items);
    } catch (err) {
      toast(classifyError(err).userMessage, 'error');
      setResults([]);
    } finally {
      setSearching(false);
    }
  };

  return (
    <div style={{
      position: 'fixed', inset: 0, zIndex: 999,
      display: 'flex', alignItems: 'flex-start', justifyContent: 'center',
      paddingTop: '15vh',
      background: 'rgba(0,0,0,0.6)',
    }}
      onClick={(e) => { if (e.target === e.currentTarget) onClose(); }}
    >
      <div style={{
        width: 560, maxHeight: '60vh', overflow: 'hidden',
        background: 'var(--panel, #0e0f0c)',
        border: '1px solid var(--border)',
        borderRadius: 'var(--radius, 4px)',
        display: 'flex', flexDirection: 'column',
        boxShadow: '0 16px 48px rgba(0,0,0,0.4)',
      }}>
        {/* Search input */}
        <div style={{ padding: 12, borderBottom: '1px solid var(--border)' }}>
          <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
            <input
              className="input"
              type="text"
              placeholder={t('fileBrowser.searchByMode', {
                mode: mode === 'content' ? t('fileBrowser.searchModeContent') : t('fileBrowser.searchModeName'),
              })}
              value={query}
              onChange={(e) => {
                const val = e.target.value;
                if (val.startsWith('content:')) {
                  setMode('content');
                  handleSearch(val.slice(8));
                } else {
                  setMode('name');
                  handleSearch(val);
                }
              }}
              autoFocus
              style={{ flex: 1, fontSize: FONT_SIZE.xl, padding: `${SPACING.sm}px 10px` }}
            />
            <span style={{
              fontSize: FONT_SIZE.xs, padding: '2px 6px', borderRadius: BORDER_RADIUS.sm,
              background: mode === 'content' ? 'var(--primary)' : 'var(--surface)',
              color: mode === 'content' ? '#FFFFFF' : 'var(--text-secondary)',
            }}>
              {mode === 'content' ? t('fileBrowser.searchModeContent') : t('fileBrowser.searchModeName')}
            </span>
          </div>
        </div>

        {/* Results */}
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
            results.slice(0, 50).map((r, idx) => (
              <div
                key={`${r.path}-${r.line}-${idx}`}
                onClick={() => { onNavigate(r.path); onClose(); }}
                style={{
                  padding: '8px 12px',
                  cursor: 'pointer',
                  borderBottom: '1px solid var(--border)',
                  transition: 'background 0.08s',
                }}
                onMouseEnter={(e) => { (e.currentTarget as HTMLElement).style.background = 'var(--surface)'; }}
                onMouseLeave={(e) => { (e.currentTarget as HTMLElement).style.background = 'transparent'; }}
              >
                <div style={{ fontSize: FONT_SIZE.md, color: 'var(--text)', marginBottom: 2, display: 'flex', alignItems: 'center', gap: 8 }}>
                  {r.name}
                  {mode === 'content' && <span style={{ color: 'var(--text-secondary)' }}>{t('fileBrowser.lineNumber', { line: r.line })}</span>}
                  {r.score != null && r.score > 0 && (
                    <span style={{
                      fontSize: FONT_SIZE.xs, color: 'var(--text-secondary)',
                      background: 'var(--surface)', padding: '1px 5px',
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
          {results.length > 50 && (
            <div style={{ padding: '6px 12px', fontSize: FONT_SIZE.sm, color: 'var(--text-secondary)', textAlign: 'center' }}>
              {t('fileBrowser.showingResults', { count: results.length })}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
