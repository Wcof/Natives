'use client';

import { useState, useEffect, useCallback } from 'react';
import { type AgentSession } from '@/types/agent';
import { t, type Locale } from '@/i18n';

export default function ProjectMemory() {
  const [sessions, setSessions] = useState<AgentSession[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
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

  const loadSessions = useCallback(async () => {
    setLoading(true);
    try {
      const api = window.nativesAPI;
      if (api?.agent?.scanProjects && api?.agent?.getSessions) {
        const projects = await api.agent.scanProjects();
        if (Array.isArray(projects) && projects.length > 0) {
          // Get sessions from the first project
          const result = await api.agent.getSessions(projects[0]!);
          if (Array.isArray(result)) {
            setSessions(result as AgentSession[]);
          }
        }
      }
    } catch (err) {
      console.error('[ProjectMemory] Failed to load sessions:', err);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    loadSessions();
  }, [loadSessions]);

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
        padding: '8px 10px', borderBottom: '1px solid var(--border,#262920)',
        display: 'flex', alignItems: 'center', justifyContent: 'space-between',
      }}>
        <div style={{ fontSize: 11, fontWeight: 600, color: 'var(--text-dim)', textTransform: 'uppercase', letterSpacing: 0.5 }}>
          {t(locale, 'aiWorkbench.projectMemory')} ({sessions.length})
        </div>
        <button className="btn-ghost" onClick={loadSessions} style={{ fontSize: 10, padding: '3px 6px' }}>
          ↻
        </button>
      </div>

      {/* Sessions list */}
      <div style={{ flex: 1, overflow: 'auto', padding: 6 }}>
        {loading ? (
          <div style={{ padding: 20, textAlign: 'center', color: 'var(--text-faint)', fontSize: 12 }}>
            {t(locale, 'common.loading')}
          </div>
        ) : sessions.length === 0 ? (
          <div style={{ padding: 20, textAlign: 'center', color: 'var(--text-faint)', fontSize: 12 }}>
            {t(locale, 'aiWorkbench.noSessions')}
          </div>
        ) : (
          sessions.map((s) => (
            <div
              key={s.id}
              onClick={() => setSelected(selected === s.id ? null : s.id)}
              style={{
                padding: '8px 10px', marginBottom: 4,
                borderRadius: 6, cursor: 'pointer',
                background: selected === s.id ? 'var(--bg-3,#1c1e17)' : 'var(--bg-2,#131410)',
                border: `1px solid ${selected === s.id ? 'var(--accent,#cdf24b)' : 'var(--border,#262920)'}`,
                transition: 'all 0.12s',
              }}
            >
              <div style={{ fontSize: 12, color: 'var(--text)', fontWeight: 600, marginBottom: 2 }}>
                {s.title}
              </div>
              <div style={{ fontSize: 10, color: 'var(--text-faint)' }}>
                {s.engine} · {s.filesModified.length} {t(locale, 'aiWorkbench.files')} · {new Date(s.startTime).toLocaleDateString()}
              </div>
              {selected === s.id && (
                <div style={{ marginTop: 8, paddingTop: 6, borderTop: '1px solid var(--border,#262920)' }}>
                  {s.filesModified.length > 0 && (
                    <div style={{ fontSize: 11, color: 'var(--text-dim)', marginBottom: 6 }}>
                      <span style={{ fontWeight: 600 }}>{t(locale, 'aiWorkbench.files')}:</span>{' '}
                      {s.filesModified.slice(0, 5).map((f, i) => (
                        <span key={f}>
                          {i > 0 && ', '}
                          <span style={{ fontFamily: 'var(--font-mono)', color: 'var(--accent,#cdf24b)' }}>
                            {f.split('/').pop()}
                          </span>
                        </span>
                      ))}
                      {s.filesModified.length > 5 && ` +${s.filesModified.length - 5}`}
                    </div>
                  )}
                  {s.skillsUsed.length > 0 && (
                    <div style={{ fontSize: 11, color: 'var(--text-dim)', marginBottom: 6 }}>
                      <span style={{ fontWeight: 600 }}>{t(locale, 'aiWorkbench.skillsLabel')}:</span>{' '}
                      {s.skillsUsed.join(', ')}
                    </div>
                  )}
                  <button
                    className="btn btn-primary"
                    style={{ fontSize: 11, padding: '4px 10px' }}
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
