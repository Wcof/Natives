'use client';

import { useState, useEffect, useCallback } from 'react';
import { type FileChangeEvent } from '@/types/agent';
import { t, type Locale } from '@/i18n';

export default function AgentDashboard({ sessionId }: { sessionId?: string }) {
  const [changes, setChanges] = useState<FileChangeEvent[]>([]);
  const [paused, setPaused] = useState(false);
  const [filter, setFilter] = useState<'all' | 'create' | 'modify' | 'delete'>('all');
  const [locale, setLocale] = useState<Locale>('zh');

  useEffect(() => {
    async function loadLocale() {
      try {
        const saved = await window.nativesAPI?.getLocale?.();
        if (saved) setLocale(saved === 'en' ? 'en' : 'zh');
      } catch { /* ignore */ }
    }
    loadLocale();
  }, []);

  // Listen for file change events
  useEffect(() => {
    if (paused) return;
    const api = window.nativesAPI;
    if (!api?.onDbStateChanged) return;
    const unsub = api.onDbStateChanged((_event, channel, data: unknown) => {
      if (channel === 'file:changed' && data && typeof data === 'object' && 'path' in data) {
        const d = data as { path?: string; type?: string };
        if (d.path) {
          const eventType = (['create', 'modify', 'delete'].includes(d.type || '') ? d.type : 'modify') as 'create' | 'modify' | 'delete';
          setChanges((prev) => [
            { path: d.path!, type: eventType, timestamp: Date.now(), sessionId },
            ...prev,
          ].slice(0, 100));
        }
      }
    });
    return unsub;
  }, [sessionId, paused]);

  const handleClear = useCallback(() => setChanges([]), []);

  const handleNavigate = useCallback((path: string) => {
    const dir = path.substring(0, path.lastIndexOf('/')) || '/';
    window.dispatchEvent(new CustomEvent('navigate-files', { detail: dir }));
  }, []);

  const filtered = filter === 'all' ? changes : changes.filter((c) => c.type === filter);
  const getIntensity = (idx: number) => Math.max(0.2, 1 - idx * 0.03);

  const typeIcons: Record<string, string> = { create: '+', delete: '−', modify: '✎' };
  const typeColors: Record<string, string> = { create: '#4ec9b0', delete: '#d9534f', modify: 'var(--accent,#cdf24b)' };

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
      {/* Header */}
      <div style={{
        padding: '8px 10px',
        borderBottom: '1px solid var(--border,#262920)',
        display: 'flex', alignItems: 'center', justifyContent: 'space-between',
      }}>
        <div style={{ fontSize: 11, fontWeight: 600, color: 'var(--text-dim)', textTransform: 'uppercase', letterSpacing: 0.5 }}>
          {t(locale, 'aiWorkbench.agentChanges')}
        </div>
        <div style={{ display: 'flex', gap: 4 }}>
          {/* Filter */}
          {(['all', 'create', 'modify', 'delete'] as const).map((f) => (
            <button
              key={f}
              className="btn-ghost"
              onClick={() => setFilter(f)}
              style={{
                fontSize: 10, padding: '2px 6px', borderRadius: 3,
                color: filter === f ? 'var(--accent,#cdf24b)' : 'var(--text-faint)',
                background: filter === f ? 'var(--accent-soft,#cdf24b1f)' : 'transparent',
              }}
            >
              {f === 'all' ? t(locale, 'aiWorkbench.dashboard.all') : `${typeIcons[f]} ${t(locale, 'aiWorkbench.dashboard.' + f)}`}
            </button>
          ))}
          {/* Pause/Resume */}
          <button
            className="btn-ghost"
            onClick={() => setPaused(!paused)}
            style={{
              fontSize: 10, padding: '2px 6px', borderRadius: 3,
              color: paused ? '#e6b800' : 'var(--text-faint)',
            }}
            title={paused ? t(locale, 'aiWorkbench.dashboard.resume') : t(locale, 'aiWorkbench.dashboard.pause')}
          >
            {paused ? '▶' : '⏸'}
          </button>
          {/* Clear */}
          <button
            className="btn-ghost"
            onClick={handleClear}
            style={{ fontSize: 10, padding: '2px 6px', borderRadius: 3, color: 'var(--text-faint)' }}
            title={t(locale, 'aiWorkbench.dashboard.clear')}
          >
            ✕
          </button>
        </div>
      </div>

      {/* Changes list */}
      <div style={{ flex: 1, overflow: 'auto', padding: 6 }}>
        {filtered.length === 0 ? (
          <div style={{ padding: 20, textAlign: 'center', color: 'var(--text-faint)', fontSize: 12 }}>
            {paused ? t(locale, 'aiWorkbench.dashboard.paused') : t(locale, 'aiWorkbench.waitingForChanges')}
          </div>
        ) : (
          filtered.map((ch, i) => (
            <div
              key={`${ch.path}-${ch.timestamp}-${i}`}
              onClick={() => handleNavigate(ch.path)}
              style={{
                padding: '5px 8px', marginBottom: 3, borderRadius: 4, cursor: 'pointer',
                fontSize: 11, transition: 'opacity 0.3s',
                background: `rgba(205, 242, 75, ${getIntensity(i) * 0.1})`,
                borderLeft: `3px solid ${typeColors[ch.type] || typeColors.modify}${Math.round(getIntensity(i) * 255).toString(16).padStart(2, '0')}`,
                animation: i === 0 ? 'livePulse 1.1s ease-in-out infinite' : undefined,
                color: 'var(--text)',
                overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
              }}
              title={ch.path}
            >
              <span style={{ fontSize: 9, marginRight: 4, color: typeColors[ch.type] }}>
                {typeIcons[ch.type] || '✎'}
              </span>
              <span style={{ opacity: 0.5, fontSize: 10, marginRight: 4 }}>
                {ch.path.split('/').slice(-2, -1)[0]}/
              </span>
              {ch.path.split('/').pop()}
            </div>
          ))
        )}
      </div>

      {/* Footer */}
      <div style={{
        padding: '4px 10px', borderTop: '1px solid var(--border,#262920)',
        fontSize: 10, color: 'var(--text-faint)', display: 'flex', justifyContent: 'space-between',
      }}>
        <span>{t(locale, 'aiWorkbench.dashboard.changesCount').replace('{n}', String(filtered.length))}</span>
        {paused && <span style={{ color: '#e6b800' }}>⏸ {t(locale, 'aiWorkbench.dashboard.paused')}</span>}
      </div>
    </div>
  );
}
