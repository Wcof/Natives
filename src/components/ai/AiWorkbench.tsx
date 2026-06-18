'use client';

import { useState, useEffect } from 'react';
import { t, type Locale } from '@/i18n';
import AgentDashboard from './AgentDashboard';
import SessionReplay from './SessionReplay';
import SkillsPanel from './SkillsPanel';
import ProjectMemory from './ProjectMemory';
import UsagePanel from './UsagePanel';
import RtkPanel from './RtkPanel';
import ChangeInbox from './ChangeInbox';
import FollowModeUI from './FollowModeUI';
import AIFileOrganizer from './AIFileOrganizer';

type AiTab = 'agents' | 'sessions' | 'skills' | 'memory' | 'usage' | 'inbox' | 'files';

export default function AiWorkbench() {
  const [tab, setTab] = useState<AiTab>('agents');
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

  const tabs: { id: AiTab; label: string }[] = [
    { id: 'agents', label: t(locale, 'aiWorkbench.tabs.agents') },
    { id: 'sessions', label: t(locale, 'aiWorkbench.tabs.sessions') },
    { id: 'skills', label: t(locale, 'aiWorkbench.tabs.skills') },
    { id: 'memory', label: t(locale, 'aiWorkbench.tabs.memory') },
    { id: 'usage', label: t(locale, 'aiWorkbench.tabs.usage') },
    { id: 'inbox', label: t(locale, 'aiWorkbench.tabs.inbox') },
    { id: 'files', label: t(locale, 'aiWorkbench.tabs.files') },
  ];

  return (
    <div style={{ height: '100%', display: 'flex', flexDirection: 'column' }}>
      <div style={{ display: 'flex', gap: 0, borderBottom: '1px solid var(--vibe-btn-border)', padding: '0 12px' }}>
        {tabs.map((t) => (
          <button
            key={t.id}
            onClick={() => setTab(t.id)}
            style={{
              padding: '10px 14px',
              fontSize: 'var(--fs-sm)',
              fontWeight: 500,
              background: 'none',
              border: 'none',
              borderBottom: tab === t.id ? '2px solid var(--accent)' : '2px solid transparent',
              color: tab === t.id ? 'var(--vibe-brand-text)' : 'var(--vibe-btn-text)',
              cursor: 'pointer',
            }}
          >
            {t.label}
          </button>
        ))}
      </div>
      <div style={{ flex: 1, overflow: 'auto', padding: 'var(--space-md)' }}>
        {tab === 'agents' && <AgentDashboard />}
        {tab === 'sessions' && <SessionReplay />}
        {tab === 'skills' && <SkillsPanel />}
        {tab === 'memory' && <ProjectMemory />}
        {tab === 'usage' && <><UsagePanel /><RtkPanel /></>}
        {tab === 'inbox' && <ChangeInbox />}
        {tab === 'files' && (
          <div>
            <FollowModeUI />
            <AIFileOrganizer />
          </div>
        )}
      </div>
    </div>
  );
}
