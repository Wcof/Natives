'use client';

import { useState } from 'react';
import { type ContentSearchResult } from '@/types/file';

interface FileSearchProps {
  onClose: () => void;
  onNavigate: (path: string) => void;
}

export default function FileSearch({ onClose, onNavigate }: FileSearchProps) {
  const [query, setQuery] = useState('');
  const [results, setResults] = useState<ContentSearchResult[]>([]);
  const [searching, setSearching] = useState(false);
  const [mode, setMode] = useState<'name' | 'content'>('name');

  const httpPort = window.__nativesHttpPort || 3001;

  const handleSearch = async (q: string) => {
    setQuery(q);
    if (!q.trim()) {
      setResults([]);
      return;
    }

    setSearching(true);
    try {
      const endpoint = mode === 'content' ? 'grep' : 'search';
      const res = await fetch(`http://localhost:${httpPort}/api/fs/${endpoint}?q=${encodeURIComponent(q)}&root=${encodeURIComponent('/')}`);
      if (res.ok) {
        const data = await res.json();
        setResults(Array.isArray(data) ? data : []);
      }
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
        border: '1px solid var(--border, #262920)',
        borderRadius: 'var(--radius, 4px)',
        display: 'flex', flexDirection: 'column',
        boxShadow: '0 16px 48px rgba(0,0,0,0.4)',
      }}>
        {/* Search input */}
        <div style={{ padding: 12, borderBottom: '1px solid var(--border, #262920)' }}>
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
              style={{ flex: 1, fontSize: 14, padding: '8px 10px' }}
            />
            <span style={{
              fontSize: 10, padding: '2px 6px', borderRadius: 3,
              background: mode === 'content' ? 'var(--accent, #FFF5E6)' : 'var(--bg-3, #1c1e17)',
              color: mode === 'content' ? 'var(--accent-ink, #0b0c0a)' : 'var(--text-dim, #9b9d8c)',
            }}>
              {mode === 'content' ? 'content' : 'name'}
            </span>
          </div>
        </div>

        {/* Results */}
        <div style={{ flex: 1, overflow: 'auto' }}>
          {searching ? (
            <div style={{ padding: 20, textAlign: 'center', color: 'var(--text-faint, #62655a)' }}>
              Searching...
            </div>
          ) : results.length === 0 && query ? (
            <div style={{ padding: 20, textAlign: 'center', color: 'var(--text-faint, #62655a)' }}>
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
                  borderBottom: '1px solid var(--border, #262920)',
                  transition: 'background 0.08s',
                }}
                onMouseEnter={(e) => { (e.currentTarget as HTMLElement).style.background = 'var(--bg-2, #131410)'; }}
                onMouseLeave={(e) => { (e.currentTarget as HTMLElement).style.background = 'transparent'; }}
              >
                <div style={{ fontSize: 12, color: 'var(--text, #f2f2ea)', marginBottom: 2 }}>
                  {r.name}
                  {mode === 'content' && <span style={{ color: 'var(--text-faint, #62655a)', marginLeft: 8 }}>line {r.line}</span>}
                </div>
                <div style={{ fontSize: 10, color: 'var(--text-faint, #62655a)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                  {r.path}
                </div>
                {mode === 'content' && r.preview && (
                  <div style={{
                    fontSize: 11, color: 'var(--text-dim, #9b9d8c)',
                    marginTop: 4, fontFamily: 'monospace',
                    overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
                  }}>
                    {r.preview}
                  </div>
                )}
              </div>
            ))
          )}
          {results.length > 50 && (
            <div style={{ padding: '6px 12px', fontSize: 11, color: 'var(--text-faint, #62655a)', textAlign: 'center' }}>
              Showing 50 of {results.length} results
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
