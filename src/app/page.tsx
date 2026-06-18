'use client';

import { useState, useEffect } from 'react';
import { SPACING, FONT_SIZE, BORDER_RADIUS } from '@/lib/design-tokens';
import { fmtCount } from '@/lib/format';
import type { SkillInfo, ClaudeUsage } from '@/types/agent';
import { TokenHero } from '@/components/dashboard/TokenHero';
import { TokenTrendChart } from '@/components/dashboard/TokenTrendChart';
import type { TokenHistoryPoint } from '@/components/dashboard/TokenTrendChart';
import { SkillsPanel } from '@/components/dashboard/SkillsPanel';
import { ModelStatsTable, type ModelStat } from '@/components/dashboard/ModelStatsTable';
import { useTranslation } from 'react-i18next';
import { Grid3x3, HardDrive, Activity } from 'lucide-react';

/**
 * DashboardPage — Personal homepage dashboard
 *
 * New layout (removed Quick Actions):
 * ┌─────────────────────────────────────────────────────┐
 * │  Welcome back                                        │
 * │  Your personal desktop                               │
 * ├─────────────────────────────────────────────────────┤
 * │  [Stat Cards] Modules │ Storage │ Status             │
 * ├─────────────────────────────────────────────────────┤
 * │  Token Hero Card                                     │
 * │  Real Tokens │ Requests │ Cost                       │
 * │  [Input] [Output] [Cache Write] [Cache Read] [Rate]  │
 * ├─────────────────────────────────────────────────────┤
 * │  Token Trend Chart (today / 7d / 30d)                │
 * ├─────────────────────────────────────────────────────┤
 * │  Skills Panel                                        │
 * │  Installed / Enabled / Source Distribution           │
 * │  Usage Ranking (hits / recent)                       │
 * ├─────────────────────────────────────────────────────┤
 * │  Model Stats Table                                   │
 * │  Model │ Requests │ Tokens │ Cost │ Avg Cost         │
 * └─────────────────────────────────────────────────────┘
 */

interface UsageData {
  skills: SkillInfo[];
  claude: ClaudeUsage | null;
  moduleCount: number;
  usageHistory: TokenHistoryPoint[];
  modelStats: ModelStat[];
}

