'use client';

import { useState } from 'react';
import { type ContentSearchResult } from '@/types/file';
import { SPACING, FONT_SIZE, BORDER_RADIUS } from '@/lib/design-tokens';

interface FileSearchProps {
  onClose: () => void;
  onNavigate: (path: string) => void;
}

export default function FileSearch({ onClose, onNavigate }: FileSearchProps) {
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
    } catch {
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
        border: '1px solid var(--vibe-btn-border)',
        borderRadius: 'var(--radius, 4px)',
        display: 'flex', flexDirection: 'column',
        boxShadow: '0 16px 48px rgba(0,0,0,0.4)',
      }}>
        {/* Search input */}
        <div style={{ padding: 12, borderBottom: '1px solid var(--vibe-btn-border)' }}>
          <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
            <input
              className="input"
              type="text"
              placeholder={`Search by ${mode}... (prefix with "content:" for full-text)`}
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
              background: mode === 'content' ? 'var(--accent)' : 'var(--vibe-btn-bg)',
              color: mode === 'content' ? 'var(--accent-ink)' : 'var(--vibe-btn-text)',
            }}>
              {mode === 'content' ? 'content' : 'name'}
            </span>
          </div>
        </div>

        {/* Results */}
        <div style={{ flex: 1, overflow: 'auto' }}>
          {searching ? (
            <div style={{ padding: SPACING.xl, textAlign: 'center', color: 'var(--vibe-btn-text)' }}>
              Searching...
            </div>
          ) : results.length === 0 && query ? (
            <div style={{ padding: SPACING.xl, textAlign: 'center', color: 'var(--vibe-btn-text)' }}>
              No results for "{query}"
            </div>
          ) : (
            results.slice(0, 50).map((r, idx) => (
              <div
                key={`${r.path}-${r.line}-${idx}`}
                onClick={() => { onNavigate(r.path); onClose(); }}
                style={{
                  padding: '8px 12px',
                  cursor: 'pointer',
                  borderBottom: '1px solid var(--vibe-btn-border)',
                  transition: 'background 0.08s',
                }}
                onMouseEnter={(e) => { (e.currentTarget as HTMLElement).style.background = 'var(--vibe-toolbar-bg)'; }}
                onMouseLeave={(e) => { (e.currentTarget as HTMLElement).style.background = 'transparent'; }}
              >
                <div style={{ fontSize: FONT_SIZE.md, color: 'var(--vibe-brand-text)', marginBottom: 2, display: 'flex', alignItems: 'center', gap: 8 }}>
                  {r.name}
                  {mode === 'content' && <span style={{ color: 'var(--vibe-btn-text)' }}>line {r.line}</span>}
                  {r.score != null && r.score > 0 && (
                    <span style={{
                      fontSize: FONT_SIZE.xs, color: 'var(--vibe-btn-text)',
                      background: 'var(--vibe-btn-bg)', padding: '1px 5px',
                      borderRadius: BORDER_RADIUS.sm, marginLeft: 'auto',
                    }}>
                      {r.score.toFixed(1)}
                    </span>
                  )}
                </div>
                <div style={{ fontSize: FONT_SIZE.xs, color: 'var(--vibe-btn-text)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                  {r.path}
                </div>
                {mode === 'content' && r.preview && (
                  <div style={{
                    fontSize: FONT_SIZE.sm, color: 'var(--vibe-btn-text)',
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
            <div style={{ padding: '6px 12px', fontSize: FONT_SIZE.sm, color: 'var(--vibe-btn-text)', textAlign: 'center' }}>
              Showing 50 of {results.length} results
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
