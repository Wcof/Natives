'use client';

import { useState, useEffect, useCallback } from 'react';
import { useAsyncData } from '@/hooks/useAsyncData';
import { type AgentSession } from '@/types/agent';
import { t, type Locale } from '@/i18n';
import { RefreshCw } from 'lucide-react';
import { EmptyState } from '@/components/ui/EmptyState';
import { SPACING, FONT_SIZE, BORDER_RADIUS, TRANSITION } from '@/lib/design-tokens';

export default function ProjectMemory() {
  const [selected, setSelected] = useState<string | null>(null);
  const [locale, setLocale] = useState<Locale>('zh');
  const { data: sessions, loading, error, reload: loadSessions } = useAsyncData(async () => {
    const api = window.nativesAPI;
    if (api?.agent?.scanProjects && api?.agent?.getSessions) {
      const projects = await api.agent.scanProjects() as Array<{ path: string; name: string }>;
      if (Array.isArray(projects) && projects.length > 0) {
        const result = await api.agent.getSessions(projects[0]!.path);
        if (Array.isArray(result)) return result as AgentSession[];
      }
    }
    return [];
  }, []);

  useEffect(() => {
    async function loadLocale() {
      try {
        const saved = await window.nativesAPI?.getLocale?.();
        if (saved) setLocale(saved === 'en' ? 'en' : 'zh');
      } catch { /* ignore */ }
    }
    loadLocale();
  }, []);

  const handleRestore = useCallback(async (session: AgentSession) => {
    const api = window.nativesAPI;
    if (!api?.terminal?.write) return;

    // Show terminal panel
    window.dispatchEvent(new CustomEvent('toggle-terminal'));

    try {
      // Create a new terminal session (or get active one)
      const result = await api.terminal.create() as { sessionId?: string; error?: string };
      const sessionId = result?.sessionId;
      if (!sessionId) {
        console.error('[ProjectMemory] Failed to create terminal session:', result?.error);
        return;
      }
      // Write the claude --resume command to restore the session
      await api.terminal.write(sessionId, `claude --resume ${session.id}\n`);
    } catch (err) {
      console.error('[ProjectMemory] Failed to resume session via terminal:', err);
    }
  }, []);

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
      {/* Header */}
      <div style={{
        padding: '8px 10px', borderBottom: '1px solid var(--vibe-btn-border)',
        display: 'flex', alignItems: 'center', justifyContent: 'space-between',
      }}>
        <div style={{ fontSize: FONT_SIZE.sm, fontWeight: 600, color: 'var(--text-dim)', textTransform: 'uppercase', letterSpacing: 0.5 }}>
          {t(locale, 'aiWorkbench.projectMemory')} ({(sessions ?? []).length})
        </div>
        <button className="btn-ghost" onClick={loadSessions} style={{ fontSize: FONT_SIZE.xs, padding: '3px 6px' }}>
          <RefreshCw size={14} />
        </button>
      </div>

      {/* Sessions list */}
      <div style={{ flex: 1, overflow: 'auto', padding: 6 }}>
        {loading ? (
          <div style={{ padding: SPACING.xl, textAlign: 'center', color: 'var(--text-faint)', fontSize: 'var(--fs-sm)' }}>
            {t(locale, 'common.loading')}
          </div>
        ) : (sessions ?? []).length === 0 ? (
          <EmptyState title={t(locale, 'aiWorkbench.projectMemoryEmpty')} />
        ) : (
          (sessions ?? []).map((s) => (
            <div
              key={s.id}
              onClick={() => setSelected(selected === s.id ? null : s.id)}
              style={{
                padding: '8px 10px', marginBottom: SPACING.xs,
                borderRadius: BORDER_RADIUS.md, cursor: 'pointer',
                background: selected === s.id ? 'var(--vibe-btn-bg)' : 'var(--vibe-toolbar-bg)',
                border: `1px solid ${selected === s.id ? 'var(--accent)' : 'var(--vibe-btn-border)'}`,
                transition: 'all 0.12s',
              }}
            >
              <div style={{ fontSize: 'var(--fs-sm)', color: 'var(--text)', fontWeight: 600, marginBottom: 2 }}>
                {s.title}
              </div>
              <div style={{ fontSize: FONT_SIZE.xs, color: 'var(--text-faint)' }}>
                {s.engine} · {s.filesModified.length} {t(locale, 'aiWorkbench.files')} · {new Date(s.startTime).toLocaleDateString()}
              </div>
              {selected === s.id && (
                <div style={{ marginTop: 8, paddingTop: 6, borderTop: '1px solid var(--vibe-btn-border)' }}>
                  {s.filesModified.length > 0 && (
                    <div style={{ fontSize: FONT_SIZE.sm, color: 'var(--text-dim)', marginBottom: 6 }}>
                      <span style={{ fontWeight: 600 }}>{t(locale, 'aiWorkbench.files')}:</span>{' '}
                      {s.filesModified.slice(0, 5).map((f, i) => (
                        <span key={f}>
                          {i > 0 && ', '}
                          <span style={{ fontFamily: 'var(--font-mono)', color: 'var(--accent)' }}>
                            {f.split('/').pop()}
                          </span>
                        </span>
                      ))}
                      {s.filesModified.length > 5 && ` +${s.filesModified.length - 5}`}
                    </div>
                  )}
                  {s.skillsUsed.length > 0 && (
                    <div style={{ fontSize: FONT_SIZE.sm, color: 'var(--text-dim)', marginBottom: 6 }}>
                      <span style={{ fontWeight: 600 }}>{t(locale, 'aiWorkbench.skillsLabel')}:</span>{' '}
                      {s.skillsUsed.join(', ')}
                    </div>
                  )}
                  <button
                    className="btn btn-primary"
                    style={{ fontSize: FONT_SIZE.sm, padding: '4px 10px' }}
                    onClick={(e) => { e.stopPropagation(); handleRestore(s); }}
                  >
                    {t(locale, 'aiWorkbench.restoreSession')}
                  </button>
                </div>
              )}
            </div>
          ))
        )}
      </div>
    </div>
  );
}
