'use client';

import { useState, useEffect } from 'react';
import { type Locale } from '@/i18n';
import { Zap, Grid3x3, HardDrive, Activity, Code2, Cpu } from 'lucide-react';
import type { SkillInfo, ClaudeUsage } from '@/types/agent';

interface UsageData {
  skills: { total: number; enabled: number };
  codexTokens: { used: number; limit: number };
  moduleCount: number;
}

export default function DashboardPage() {
  const [locale, setLocale] = useState<Locale>('zh');
  const [usage, setUsage] = useState<UsageData | null>(null);

  useEffect(() => {
    async function loadLocale() {
      try {
        const saved = await window.nativesAPI?.getLocale?.();
        if (saved) setLocale(saved === 'en' ? 'en' : 'zh');
      } catch { /* ignore */ }
    }
    loadLocale();
  }, []);

  useEffect(() => {
    function handleLocaleChange(e: Event) {
      const detail = (e as CustomEvent).detail;
      if (detail === 'en') setLocale('en');
      else if (detail && detail.startsWith('zh')) setLocale('zh');
    }
    window.addEventListener('locale-changed', handleLocaleChange);
    return () => window.removeEventListener('locale-changed', handleLocaleChange);
  }, []);

  useEffect(() => {
    async function loadUsage() {
      try {
        const api = window.nativesAPI;
        if (!api) return;

        // Skills count
        const skills: SkillInfo[] = await api.agent.scanSkills().catch(() => []);
        const enabled = skills.filter(s => s.enabled).length;

        // Claude usage for tokens
        const usageResult = await api.usage.refresh().catch(() => null);
        const claude: ClaudeUsage | null = usageResult?.claude ?? null;

        // Compute Codex Pro tokens — use Claude's total as a proxy, or 0 if unavailable
        const tokensUsed = claude?.localTokens?.total ?? 0;
        const tokensLimit = Math.max(tokensUsed, 10_000_000); // 10M as default cap

        // Module count — real installed modules
        const modules = await api.module.list().catch(() => []);
        const moduleCount = Array.isArray(modules) ? modules.length : 0;

        setUsage({
          skills: { total: skills.length, enabled },
          codexTokens: { used: tokensUsed, limit: tokensLimit },
          moduleCount,
        });
      } catch { /* fallback — remain null */ }
    }
    loadUsage();
  }, []);

  const t = locale === 'en' ? enDash : zhDash;

  return (
    <div className="w-full h-full flex flex-col overflow-hidden">
      {/* Greeting */}
      <div className="px-8 pt-8 pb-4">
        <h1 className="text-2xl font-bold text-[var(--vibe-brand-text)] tracking-tight">
          {t.greeting}
        </h1>
        <p className="text-sm text-[var(--text-faint)] mt-1">
          {t.subtitle}
        </p>
      </div>

      {/* Stats Cards */}
      <div className="px-8 pb-6 grid grid-cols-1 sm:grid-cols-3 gap-4">
        <StatCard
          icon={<Grid3x3 size={18} />}
          label={t.modulesLabel}
          value={usage ? String(usage.moduleCount) : '—'}
          color="var(--vibe-accent-color)"
        />
        <StatCard
          icon={<HardDrive size={18} />}
          label={t.storageLabel}
          value="—"
          color="var(--vibe-accent-color)"
        />
        <StatCard
          icon={<Activity size={18} />}
          label={t.statusLabel}
          value={t.ready}
          color="var(--vibe-active-color)"
        />
      </div>

      {/* Quick Actions */}
      <div className="px-8 pb-4">
        <h2 className="text-[0.6875rem] font-semibold uppercase tracking-[0.08em] text-[var(--vibe-section-header)] mb-3">
          {t.quickActions}
        </h2>
        <div className="flex flex-wrap gap-3">
          <QuickActionButton
            icon={<Zap size={16} />}
            label={t.openWorkshop}
            onClick={() => window.dispatchEvent(new CustomEvent('navigate', { detail: '__workshop__' }))}
            color="var(--vibe-accent-color)"
          />
          <QuickActionButton
            icon={<Grid3x3 size={16} />}
            label={t.openModules}
            onClick={() => window.dispatchEvent(new CustomEvent('navigate', { detail: 'modules' }))}
            color="var(--vibe-accent-color)"
          />
        </div>
      </div>

      {/* Skills + Codex Pro 额度 */}
      <div className="px-8 pb-4">
        <h2 className="text-[0.6875rem] font-semibold uppercase tracking-[0.08em] text-[var(--text-faint)] mb-3">
          {t.quotaTitle}
        </h2>
        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          {/* Skills 卡 */}
          <div className="rounded-xl border border-[var(--vibe-content-border)] bg-[var(--vibe-content-bg)] px-5 py-4">
            <div className="flex items-center gap-3 mb-3">
              <div className="flex h-8 w-8 items-center justify-center rounded-lg" style={{ backgroundColor: 'var(--vibe-accent-soft)', color: 'var(--vibe-accent-color)' }}>
                <Code2 size={16} />
              </div>
              <span className="text-sm font-semibold text-[var(--vibe-brand-text)]">Skills</span>
            </div>
            {usage ? (
              <>
                <div className="mb-1 flex items-center justify-between text-xs">
                  <span className="text-[var(--text-dim)]">{t.skillsActive}</span>
                  <span className="font-medium text-[var(--vibe-brand-text)]">{usage.skills.enabled}/{usage.skills.total}</span>
                </div>
                <div className="h-1.5 w-full overflow-hidden rounded-full bg-[var(--vibe-btn-bg)]">
                  <div
                    className="h-full rounded-full"
                    style={{ width: `${usage.skills.total > 0 ? Math.round((usage.skills.enabled / usage.skills.total) * 100) : 0}%` }}
                  />
                </div>
              </>
            ) : (
              <p className="text-xs text-[var(--text-faint)]">{t.loading}</p>
            )}
          </div>

          {/* Codex Pro 卡 */}
          <div className="rounded-xl border border-[var(--vibe-content-border)] bg-[var(--vibe-content-bg)] px-5 py-4">
            <div className="flex items-center gap-3 mb-3">
              <div className="flex h-8 w-8 items-center justify-center rounded-lg" style={{ backgroundColor: 'var(--vibe-accent-soft)', color: 'var(--vibe-accent-color)' }}>
                <Cpu size={16} />
              </div>
              <span className="text-sm font-semibold text-[var(--vibe-brand-text)]">Codex Pro</span>
            </div>
            {usage ? (
              <>
                <div className="mb-1 flex items-center justify-between text-xs">
                  <span className="text-[var(--text-dim)]">{t.tokenUsage}</span>
                  <span className="font-medium text-[var(--vibe-brand-text)]">{formatTokens(usage.codexTokens.used)}</span>
                </div>
                <div className="h-1.5 w-full overflow-hidden rounded-full bg-[var(--vibe-btn-bg)]">
                  <div
                    className="h-full rounded-full"
                    style={{ width: `${Math.min(100, Math.round((usage.codexTokens.used / usage.codexTokens.limit) * 100))}%`, background: 'var(--vibe-progress-bg)' }}
                  />
                </div>
                <div className="mt-2 flex justify-end text-[0.625rem] text-[var(--text-faint)]">
                  <span>{formatTokens(usage.codexTokens.limit)} {t.monthlyCap}</span>
                </div>
              </>
            ) : (
              <p className="text-xs text-[var(--text-faint)]">{t.loading}</p>
            )}
          </div>
        </div>
      </div>

      {/* Recent items placeholder (empty state) */}
      <div className="flex-1 px-8 pb-8">
        <h2 className="text-[0.6875rem] font-semibold uppercase tracking-[0.08em] text-[var(--text-faint)] mb-3">
          {t.recentActivity}
        </h2>
        <div className="flex flex-col items-center justify-center h-full text-center">
          <div className="w-12 h-12 rounded-full bg-[var(--vibe-btn-bg)] flex items-center justify-center mb-3">
            <Activity size={20} className="text-[var(--text-faint)]" />
          </div>
          <p className="text-sm text-[var(--text-faint)]">{t.noActivity}</p>
        </div>
      </div>
    </div>
  );
}

