'use client';

import { useState, useEffect, useCallback } from 'react';
import { type FileChangeEvent } from '@/types/agent';
import { t, useLocale } from '@/i18n';

// ── Noise filter: exclude system/generated files ──
const NOISE_PATTERNS = [
  /\.git\//, /\.svn\//, /\.hg\//,
  /node_modules\//, /\.next\//, /\.nuxt\//, /dist\//, /build\//, /out\//,
  /__pycache__\//, /\.pyc$/, /\.pyo$/,
  /\.DS_Store$/, /Thumbs\.db$/, /desktop\.ini$/,
  /\.cache\//, /\.tmp\//, /\.temp\//,
  /\.db-wal$/, /\.db-shm$/, /\.db-journal$/,
  /\.lock$/, /package-lock\.json$/, /yarn\.lock$/, /pnpm-lock\.yaml$/,
];

function isNoisyChange(path: string): boolean {
  return NOISE_PATTERNS.some(p => p.test(path));
}

interface ChangeItem extends FileChangeEvent {
  project?: string;
  count: number;
}

export default function ChangeInbox() {
  const [items, setItems] = useState<ChangeItem[]>([]);
  const [showFiltered, setShowFiltered] = useState(false);
  const locale = useLocale();

  // Listen for file change events
  useEffect(() => {
    const api = window.nativesAPI;
    if (!api?.onDbStateChanged) return;
    const unsub = api.onDbStateChanged((_event, channel, data: unknown) => {
      if (channel === 'file:changed' && data && typeof data === 'object' && 'path' in data) {
        const d = data as { path?: string; type?: string; project?: string };
        if (!d.path) return;

        // Noise filter
        if (!showFiltered && isNoisyChange(d.path)) return;

        const eventType = (['create', 'modify', 'delete'].includes(d.type || '') ? d.type : 'modify') as 'create' | 'modify' | 'delete';
        const parts = d.path.split('/');
        const project = d.project || parts[parts.length - 2] || 'Unknown';

        setItems((prev) => {
          // Deduplicate by path: increment count if exists
          const existing = prev.find(p => p.path === d.path);
          if (existing) {
            return prev.map(p =>
              p.path === d.path ? { ...p, count: p.count + 1, timestamp: Date.now(), type: eventType } : p
            );
          }
          return [
            { path: d.path!, type: eventType, timestamp: Date.now(), project, count: 1 },
            ...prev,
          ].slice(0, 100);
        });
      }
    });
    return unsub;
  }, [showFiltered]);

  const handleClear = useCallback(() => setItems([]), []);

  const handleNavigate = useCallback((path: string) => {
    const dir = path.substring(0, path.lastIndexOf('/')) || '/';
    window.dispatchEvent(new CustomEvent('navigate-files', { detail: dir }));
  }, []);

  // Sort by timestamp (most recent first)
  const sorted = [...items].sort((a, b) => b.timestamp - a.timestamp);

  const groupedByProject = sorted.reduce<Record<string, ChangeItem[]>>((acc, item) => {
    const project = item.project || 'Unknown';
    if (!acc[project]) acc[project] = [];
    acc[project]!.push(item);
    return acc;
  }, {});

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
      {/* Header */}
      <div style={{
        padding: '8px 10px', borderBottom: '1px solid var(--border,#262920)',
        display: 'flex', alignItems: 'center', justifyContent: 'space-between',
      }}>
        <div style={{ fontSize: 11, fontWeight: 600, color: 'var(--text-dim)', textTransform: 'uppercase', letterSpacing: 0.5 }}>
          {t(locale, 'aiWorkbench.changeInbox')} ({items.length})
        </div>
        <div style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
          <button
            className="btn-ghost"
            onClick={() => setShowFiltered(!showFiltered)}
            style={{ fontSize: 10, padding: '2px 6px', color: showFiltered ? 'var(--accent)' : 'var(--text-faint)' }}
            title={showFiltered ? 'Hide system files' : 'Show system files'}
          >
            {showFiltered ? '👁' : '👁‍🗨'}
          </button>
          {items.length > 0 && (
            <button className="btn-ghost" onClick={handleClear} style={{ fontSize: 10, padding: '2px 6px', color: 'var(--text-faint)' }}>
              {t(locale, 'notifications.clear')}
            </button>
          )}
        </div>
      </div>

      {/* Changes list */}
      <div style={{ flex: 1, overflow: 'auto', padding: 6 }}>
        {items.length === 0 ? (
          <div style={{ padding: 20, textAlign: 'center', color: 'var(--text-faint)', fontSize: 'var(--fs-sm)' }}>
            {t(locale, 'aiWorkbench.noChanges')}
          </div>
        ) : (
          Object.entries(groupedByProject).map(([project, changes]) => (
            <div key={project} style={{ marginBottom: 10 }}>
              <div style={{ fontSize: 10, fontWeight: 600, color: 'var(--text-dim)', marginBottom: 4, padding: '0 4px' }}>
                📂 {project}
              </div>
              {changes.map((ch, i) => (
                <div
                  key={`${ch.path}-${i}`}
                  onClick={() => handleNavigate(ch.path)}
                  style={{
                    padding: '4px 8px', fontSize: 11, color: 'var(--text)',
                    borderRadius: 4, cursor: 'pointer', marginBottom: 2,
                    background: 'var(--bg-2,#131410)',
                    borderLeft: `3px solid ${ch.type === 'create' ? 'var(--diff-add)' : ch.type === 'delete' ? 'var(--danger)' : 'var(--warning)'}`,
                    display: 'flex', alignItems: 'center', gap: 4,
                  }}
                >
                  <span style={{ flexShrink: 0 }}>
                    {ch.type === 'create' ? '🟢' : ch.type === 'delete' ? '🔴' : '🟡'}
                  </span>
                  <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', flex: 1 }}>
                    {ch.path.split('/').pop()}
                  </span>
                  {ch.count > 1 && (
                    <span style={{
                      fontSize: 9, fontWeight: 600, color: 'var(--accent)',
                      background: 'var(--accent-soft)', padding: '0 4px', borderRadius: 3,
                      flexShrink: 0,
                    }}>
                      ×{ch.count}
                    </span>
                  )}
                </div>
              ))}
            </div>
          ))
        )}
      </div>
    </div>
  );
}
