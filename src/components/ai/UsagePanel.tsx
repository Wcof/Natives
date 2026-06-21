'use client';

import { useState, useEffect, useMemo } from 'react';
import { useAsyncData } from '@/hooks/useAsyncData';
import { t, type Locale } from '@/i18n';
import { ProgressBar } from '@/components/ui/ProgressBar';
import { SPACING, FONT_SIZE, BORDER_RADIUS, TRANSITION } from '@/lib/design-tokens';
import { TokenHero } from '@/components/dashboard/TokenHero';
import { TokenTrendChart } from '@/components/dashboard/TokenTrendChart';
import { ModelStatsTable } from '@/components/dashboard/ModelStatsTable';
import { CircleAlert, FileText, Database, RefreshCw, DollarSign } from 'lucide-react';
import { MathCurveLoader } from '@/components/ui/MathCurveLoader';
import type { UsageResponse, ClaudeUsage, CodexUsage, RtkUsage } from '@/types/agent';

export default function UsagePanel() {
  const [activeTab, setActiveTab] = useState<'claude' | 'codex' | 'rtk'>('claude');
  const [locale, setLocale] = useState<Locale>('zh');

  const { data: usageData, loading, error, reload: fetchUsage } = useAsyncData(async () => {
    const api = window.nativesAPI;
    if (!api?.usage?.refresh) return null;
    return await api.usage.refresh() as unknown as UsageResponse | null;
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

  // Model stats for the table — merged from all sources
  const modelStats = useMemo(() => usageData?.modelStats ?? [], [usageData]);

  // History for trend chart — prefer Claude history
  const historyForChart = useMemo(() => usageData?.history ?? [], [usageData]);

  const sourceBreadcrumbs = usageData?.sourceBreadcrumbs ?? [];

  const tabs = [
    { id: 'claude' as const, label: 'Claude Code' },
    { id: 'codex' as const, label: 'Codex' },
    { id: 'rtk' as const, label: 'RTK' },
  ];

  return (
    <div style={{ padding: SPACING.sm }}>
      {/* Tab bar */}
      <div style={{
        display: 'flex', gap: SPACING.xs, marginBottom: SPACING.sm,
        borderBottom: '1px solid var(--border)',
      }}>
        {tabs.map((tab) => (
          <button
            key={tab.id}
            className="btn btn-ghost"
            onClick={() => setActiveTab(tab.id)}
            style={{
              fontSize: FONT_SIZE.sm, padding: '4px 10px', flex: 1, borderRadius: 0,
              borderBottom: activeTab === tab.id ? '2px solid var(--accent)' : '2px solid transparent',
              color: activeTab === tab.id ? 'var(--accent)' : 'var(--text-dim)',
              transition: TRANSITION.normal,
            }}
          >
            {tab.label}
          </button>
        ))}
      </div>

      {/* Loading state */}
      {loading && (
        <div style={{ display: 'flex', justifyContent: 'center', padding: SPACING.lg }}>
          <MathCurveLoader size={32} />
        </div>
      )}

      {/* Error state */}
      {error && !loading && (
        <div style={{
          display: 'flex', alignItems: 'center', gap: SPACING.sm,
          fontSize: FONT_SIZE.sm, color: 'var(--vibe-error-color, #e74c3c)',
          padding: SPACING.sm, marginBottom: SPACING.sm,
        }}>
          <CircleAlert size={14} />
          <span>{String(error)}</span>
        </div>
      )}

      {/* Source breadcrumbs */}
      {sourceBreadcrumbs.length > 0 && (
        <div style={{
          display: 'flex', flexDirection: 'column', gap: 2,
          marginBottom: SPACING.sm, fontSize: FONT_SIZE.xs,
          color: 'var(--text-faint)',
        }}>
          {sourceBreadcrumbs.map((path, i) => (
            <div key={i} style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
              <FileText size={10} />
              <span style={{ fontFamily: 'var(--font-mono)', opacity: 0.7 }}>{path}</span>
            </div>
          ))}
        </div>
      )}

      {/* ── Claude Tab ── */}
      {activeTab === 'claude' && (
        <div>
          {usageData?.claude ? (
            <>
              {/* TokenHero 头部统计 */}
              <TokenHero
                usage={usageData.claude as ClaudeUsage}
                isLoading={loading}
                sourceConfigured={usageData.sourceConfigured}
                minimal
              />

              {/* 成本展示 */}
              {usageData.claude.totalCost != null && usageData.claude.totalCost > 0 && (
                <div style={{
                  display: 'flex', alignItems: 'center', gap: SPACING.sm,
                  fontSize: FONT_SIZE.sm, color: 'var(--text-dim)',
                  marginBottom: SPACING.sm, padding: `${SPACING.xs}px 0`,
                }}>
                  <DollarSign size={14} />
                  <span>{t(locale, 'dashboard.totalCost')}: <strong>${usageData.claude.totalCost.toFixed(2)}</strong></span>
                </div>
              )}

              {/* 活跃统计 */}
              <div style={{
                display: 'flex', gap: SPACING.md, fontSize: FONT_SIZE.xs,
                color: 'var(--text-faint)', marginBottom: SPACING.sm,
              }}>
                <span>{usageData.claude.activity?.totalSessions ?? 0} {t(locale, 'aiWorkbench.sessions')}</span>
                <span>{usageData.claude.activity?.totalMessages ?? 0} {t(locale, 'aiWorkbench.messages')}</span>
                {usageData.claude.activity?.firstSessionDate && (
                  <span>{t(locale, 'aiWorkbench.since')}: {usageData.claude.activity.firstSessionDate}</span>
                )}
              </div>

              {/* 趋势图 */}
              {historyForChart.length > 0 && (
                <div style={{ marginBottom: SPACING.md }}>
                  <TokenTrendChart
                    usageHistory={historyForChart.map(h => ({
                      date: new Date(h.timestamp).toISOString().split('T')[0] ?? '',
                      input: h.inputTokens,
                      output: h.outputTokens,
                      cacheWrite: h.cacheCreationTokens,
                      cacheRead: h.cacheReadTokens,
                      total: h.inputTokens + h.outputTokens + h.cacheCreationTokens + h.cacheReadTokens,
                      skills: h.skills ?? 0,
                    }))}
                    isLoading={loading}
                    minimal
                  />
                </div>
              )}

              {/* 模型统计表 */}
              {modelStats.length > 0 && (
                <div style={{ marginBottom: SPACING.md }}>
                  <ModelStatsTable modelStats={modelStats} isLoading={loading} minimal />
                </div>
              )}

              {/* 模型 token 明细 */}
              <div style={{ fontSize: FONT_SIZE.sm, fontWeight: 600, color: 'var(--text)', marginBottom: 6 }}>
                {t(locale, 'aiWorkbench.modelBreakdown')}
              </div>
              {Object.entries(usageData.claude.models ?? {}).map(([modelId, modelUsage]) => (
                <div key={modelId} style={{ marginBottom: SPACING.xs }}>
                  <ProgressBar
                    label={modelId}
                    used={(modelUsage.inputTokens ?? 0) + (modelUsage.outputTokens ?? 0)}
                    limit={((modelUsage.inputTokens ?? 0) + (modelUsage.outputTokens ?? 0) + (modelUsage.cacheReadInputTokens ?? 0)) || 1}
                    color="var(--vibe-active-color)"
                  />
                </div>
              ))}
            </>
          ) : (
            <div style={{
              display: 'flex', flexDirection: 'column', alignItems: 'center',
              gap: SPACING.sm, padding: SPACING.lg,
              fontSize: FONT_SIZE.sm, color: 'var(--text-faint)',
            }}>
              <Database size={24} />
              {usageData?.error || t(locale, 'aiWorkbench.usage.noClaudeData')}
              <span style={{ fontSize: FONT_SIZE.xs, opacity: 0.6 }}>
                {t(locale, 'dashboard.sourceBreadcrumb')}: {'~/.claude/stats-cache.json'}
              </span>
            </div>
          )}

          {/* 刷新按钮 */}
          <button
            className="btn btn-ghost"
            onClick={fetchUsage}
            disabled={loading}
            style={{
              marginTop: 8, fontSize: FONT_SIZE.sm,
              display: 'flex', alignItems: 'center', gap: SPACING.xs,
              color: 'var(--text-dim)',
            }}
          >
            <RefreshCw size={12} style={{ animation: loading ? 'spin 0.8s linear infinite' : undefined }} />
            {loading ? '...' : t(locale, 'aiWorkbench.refresh')}
          </button>
        </div>
      )}

      {/* ── Codex Tab ── */}
      {activeTab === 'codex' && (
        <div>
          {usageData?.codex ? (
            <>
              {/* 总览 */}
              <div style={{ display: 'flex', gap: SPACING.sm, marginBottom: SPACING.md, flexWrap: 'wrap' }}>
                <div style={{ fontSize: FONT_SIZE.sm, color: 'var(--text-dim)' }}>
                  {t(locale, 'dashboard.today')}: <span style={{ color: 'var(--text)', fontWeight: 600 }}>{usageData.codex.todayTokens}</span>
                </div>
                <div style={{ fontSize: FONT_SIZE.sm, color: 'var(--text-dim)' }}>
                  {t(locale, 'dashboard.last7Days')}: <span style={{ color: 'var(--text)', fontWeight: 600 }}>{usageData.codex.weekTokens}</span>
                </div>
                <div style={{ fontSize: FONT_SIZE.sm, color: 'var(--text-dim)' }}>
                  {t(locale, 'dashboard.total')}: <span style={{ color: 'var(--text)', fontWeight: 600 }}>{usageData.codex.totalTokens}</span>
                </div>
                <div style={{ fontSize: FONT_SIZE.sm, color: 'var(--text-dim)' }}>
                  {t(locale, 'dashboard.requests')}: <span style={{ color: 'var(--text)', fontWeight: 600 }}>{usageData.codex.totalSessions}</span>
                </div>
              </div>

              {/* 成本估算 */}
              {usageData.codex.totalCost > 0 && (
                <div style={{
                  display: 'flex', alignItems: 'center', gap: SPACING.sm,
                  fontSize: FONT_SIZE.sm, color: 'var(--text-dim)',
                  marginBottom: SPACING.sm,
                }}>
                  <DollarSign size={14} />
                  <span>{t(locale, 'dashboard.totalCost')}: <strong>${usageData.codex.totalCost.toFixed(2)}</strong> (est.)</span>
                </div>
              )}

              {/* 趋势图 */}
              {(usageData.codex.history?.length ?? 0) > 0 && (
                <div style={{ marginBottom: SPACING.md }}>
                  <TokenTrendChart
                    usageHistory={(usageData.codex.history ?? []).map(h => ({
                      date: new Date(h.timestamp).toISOString().split('T')[0] ?? '',
                      input: h.inputTokens,
                      output: h.outputTokens,
                      cacheWrite: h.cacheCreationTokens,
                      cacheRead: h.cacheReadTokens,
                      total: h.inputTokens + h.outputTokens + h.cacheCreationTokens + h.cacheReadTokens,
                      skills: h.skills ?? 0,
                    }))}
                    isLoading={loading}
                    minimal
                  />
                </div>
              )}
            </>
          ) : (
            <div style={{
              display: 'flex', flexDirection: 'column', alignItems: 'center',
              gap: SPACING.sm, padding: SPACING.lg,
              fontSize: FONT_SIZE.sm, color: 'var(--text-faint)',
            }}>
              <Database size={24} />
              {t(locale, 'aiWorkbench.usage.noCodexData')}
              <span style={{ fontSize: FONT_SIZE.xs, opacity: 0.6 }}>
                {t(locale, 'dashboard.sourceBreadcrumb')}: {'~/.codex/account.json'}
              </span>
            </div>
          )}

          <button
            className="btn btn-ghost"
            onClick={fetchUsage}
            disabled={loading}
            style={{
              marginTop: 8, fontSize: FONT_SIZE.sm,
              display: 'flex', alignItems: 'center', gap: SPACING.xs,
              color: 'var(--text-dim)',
            }}
          >
            <RefreshCw size={12} style={{ animation: loading ? 'spin 0.8s linear infinite' : undefined }} />
            {loading ? '...' : t(locale, 'aiWorkbench.refresh')}
          </button>
        </div>
      )}

      {/* ── RTK Tab ── */}
      {activeTab === 'rtk' && (
        <div>
          {usageData?.rtk && usageData.rtk.totalCommands > 0 ? (
            <>
              <div style={{ display: 'flex', gap: SPACING.md, marginBottom: SPACING.md }}>
                <div style={{ fontSize: FONT_SIZE.sm, color: 'var(--text-dim)' }}>
                  {t(locale, 'aiWorkbench.tokensSaved')}: <span style={{ color: 'var(--text)', fontWeight: 600 }}>{usageData.rtk.totalSaved}</span>
                </div>
                <div style={{ fontSize: FONT_SIZE.sm, color: 'var(--text-dim)' }}>
                  {t(locale, 'aiWorkbench.commands')}: <span style={{ color: 'var(--text)', fontWeight: 600 }}>{usageData.rtk.totalCommands}</span>
                </div>
              </div>

              {/* Top commands */}
              {(usageData.rtk.topCommands?.length ?? 0) > 0 && (
                <div>
                  <div style={{ fontSize: FONT_SIZE.sm, fontWeight: 600, color: 'var(--text)', marginBottom: 6 }}>
                    {t(locale, 'aiWorkbench.rtk.topCommands')}
                  </div>
                  <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
                    {usageData.rtk.topCommands.slice(0, 10).map((cmd, idx) => (
                      <div
                        key={idx}
                        style={{
                          display: 'flex', justifyContent: 'space-between',
                          fontSize: FONT_SIZE.xs, padding: '2px 0',
                          borderBottom: '0.5px solid var(--border)',
                          fontFamily: 'var(--font-mono)',
                        }}
                      >
                        <span style={{ color: 'var(--text-dim)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', flex: 1 }}>
                          {cmd.command}
                        </span>
                        <span style={{ color: 'var(--text-faint)', marginLeft: SPACING.sm }}>
                          {cmd.count}x / {cmd.totalSaved}{' '}
                          <span style={{ fontSize: FONT_SIZE.xs }}>
                            {t(locale, 'aiWorkbench.tokensSaved')?.toLowerCase()}
                          </span>
                        </span>
                      </div>
                    ))}
                  </div>
                </div>
              )}
            </>
          ) : (
            <div style={{
              display: 'flex', flexDirection: 'column', alignItems: 'center',
              gap: SPACING.sm, padding: SPACING.lg,
              fontSize: FONT_SIZE.sm, color: 'var(--text-faint)',
            }}>
              <Database size={24} />
              {t(locale, 'aiWorkbench.usage.noRtkData')}
              <span style={{ fontSize: FONT_SIZE.xs, opacity: 0.6 }}>
                {t(locale, 'dashboard.sourceBreadcrumb')}: {'~/.natives/rtk-history.json'}
              </span>
            </div>
          )}

          <button
            className="btn btn-ghost"
            onClick={fetchUsage}
            disabled={loading}
            style={{
              marginTop: 8, fontSize: FONT_SIZE.sm,
              display: 'flex', alignItems: 'center', gap: SPACING.xs,
              color: 'var(--text-dim)',
            }}
          >
            <RefreshCw size={12} style={{ animation: loading ? 'spin 0.8s linear infinite' : undefined }} />
            {loading ? '...' : t(locale, 'aiWorkbench.refresh')}
          </button>
        </div>
      )}
    </div>
  );
}
