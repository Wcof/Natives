'use client';

import { useState, useEffect, useCallback } from 'react';
import { type SkillInfo } from '@/types/agent';
import { t, type Locale } from '@/i18n';

export default function SkillsPanel() {
  const [skills, setSkills] = useState<SkillInfo[]>([]);
  const [filter, setFilter] = useState<'all' | 'healthy' | 'issues'>('all');
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

  const loadSkills = useCallback(async () => {
    setLoading(true);
    try {
      const api = window.nativesAPI;
      if (api?.agent?.scanSkills) {
        const result = await api.agent.scanSkills();
        if (Array.isArray(result)) {
          setSkills(result as SkillInfo[]);
        }
      }
    } catch (err) {
      console.error('[SkillsPanel] Failed to load skills:', err);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    loadSkills();
  }, [loadSkills]);

  const filtered = filter === 'all' ? skills : skills.filter((s) => filter === 'healthy' ? s.health.ok : !s.health.ok);

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
      {/* Filter tabs */}
      <div style={{
        display: 'flex', gap: 4, padding: '8px 10px',
        borderBottom: '1px solid var(--border,#262920)',
      }}>
        {(['all', 'healthy', 'issues'] as const).map((f) => (
          <button
            key={f}
            className="btn-ghost"
            onClick={() => setFilter(f)}
            style={{
              fontSize: 10, padding: '3px 8px', borderRadius: 4,
              color: filter === f ? 'var(--accent,#cdf24b)' : 'var(--text-faint)',
              background: filter === f ? 'var(--accent-soft,#cdf24b1f)' : 'transparent',
            }}
          >
            {f === 'all' ? t(locale, 'aiWorkbench.skills') : f}
            {f !== 'all' && ` (${skills.filter((s) => f === 'healthy' ? s.health.ok : !s.health.ok).length})`}
          </button>
        ))}
        <div style={{ flex: 1 }} />
        <button className="btn-ghost" onClick={loadSkills} style={{ fontSize: 10, padding: '3px 6px' }}>
          ↻
        </button>
      </div>

      {/* Skills list */}
      <div style={{ flex: 1, overflow: 'auto', padding: 6 }}>
        {loading ? (
          <div style={{ padding: 20, textAlign: 'center', color: 'var(--text-faint)', fontSize: 12 }}>
            {t(locale, 'common.loading')}
          </div>
        ) : filtered.length === 0 ? (
          <div style={{ padding: 20, textAlign: 'center', color: 'var(--text-faint)', fontSize: 12 }}>
            {t(locale, 'aiWorkbench.noSkills')}
          </div>
        ) : (
          filtered.map((skill) => (
            <div key={skill.name} style={{
              padding: '8px 10px', marginBottom: 4,
              borderRadius: 6,
              border: '1px solid var(--border,#262920)',
              background: 'var(--bg-2,#131410)',
            }}>
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 4 }}>
                <span style={{ fontSize: 12, fontWeight: 600, color: 'var(--text)' }}>{skill.name}</span>
                <span style={{
                  width: 8, height: 8, borderRadius: '50%',
                  background: skill.health.ok ? '#4bcdf2' : '#f24b4b',
                }} />
              </div>
              {skill.description && (
                <div style={{ fontSize: 10, color: 'var(--text-faint)', marginBottom: 2 }}>
                  {skill.description.slice(0, 100)}{skill.description.length > 100 ? '...' : ''}
                </div>
              )}
              <div style={{ fontSize: 10, color: 'var(--text-dim)' }}>
                {t(locale, 'aiWorkbench.triggered')} {skill.triggerCount}x · {skill.source}
              </div>
            </div>
          ))
        )}
      </div>
    </div>
  );
}
