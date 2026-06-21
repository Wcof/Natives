'use client';

import { useState, useEffect } from 'react';
import { SPACING, FONT_SIZE } from '@/lib/design-tokens';
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
    <div style={{ height: '100%', display: 'flex', flexDirection: 'column', padding: SPACING.lg }}>
      <div style={{ display: 'flex', gap: 0, borderBottom: '0.0625rem solid var(--vibe-toolbar-border)', marginBottom: SPACING.lg }}>
        {tabs.map((t) => (
          <button
            key={t.id}
            onClick={() => setTab(t.id)}
            style={{
              padding: `${SPACING.sm}px ${SPACING.md}px`,
              fontSize: FONT_SIZE.md,
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

      {tab === 'screenshot' && <ScreenshotCard />}
      {tab === 'release' && <ReleaseWizardDialog />}
      {tab === 'update' && <UpdateNotification locale={locale} />}
    </div>
  );
}
