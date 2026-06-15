'use client';

import { useState, useEffect, useCallback } from 'react';
import { type FileChangeEvent } from '@/types/agent';
import { t, type Locale } from '@/i18n';

export default function ChangeInbox() {
  const [items, setItems] = useState<(FileChangeEvent & { project?: string })[]>([]);
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
    const api = window.nativesAPI;
    if (!api?.onDbStateChanged) return;
    const unsub = api.onDbStateChanged((_event, channel, data: unknown) => {
      if (channel === 'file:changed' && data && typeof data === 'object' && 'path' in data) {
        const d = data as { path?: string; type?: string; project?: string };
        if (d.path) {
          const eventType = (['create', 'modify', 'delete'].includes(d.type || '') ? d.type : 'modify') as 'create' | 'modify' | 'delete';
          // Extract project name from path (parent of parent directory)
          const parts = d.path.split('/');
          const project = d.project || parts[parts.length - 2] || 'Unknown';
          setItems((prev) => [
            { path: d.path!, type: eventType, timestamp: Date.now(), project },
            ...prev,
          ].slice(0, 200));
        }
      }
    });
    return unsub;
  }, []);

  const handleClear = useCallback(() => setItems([]), []);

  const handleNavigate = useCallback((path: string) => {
    const dir = path.substring(0, path.lastIndexOf('/')) || '/';
    window.dispatchEvent(new CustomEvent('navigate-files', { detail: dir }));
  }, []);

  const groupedByProject = items.reduce<Record<string, typeof items>>((acc, item) => {
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
        {items.length > 0 && (
          <button className="btn-ghost" onClick={handleClear} style={{ fontSize: 10, padding: '2px 6px', color: 'var(--text-faint)' }}>
            {t(locale, 'notifications.clear')}
          </button>
        )}
      </div>

      {/* Changes list */}
      <div style={{ flex: 1, overflow: 'auto', padding: 6 }}>
        {items.length === 0 ? (
          <div style={{ padding: 20, textAlign: 'center', color: 'var(--text-faint)', fontSize: 12 }}>
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
                  className={i === 0 ? 'anim-changedPulse' : ''}
                  style={{
                    padding: '4px 8px', fontSize: 11, color: 'var(--text)',
                    borderRadius: 4, cursor: 'pointer', marginBottom: 2,
                    background: 'var(--bg-2,#131410)',
                    borderLeft: `3px solid ${ch.type === 'create' ? '#4ec9b0' : ch.type === 'delete' ? '#d9534f' : '#e6b800'}`,
                  }}
                >
                  <span className="anim-changedBreath" style={{ marginRight: 4 }}>
                    {ch.type === 'create' ? '🟢' : ch.type === 'delete' ? '🔴' : '🟡'}
                  </span>
                  {ch.path.split('/').pop()}
                </div>
              ))}
            </div>
          ))
        )}
      </div>
    </div>
  );
}
