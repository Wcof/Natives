'use client';

import { useState } from 'react';
import { t as tr, useLocale } from '@/i18n';
import AgentDashboard from './AgentDashboard';
import AgentSessions from './AgentSessions';
import SkillsPanel from './SkillsPanel';
import UsagePanel from './UsagePanel';
import RtkPanel from './RtkPanel';
import ChangeInbox from './ChangeInbox';
import AIFileOrganizer from './AIFileOrganizer';

// 原 'memory'（ProjectMemory）与 'sessions'（SessionReplay）读同一数据源且均为假面板，
// 已合并为真实数据驱动的 AgentSessions；'files' 页签中冗余的 FollowModeUI（终端跟随开关
// 已在终端工具栏中有真实实现）一并移除。
type AiTab = 'agents' | 'sessions' | 'skills' | 'usage' | 'inbox' | 'files';

export default function AiWorkbench() {
  const [tab, setTab] = useState<AiTab>('agents');
  const locale = useLocale();

  const tabs: { id: AiTab; label: string }[] = [
    { id: 'agents', label: tr(locale, 'aiWorkbench.tabs.agents') },
    { id: 'sessions', label: tr(locale, 'aiWorkbench.tabs.sessions') },
    { id: 'skills', label: tr(locale, 'aiWorkbench.tabs.skills') },
    { id: 'usage', label: tr(locale, 'aiWorkbench.tabs.usage') },
    { id: 'inbox', label: tr(locale, 'aiWorkbench.tabs.inbox') },
    { id: 'files', label: tr(locale, 'aiWorkbench.tabs.files') },
  ];

  return (
    <div style={{ height: '100%', display: 'flex', flexDirection: 'column' }}>
      <div style={{ display: 'flex', gap: 0, borderBottom: '1px solid var(--border)', padding: '0 12px' }}>
        {tabs.map((item) => (
          <button
            key={item.id}
            type="button"
            onClick={() => setTab(item.id)}
            style={{
              padding: '10px 14px',
              fontSize: 'var(--fs-sm)',
              fontWeight: 500,
              background: 'none',
              border: 'none',
              borderBottom: tab === item.id ? '2px solid var(--primary)' : '2px solid transparent',
              color: tab === item.id ? 'var(--text)' : 'var(--text-secondary)',
              cursor: 'pointer',
            }}
          >
            {item.label}
          </button>
        ))}
      </div>
      <div style={{ flex: 1, overflow: 'auto', padding: 'var(--space-md)' }}>
        {tab === 'agents' && <AgentDashboard />}
        {tab === 'sessions' && <AgentSessions />}
        {tab === 'skills' && <SkillsPanel />}
        {tab === 'usage' && <><UsagePanel /><RtkPanel /></>}
        {tab === 'inbox' && <ChangeInbox />}
        {tab === 'files' && <AIFileOrganizer />}
      </div>
    </div>
  );
}