function StatCard({
  icon, label, value, color,
}: {
  icon: React.ReactNode;
  label: string;
  value: string;
  color: string;
}) {
  return (
    <div className="rounded-xl border border-[var(--vibe-content-border)] bg-[var(--vibe-content-bg)] px-5 py-4 flex items-center gap-4">
      <div
        className="flex h-10 w-10 items-center justify-center rounded-lg"
        style={{ backgroundColor: `${color}18`, color }}
      >
        {icon}
      </div>
      <div>
        <p className="text-[0.6875rem] text-[var(--text-faint)]">{label}</p>
        <p className="text-lg font-semibold text-[var(--vibe-brand-text)]">{value}</p>
      </div>
    </div>
  );
}

function QuickActionButton({
  icon, label, onClick, color,
}: {
  icon: React.ReactNode;
  label: string;
  onClick: () => void;
  color: string;
}) {
  return (
    <button
      onClick={onClick}
      className="flex items-center gap-2.5 rounded-lg border border-[var(--vibe-content-border)] bg-[var(--vibe-content-bg)] px-4 py-2.5 text-sm text-[var(--vibe-brand-text)] hover:bg-[var(--vibe-btn-hover-bg)] transition-all"
    >
      <span style={{ color }}>{icon}</span>
      <span>{label}</span>
    </button>
  );
}

const enDash = {
  greeting: 'Welcome back',
  subtitle: 'Your personal desktop',
  modulesLabel: 'Modules',
  storageLabel: 'Storage',
  statusLabel: 'Status',
  ready: 'Ready',
  quickActions: 'Quick Actions',
  openWorkshop: 'Open Workshop',
  openModules: 'Manage Modules',
  quotaTitle: 'Usage',
  skillsActive: 'Active',
  tokenUsage: 'Tokens',
  monthlyCap: 'month cap',
  loading: 'Loading...',
  recentActivity: 'Recent Activity',
  noActivity: 'No recent activity yet',
};

const zhDash = {
  greeting: '欢迎回来',
  subtitle: '你的个人桌面',
  modulesLabel: '模块',
  storageLabel: '存储',
  statusLabel: '状态',
  ready: '就绪',
  quickActions: '快捷操作',
  openWorkshop: '打开工作坊',
  openModules: '管理模块',
  quotaTitle: '使用额度',
  skillsActive: '已启用',
  tokenUsage: 'Tokens',
  monthlyCap: '月上限',
  loading: '加载中...',
  recentActivity: '最近动态',
  noActivity: '暂无最近动态',
};

function formatTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return String(n);
}
