'use client';

import { Route, Store } from 'lucide-react';
import { BORDER_RADIUS, FONT_SIZE, SPACING } from '@/lib/design-tokens';
import { t, type Locale } from '@/i18n';

export type ProviderSettingsTab = 'management' | 'routing';

export function ProviderSettingsTabs({ locale, activeTab, onChange }: {
  locale: Locale;
  activeTab: ProviderSettingsTab;
  onChange: (tab: ProviderSettingsTab) => void;
}) {
  const tabs: Array<{ id: ProviderSettingsTab; label: string; icon: React.ReactNode }> = [
    { id: 'management', label: t(locale, 'settings.providerManagementTab'), icon: <Store size={15} /> },
    { id: 'routing', label: t(locale, 'settings.providerRoutingTab'), icon: <Route size={15} /> },
  ];
  return (
    <div role="tablist" aria-label={t(locale, 'settings.providerTabsLabel')} style={tabListStyle}>
      {tabs.map((tab) => {
        const selected = tab.id === activeTab;
        return (
          <button key={tab.id} type="button" role="tab" aria-selected={selected} onClick={() => onChange(tab.id)} style={{ ...tabStyle, ...(selected ? activeTabStyle : {}) }}>
            {tab.icon}{tab.label}
          </button>
        );
      })}
    </div>
  );
}

const tabListStyle: React.CSSProperties = { display: 'inline-flex', gap: SPACING.xs, marginBottom: SPACING.lg, padding: SPACING.xs, border: '1px solid var(--border)', borderRadius: BORDER_RADIUS.md, background: 'var(--surface-hover)' };
const tabStyle: React.CSSProperties = { display: 'inline-flex', alignItems: 'center', gap: 6, minHeight: 32, padding: `0 ${SPACING.md}px`, border: '1px solid transparent', borderRadius: BORDER_RADIUS.sm, background: 'transparent', color: 'var(--text-secondary)', cursor: 'pointer', fontSize: FONT_SIZE.xs, fontWeight: 600 };
const activeTabStyle: React.CSSProperties = { borderColor: 'var(--border)', background: 'var(--surface)', color: 'var(--text)' };
