'use client';

import { useState, useEffect } from 'react';
import { useAsyncData } from '@/hooks/useAsyncData';
import { t, type Locale } from '@/i18n';
import { ProgressBar } from '@/components/ui/ProgressBar';

export default function UsagePanel() {
  const [activeTab, setActiveTab] = useState<'claude' | 'codex' | 'rtk'>('claude');
  const [locale, setLocale] = useState<Locale>('zh');

  const { data: usageData, loading, error, reload: fetchUsage } = useAsyncData(async () => {
    const api = window.nativesAPI;
    if (!api?.usage?.refresh) return null;
    return await api.usage.refresh();
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

  const tabs = [
    { id: 'claude' as const, label: 'Claude Code' },
    { id: 'codex' as const, label: 'Codex' },
    { id: 'rtk' as const, label: 'RTK' },
  ];

  return (
    <div style={{ padding: 'var(--space-sm)' }}>
      <div style={{ display: 'flex', gap: 4, marginBottom: 8, borderBottom: '1px solid var(--border)' }}>
        {tabs.map((t) => (
          <button
            key={t.id}
            className="btn btn-ghost"
            onClick={() => setActiveTab(t.id)}
            style={{
              fontSize: 11, padding: '4px 10px', flex: 1, borderRadius: 0,
              borderBottom: activeTab === t.id ? '2px solid var(--accent)' : '2px solid transparent',
              color: activeTab === t.id ? 'var(--accent)' : 'var(--text-dim)',
            }}
          >
            {t.label}
          </button>
        ))}
      </div>

      {activeTab === 'claude' && (
        <div>
          <ProgressBar
            label={t(locale, 'aiWorkbench.fiveHourWindow')}
            used={usageData?.claude?.fiveHourWindow?.used ?? 0}
            limit={usageData?.claude?.fiveHourWindow?.limit ?? 50000}
            color="#FFF5E6"
          />
          <ProgressBar
            label={t(locale, 'aiWorkbench.weeklyQuota')}
            used={usageData?.claude?.weeklyQuota?.used ?? 0}
            limit={usageData?.claude?.weeklyQuota?.limit ?? 500000}
            color="#4bcdf2"
          />
          <div style={{ marginTop: 12, fontSize: 11, color: 'var(--text-dim)' }}>
            <div>{t(locale, 'aiWorkbench.localTokens')} 5h: {usageData?.claude?.localTokens?.last5h ?? 0} tokens</div>
            <div>{t(locale, 'aiWorkbench.today')}: {usageData?.claude?.localTokens?.today ?? 0} tokens</div>
            <div>{t(locale, 'aiWorkbench.thisWeek')}: {usageData?.claude?.localTokens?.thisWeek ?? 0} tokens</div>
          </div>
          <button
            className="btn btn-ghost"
            onClick={fetchUsage}
            disabled={loading}
            style={{ marginTop: 8, fontSize: 11 }}
          >
            {loading ? '...' : t(locale, 'aiWorkbench.refresh')}
          </button>
        </div>
      )}

      {activeTab === 'codex' && (
        <div>
          <ProgressBar
            label={t(locale, 'aiWorkbench.fiveHourWindow')}
            used={0}
            limit={30000}
            color="#DEA584"
          />
          <div style={{ marginTop: 8, fontSize: 11, color: 'var(--text-dim)' }}>
            {t(locale, 'aiWorkbench.plan')}: Free
          </div>
        </div>
      )}

      {activeTab === 'rtk' && (
        <div style={{ fontSize: 'var(--fs-sm)', color: 'var(--text-dim)' }}>
          <div style={{ marginBottom: 8 }}>
            <span style={{ color: 'var(--text)', fontWeight: 600 }}>{usageData?.rtk?.totalSaved ?? 0}</span>{' '}
            {t(locale, 'aiWorkbench.tokensSaved')}
          </div>
          <div style={{ marginBottom: 8 }}>
            <span style={{ color: 'var(--text)', fontWeight: 600 }}>{usageData?.rtk?.totalCommands ?? 0}</span>{' '}
            {t(locale, 'aiWorkbench.commands')}
          </div>
          {(!usageData?.rtk || usageData?.rtk?.totalCommands === 0) && (
            <div style={{ fontSize: 10, color: 'var(--text-faint)' }}>
              {t(locale, 'aiWorkbench.usage.noRtkData')}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
