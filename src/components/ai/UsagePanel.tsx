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
          {usageData?.claude ? (
            <>
              {/* 本地 token 统计 */}
              <div style={{ display: 'flex', gap: 8, marginBottom: 12 }}>
                <div style={{ fontSize: 11, color: 'var(--text-dim)' }}>
                  {t(locale, 'aiWorkbench.today')}: <span style={{ color: 'var(--text)', fontWeight: 600 }}>{usageData.claude.localTokens?.today ?? 0}</span>
                </div>
                <div style={{ fontSize: 11, color: 'var(--text-dim)' }}>
                  {t(locale, 'aiWorkbench.thisWeek')}: <span style={{ color: 'var(--text)', fontWeight: 600 }}>{usageData.claude.localTokens?.thisWeek ?? 0}</span>
                </div>
                <div style={{ fontSize: 11, color: 'var(--text-dim)' }}>
                  {t(locale, 'aiWorkbench.total')}: <span style={{ color: 'var(--text)', fontWeight: 600 }}>{usageData.claude.localTokens?.total ?? 0}</span>
                </div>
              </div>
              {/* 活跃统计 */}
              <div style={{ display: 'flex', gap: 12, fontSize: 10, color: 'var(--text-faint)', marginBottom: 12 }}>
                <span>{usageData.claude.activity?.totalSessions ?? 0} {t(locale, 'aiWorkbench.sessions')}</span>
                <span>{usageData.claude.activity?.totalMessages ?? 0} {t(locale, 'aiWorkbench.messages')}</span>
                {usageData.claude.activity?.firstSessionDate && (
                  <span>{t(locale, 'aiWorkbench.since')}: {usageData.claude.activity.firstSessionDate}</span>
                )}
              </div>
              {/* 模型 token 明细 */}
              <div style={{ fontSize: 11, fontWeight: 600, color: 'var(--text)', marginBottom: 6 }}>
                {t(locale, 'aiWorkbench.modelBreakdown')}
              </div>
              {Object.entries(usageData.claude.models ?? {}).map(([modelId, usage]) => (
                <div key={modelId} style={{ marginBottom: 4 }}>
                  <ProgressBar
                    label={modelId}
                    used={(usage.inputTokens ?? 0) + (usage.outputTokens ?? 0)}
                    limit={((usage.inputTokens ?? 0) + (usage.outputTokens ?? 0) + (usage.cacheReadInputTokens ?? 0)) || 1}
                    color="var(--vibe-active-color)"
                  />
                </div>
              ))}
            </>
          ) : (
            <div style={{ fontSize: 11, color: 'var(--text-faint)' }}>
              {usageData?.error || t(locale, 'aiWorkbench.usage.noClaudeData')}
            </div>
          )}
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
          <div style={{ fontSize: 11, color: 'var(--text-faint)' }}>
            {t(locale, 'aiWorkbench.usage.noCodexData')}
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
