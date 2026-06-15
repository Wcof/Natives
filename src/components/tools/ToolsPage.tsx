'use client';

import { useState, useEffect } from 'react';
import { t, type Locale } from '@/i18n';
import ScreenshotCard from './ScreenshotCard';
import ImageAnnotator from './ImageAnnotator';
import ReleaseWizardDialog from './ReleaseWizardDialog';
import UpdateNotification from './UpdateNotification';

type ToolsTab = 'screenshot' | 'release' | 'update';

export default function ToolsPage() {
  const [tab, setTab] = useState<ToolsTab>('screenshot');
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

  const tabs: { id: ToolsTab; label: string }[] = [
    { id: 'screenshot', label: t(locale, 'tools.tabs.screenshot') },
    { id: 'release', label: t(locale, 'tools.tabs.releaseWizard') },
    { id: 'update', label: t(locale, 'tools.tabs.updates') },
  ];

  return (
    <div style={{ height: '100%', display: 'flex', flexDirection: 'column', padding: 16 }}>
      <div style={{ display: 'flex', gap: 0, borderBottom: '1px solid var(--border,#262920)', marginBottom: 16 }}>
        {tabs.map((t) => (
          <button
            key={t.id}
            onClick={() => setTab(t.id)}
            style={{
              padding: '10px 14px',
              fontSize: 12,
              fontWeight: 500,
              background: 'none',
              border: 'none',
              borderBottom: tab === t.id ? '2px solid var(--accent,#cdf24b)' : '2px solid transparent',
              color: tab === t.id ? 'var(--text,#f2f2ea)' : 'var(--text-faint,#62655a)',
              cursor: 'pointer',
            }}
          >
            {t.label}
          </button>
        ))}
      </div>

      {tab === 'screenshot' && <ScreenshotCard />}
      {tab === 'release' && <ReleaseWizardDialog />}
      {tab === 'update' && <UpdateNotification />}
    </div>
  );
}
