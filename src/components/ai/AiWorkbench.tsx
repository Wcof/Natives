'use client';

import { useState } from 'react';
import { useLocale, t } from '@/i18n';
import AiResourcesPanel from './AiResourcesPanel';
import LocalProxyPanel from './LocalProxyPanel';
import AiToolIntegrationsPanel from './AiToolIntegrationsPanel';
import ChangeInbox from './ChangeInbox';
import AIFileOrganizer from './AIFileOrganizer';
import { Cpu, Server, Wrench, Inbox, FolderTree } from 'lucide-react';

type AiTab = 'resources' | 'proxy' | 'tools' | 'inbox' | 'organizer';

export default function AiWorkbench() {
  const [tab, setTab] = useState<AiTab>('resources');
  const locale = useLocale();

  const tabs: { id: AiTab; labelKey: 'aiWorkbench.tabs.resources' | 'aiWorkbench.tabs.proxy' | 'aiWorkbench.tabs.tools' | 'aiWorkbench.tabs.inbox' | 'aiWorkbench.tabs.organizer'; icon: React.ReactNode }[] = [
    { id: 'resources', labelKey: 'aiWorkbench.tabs.resources', icon: <Cpu className="w-4 h-4" /> },
    { id: 'proxy', labelKey: 'aiWorkbench.tabs.proxy', icon: <Server className="w-4 h-4" /> },
    { id: 'tools', labelKey: 'aiWorkbench.tabs.tools', icon: <Wrench className="w-4 h-4" /> },
    { id: 'inbox', labelKey: 'aiWorkbench.tabs.inbox', icon: <Inbox className="w-4 h-4" /> },
    { id: 'organizer', labelKey: 'aiWorkbench.tabs.organizer', icon: <FolderTree className="w-4 h-4" /> },
  ];

  return (
    <div style={{ height: '100%', display: 'flex', flexDirection: 'column' }}>
      <div style={{ display: 'flex', gap: 4, borderBottom: '1px solid var(--border)', padding: '0 16px' }}>
        {tabs.map((item) => {
          const isActive = tab === item.id;
          return (
            <button
              key={item.id}
              type="button"
              onClick={() => setTab(item.id)}
              className={`flex items-center gap-2 py-3 px-4 text-xs font-medium border-b-2 transition-all ${
                isActive
                  ? 'border-[var(--primary)] text-[var(--text)] font-semibold'
                  : 'border-transparent text-[var(--text-secondary)] hover:text-[var(--text)]'
              }`}
            >
              {item.icon}
              <span>{t(locale, item.labelKey)}</span>
            </button>
          );
        })}
      </div>
      <div style={{ flex: 1, overflow: 'auto', padding: 'var(--space-md)' }}>
        {tab === 'resources' && <AiResourcesPanel />}
        {tab === 'proxy' && <LocalProxyPanel />}
        {tab === 'tools' && <AiToolIntegrationsPanel />}
        {tab === 'inbox' && <ChangeInbox />}
        {tab === 'organizer' && <AIFileOrganizer />}
      </div>
    </div>
  );
}
