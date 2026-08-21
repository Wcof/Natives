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
    <div className="flex h-full min-h-0 flex-col bg-[var(--surface-subtle)]">
      <div className="flex shrink-0 items-center gap-1 border-b border-[var(--border)] px-3">
        {tabs.map((item) => {
          const isActive = tab === item.id;
          return (
            <button
              key={item.id}
              type="button"
              onClick={() => setTab(item.id)}
              aria-selected={isActive}
              className={`inline-flex items-center gap-2 rounded-t-lg border-b-2 px-3 py-2.5 text-xs font-medium transition-colors ${
                isActive
                  ? 'border-[var(--primary)] bg-[var(--surface)] text-[var(--text)]'
                  : 'border-transparent text-[var(--text-secondary)] hover:text-[var(--text)]'
              }`}
            >
              {item.icon}
              <span>{t(locale, item.labelKey)}</span>
            </button>
          );
        })}
      </div>
      <div className="min-h-0 flex-1 overflow-auto p-4">
        {tab === 'resources' && <AiResourcesPanel />}
        {tab === 'proxy' && <LocalProxyPanel />}
        {tab === 'tools' && <AiToolIntegrationsPanel />}
        {tab === 'inbox' && <ChangeInbox />}
        {tab === 'organizer' && <AIFileOrganizer />}
      </div>
    </div>
  );
}