export default function DashboardPage() {
  const { t, i18n } = useTranslation();
  const [locale, setLocale] = useState<string>('zh');
  const [usage, setUsage] = useState<UsageData | null>(null);
  const [isLoading, setIsLoading] = useState(true);

  // Locale management
  useEffect(() => {
    async function loadLocale() {
      try {
        const saved = await window.nativesAPI?.getLocale?.();
        if (saved) {
          setLocale(saved === 'en' ? 'en' : 'zh');
          i18n.changeLanguage(saved === 'en' ? 'en' : 'zh');
        }
      } catch { /* ignore */ }
    }
    loadLocale();
  }, [i18n]);

  useEffect(() => {
    function handleLocaleChange(e: Event) {
      const detail = (e as CustomEvent).detail;
      if (detail === 'en') {
        setLocale('en');
        i18n.changeLanguage('en');
      } else if (detail && detail.startsWith('zh')) {
        setLocale('zh');
        i18n.changeLanguage('zh');
      }
    }
    window.addEventListener('locale-changed', handleLocaleChange);
    return () => window.removeEventListener('locale-changed', handleLocaleChange);
  }, [i18n]);

  // Data loading
  useEffect(() => {
    async function loadUsage() {
      setIsLoading(true);
      try {
        const api = window.nativesAPI;
        if (!api) {
          setUsage(null);
          setIsLoading(false);
          return;
        }

        // Skills
        const skills: SkillInfo[] = await api.agent.scanSkills().catch(() => []);

        // Claude usage
        const usageResult = await api.usage.refresh().catch(() => null);
        const claude: ClaudeUsage | null = usageResult?.claude ?? null;

        // Module count
        const modules = await api.module.list().catch(() => []);
        const moduleCount = Array.isArray(modules) ? modules.length : 0;

        // Usage history for trend chart (if API supports it)
        // Note: history() and modelStats() may not be available in all versions
        // The components handle empty data gracefully
        const usageHistoryRaw = await (api.usage as any).history?.(5 * 60 * 60 * 1000).catch(() => []) ?? [];
        const tokenHistory: TokenHistoryPoint[] = Array.isArray(usageHistoryRaw)
          ? usageHistoryRaw.map((point: any) => ({
              date: new Date(point.timestamp ?? Date.now()).toISOString(),
              input: point.inputTokens ?? 0,
              output: point.outputTokens ?? 0,
              cacheWrite: point.cacheCreationTokens ?? 0,
              cacheRead: point.cacheReadTokens ?? 0,
            }))
          : [];

        // Model stats (if API supports it)
        const modelStatsRaw = await (api.usage as any).modelStats?.().catch(() => []) ?? [];
        const modelStats: ModelStat[] = Array.isArray(modelStatsRaw)
          ? modelStatsRaw.map((m: any) => ({
              model: m.model ?? 'unknown',
              requestCount: m.requestCount ?? 0,
              totalTokens: m.totalTokens ?? 0,
              totalCost: m.totalCost ?? 0,
              avgCostPerRequest: m.avgCostPerRequest ?? 0,
            }))
          : [];

        setUsage({
          skills,
          claude,
          moduleCount,
          usageHistory: tokenHistory,
          modelStats,
        });
      } catch {
        setUsage(null);
      } finally {
        setIsLoading(false);
      }
    }
    loadUsage();
  }, []);

  return (
    <div className="w-full h-full flex flex-col overflow-hidden">
      {/* Greeting */}
      <div style={{ padding: `${SPACING.xl}px ${SPACING.xl}px ${SPACING.lg}px`, flexShrink: 0 }}>
        <h1 style={{ fontSize: '28px', fontWeight: 700, color: 'var(--vibe-brand-text)', letterSpacing: '-0.02em' }}>
          {t('dashboard.greeting', 'Welcome back')}
        </h1>
        <p style={{ fontSize: FONT_SIZE.sm, color: 'var(--text-faint)', marginTop: SPACING.xs }}>
          {t('dashboard.subtitle', 'Your personal desktop')}
        </p>
      </div>

      {/* Stats Cards */}
      <div style={{ padding: `0 ${SPACING.xl}px ${SPACING.md}px`, flexShrink: 0 }}>
        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(3, 1fr)', gap: SPACING.md }}>
          <StatCard
            icon={<Grid3x3 size={18} style={{ color: 'var(--vibe-accent-color)' }} />}
            label={t('dashboard.modulesLabel')}
            value={usage ? String(usage.moduleCount) : '—'}
          />
          <StatCard
            icon={<HardDrive size={18} style={{ color: 'var(--vibe-accent-color)' }} />}
            label={t('dashboard.storageLabel')}
            value="—"
          />
          <StatCard
            icon={<Activity size={18} style={{ color: 'var(--vibe-active-color)' }} />}
            label={t('dashboard.statusLabel')}
            value={t('dashboard.ready')}
          />
        </div>
      </div>

      {/* Token Hero Card */}
      <div style={{ padding: `0 ${SPACING.xl}px ${SPACING.md}px`, flexShrink: 0 }}>
        <TokenHero
          usage={usage?.claude}
          isLoading={isLoading}
        />
      </div>

      {/* Token Trend Chart */}
      <div style={{ padding: `0 ${SPACING.xl}px ${SPACING.md}px`, flexShrink: 0 }}>
        <TokenTrendChart
          usageHistory={usage?.usageHistory}
          isLoading={isLoading}
        />
      </div>

      {/* Skills Panel */}
      <div style={{ padding: `0 ${SPACING.xl}px ${SPACING.md}px`, flexShrink: 0 }}>
        <SkillsPanel
          skills={usage?.skills}
          isLoading={isLoading}
        />
      </div>

      {/* Model Stats Table */}
      <div style={{ padding: `0 ${SPACING.xl}px ${SPACING.xl}px`, flex: 1, minHeight: 0 }}>
        <ModelStatsTable
          modelStats={usage?.modelStats}
          isLoading={isLoading}
        />
      </div>
    </div>
  );
}

function StatCard({
  icon,
  label,
  value,
}: {
  icon: React.ReactNode;
  label: string;
  value: string;
}) {
  return (
    <div
      style={{
        borderRadius: BORDER_RADIUS.lg,
        border: '0.0625rem solid var(--vibe-content-border)',
        background: 'var(--vibe-content-bg)',
        backdropFilter: 'blur(var(--vibe-content-blur, 24px))',
        padding: `${SPACING.md}px ${SPACING.lg}px`,
        display: 'flex',
        alignItems: 'center',
        gap: SPACING.md,
      }}
    >
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          width: 40,
          height: 40,
          borderRadius: BORDER_RADIUS.md,
          background: 'var(--vibe-accent-soft)',
        }}
      >
        {icon}
      </div>
      <div>
        <p style={{ fontSize: '11px', color: 'var(--text-faint)' }}>{label}</p>
        <p style={{ fontSize: '20px', fontWeight: 700, color: 'var(--vibe-brand-text)', fontFamily: 'var(--font-mono)' }}>
          {value}
        </p>
      </div>
    </div>
  );
}
