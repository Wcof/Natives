'use client';

import { useCallback, useEffect, useMemo, useState } from 'react';
import {
  Activity,
  BarChart3,
  Database,
  ExternalLink,
  FolderCode,
  MessageSquare,
  RefreshCw,
  TriangleAlert,
  Users,
  X,
} from 'lucide-react';
import { t, useLocale, type Locale } from '@/i18n';
import { classifyError } from '@/lib/error-classifier';
import { fmtCount } from '@/lib/format';
import { useUsageData } from '@/hooks/useUsageData';
import { useFocusTrap } from '@/lib/useFocusTrap';
import { EmptyState, ErrorState, Skeleton } from '@/components/ui/EmptyState';
// 共享纯计算模块（metrics 子代理所有权；Settings 与 Menubar 共用）。
import { buildOverviewTrend, summarizeOverviewUsage } from '@/lib/personal-overview-data';
import type { UsageViewRequest } from '@/types/usage';
import { invokeMenubar } from './menubar-bridge';
import styles from './MenubarOverview.module.css';

/** 快照超过 24 小时视为「陈旧」——仍显示真实更新时间，不伪装新。 */
const STALE_AFTER_MS = 24 * 60 * 60 * 1000;

function localDateKey(date = new Date()): string {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, '0');
  const day = String(date.getDate()).padStart(2, '0');
  return `${year}-${month}-${day}`;
}

function formatUpdatedAt(value: number, locale: Locale): string {
  return new Intl.DateTimeFormat(locale.startsWith('zh') ? 'zh-CN' : 'en-US', {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  }).format(value);
}

function valueOrUnavailable(value: number | null, locale: Locale): string {
  return value === null ? t(locale, 'menubar.unavailable') : fmtCount(value, locale);
}

function formatAverage(value: number, locale: Locale): string {
  return new Intl.NumberFormat(locale.startsWith('zh') ? 'zh-CN' : 'en-US', {
    maximumFractionDigits: 1,
  }).format(value);
}

/**
 * MenubarOverview — 菜单栏浮窗的个人概览卡片（只展示方案第 5 节真实指标）。
 *
 * 诚实性规则：
 * - 无缓存 → EmptyState +「打开 Natives 同步」，不显示假 0。
 * - 陈旧缓存 → 仍显示，但标注真实更新时间 +「陈旧」。
 * - 部分来源 → 标注覆盖范围（covered/total），不伪装全量。
 * - 缺失/错误 → 各自呈现，不伪造数值。
 * - 根卷存储：修复前隐藏该卡片，UI 上不显示假 0。
 * - 隐藏时（document.visibilityState === 'hidden'）不运行 usage 读取/扫描/图表动画。
 */
export default function MenubarOverview() {
  const locale = useLocale();
  const [hidden, setHidden] = useState(false);
  const [syncError, setSyncError] = useState<string | null>(null);
  const [registeredProjects, setRegisteredProjects] = useState<number | null>(null);
  const [projectsError, setProjectsError] = useState<string | null>(null);
  const { dialogRef, handleKeyDown } = useFocusTrap();

  // 数据视图固定：当前时区、30d、无项目过滤（冻结契约）。
  const buildViewRequest = useCallback(
    (): UsageViewRequest => ({
      preset: '30d',
      timeZone: Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC',
      projectPath: null,
    }),
    [],
  );

  const {
    state: usageState,
    errorMsg,
    isSyncing,
    sync: syncUsage,
    loadCached,
  } = useUsageData(buildViewRequest);

  const refreshRegisteredProjects = useCallback(async () => {
    try {
      const api = window.nativesAPI?.project;
      if (!api?.list) throw new Error('project API not available');
      const list = await api.list();
      setRegisteredProjects(Array.isArray(list) ? list.length : 0);
      setProjectsError(null);
    } catch (err) {
      setProjectsError(classifyError(err).userMessage);
    }
  }, []);

  // Popup hidden→visible：重新读缓存（绝不触发扫描）；隐藏时仅停动画。
  // 初始挂载的缓存读取由 useUsageData 的 mount effect 承担（Popup 首次创建即显示）。
  useEffect(() => {
    setHidden(document.visibilityState === 'hidden');
    const update = () => {
      const isHidden = document.visibilityState === 'hidden';
      setHidden(isHidden);
      if (!isHidden) {
        void loadCached();
        void refreshRegisteredProjects();
      }
    };
    document.addEventListener('visibilitychange', update);
    return () => document.removeEventListener('visibilitychange', update);
  }, [loadCached, refreshRegisteredProjects]);

  // 跨窗口事件 usage:snapshot-changed 的缓存刷新已收敛在共享 useUsageData
  // （含可见性门控，R-P3/R-P5）。此处不再重复订阅，避免同一事件触发两次
  // loadCached IPC（契约：Popup 隐藏时无重复 IPC）。

  // 只有用户明确点击「刷新」才调用 sync（扫描本机记录）。
  const handleRefresh = useCallback(async () => {
    setSyncError(null);
    const outcome = await syncUsage();
    if (!outcome.ok && outcome.message) setSyncError(outcome.message);
  }, [syncUsage]);

  const handleKeyDownOnRoot = useCallback(
    (e: React.KeyboardEvent<HTMLDivElement>) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        void invokeMenubar('menubar_hide');
        return;
      }
      handleKeyDown(e);
    },
    [handleKeyDown],
  );

  const usage = usageState.kind === 'ready' ? usageState.data : null;
  const metadata = usageState.kind === 'ready' ? usageState.metadata : null;
  const cacheMissing = usageState.kind === 'missing-cache';
  const loading = usageState.kind === 'reading-cache' && !usage;

  const summary = useMemo(() => (usage ? summarizeOverviewUsage(usage, localDateKey()) : null), [usage]);
  const trend = useMemo(() => (usage ? buildOverviewTrend(usage) : []), [usage]);
  const maxTrend = useMemo(() => Math.max(...trend.map((point) => point.totalTokens), 1), [trend]);

  const [nowMs, setNowMs] = useState<number>(() => Date.now());
  useEffect(() => {
    const id = window.setInterval(() => setNowMs(Date.now()), 60_000);
    return () => window.clearInterval(id);
  }, []);
  const isStale = metadata ? nowMs - metadata.generatedAtMs > STALE_AFTER_MS : false;

  const sourceStates = usage?.sources ?? [];
  const usableSources = sourceStates.filter((s) => s.state === 'ok' || s.state === 'partial').length;
  const coveragePartial = sourceStates.length > 0 && usableSources < sourceStates.length;

  const ioTotal = (summary?.inputTokens ?? 0) + (summary?.outputTokens ?? 0);
  const inputPercent = ioTotal > 0 ? Math.round(((summary?.inputTokens ?? 0) / ioTotal) * 100) : null;
  const outputPercent = inputPercent === null ? null : 100 - inputPercent;

  // ── 状态分支 ──
  let body: React.ReactNode;
  if (errorMsg && !usage) {
    body = (
      <ErrorState message={errorMsg} onRetry={() => void loadCached()} />
    );
  } else if (loading) {
    body = (
      <div className={styles.metricGrid}>
        {Array.from({ length: 4 }, (_, index) => (
          <Skeleton key={index} height={60} />
        ))}
      </div>
    );
  } else if (cacheMissing || !usage || !summary) {
    body = (
      <EmptyState
        icon={<BarChart3 size={26} />}
        title={t(locale, 'menubar.noCacheTitle')}
        description={t(locale, 'menubar.noCacheDesc')}
        action={{
          label: t(locale, 'menubar.openNativesSync'),
          onClick: () => void invokeMenubar('menubar_open_main'),
        }}
      />
    );
  } else {
    const ioUnavailable = summary.inputTokens === null && summary.outputTokens === null;
    body = (
      <>
        <div className={styles.metricGrid}>
          <MetricCard
            icon={<Activity size={14} />}
            label={t(locale, 'menubar.todayTokens')}
            value={valueOrUnavailable(summary.todayTokens, locale)}
          />
          <MetricCard
            icon={<BarChart3 size={14} />}
            label={t(locale, 'menubar.thirtyDayTokens')}
            value={valueOrUnavailable(summary.totalTokens, locale)}
          />
          <MetricCard
            icon={<Database size={14} />}
            label={t(locale, 'menubar.inputOutput')}
            value={ioUnavailable
              ? t(locale, 'menubar.unavailable')
              : `${valueOrUnavailable(summary.inputTokens, locale)} / ${valueOrUnavailable(summary.outputTokens, locale)}`}
            hint={inputPercent !== null && outputPercent !== null
              ? `${t(locale, 'menubar.input')} ${inputPercent}% · ${t(locale, 'menubar.output')} ${outputPercent}%`
              : undefined}
          />
          <MetricCard
            icon={<MessageSquare size={14} />}
            label={t(locale, 'menubar.averageMessages')}
            value={summary.averageMessagesPerSession === null
              ? t(locale, 'menubar.unavailable')
              : formatAverage(summary.averageMessagesPerSession, locale)}
          />
        </div>

        <section className={styles.section} aria-label={t(locale, 'menubar.trendAria')}>
          <h2 className={styles.sectionTitle}><BarChart3 size={13} /> {t(locale, 'menubar.trend')}</h2>
          {trend.length > 0 ? (
            <div className={styles.trend} role="img" aria-label={t(locale, 'menubar.trendAria')}>
              {/* 可访问的日期 + 值文本 */}
              <ol className={styles.srOnly}>
                {trend.map((point) => (
                  <li key={point.date}>
                    {t(locale, 'menubar.trendPoint', { date: point.date, value: point.totalTokens.toLocaleString() })}
                  </li>
                ))}
              </ol>
              <div className={styles.trendBars} aria-hidden="true">
                {trend.map((point) => (
                  <div
                    key={point.date}
                    className={styles.trendItem}
                    title={t(locale, 'menubar.trendPoint', { date: point.date, value: point.totalTokens.toLocaleString() })}
                  >
                    <div className={styles.trendRail}>
                      <span
                        className={styles.trendBar}
                        style={{ height: `${Math.max(2, (point.totalTokens / maxTrend) * 100)}%` }}
                      />
                    </div>
                  </div>
                ))}
              </div>
            </div>
          ) : (
            <div className={styles.errorBanner}>
              <TriangleAlert size={13} />
              <span>{t(locale, 'menubar.trendEmpty')}</span>
            </div>
          )}
        </section>

        <section className={styles.section} aria-label={t(locale, 'menubar.activitySummary')}>
          <h2 className={styles.sectionTitle}><Activity size={13} /> {t(locale, 'menubar.activitySummary')}</h2>
          <div className={styles.activityGrid}>
            <ActivityStat
              icon={<FolderCode size={14} />}
              label={t(locale, 'menubar.activeProjects')}
              value={fmtCount(summary.activeProjects, locale)}
            />
            <ActivityStat
              icon={<Users size={14} />}
              label={t(locale, 'menubar.sessions')}
              value={fmtCount(summary.sessions, locale)}
            />
            <ActivityStat
              icon={<MessageSquare size={14} />}
              label={t(locale, 'menubar.messages')}
              value={fmtCount(summary.messages, locale)}
            />
            <ActivityStat
              icon={<FolderCode size={14} />}
              label={t(locale, 'menubar.registeredProjects')}
              value={registeredProjects === null ? t(locale, 'menubar.unavailable') : fmtCount(registeredProjects, locale)}
              hint={projectsError ?? undefined}
            />
          </div>
        </section>
      </>
    );
  }

  const bannerText = errorMsg ?? syncError ?? (projectsError && usage ? projectsError : null);

  return (
    <div
      ref={dialogRef}
      onKeyDown={handleKeyDownOnRoot}
      role="dialog"
      aria-modal="true"
      aria-label={t(locale, 'menubar.title')}
      className={styles.root}
      data-hidden={hidden ? 'true' : 'false'}
    >
      <header className={styles.header}>
        <h1 className={styles.title}>{t(locale, 'menubar.title')}</h1>
        <div className={styles.headerActions}>
          <button
            type="button"
            className={styles.iconButton}
            onClick={() => void handleRefresh()}
            disabled={isSyncing}
            aria-label={t(locale, 'menubar.refresh')}
            title={t(locale, 'menubar.refreshDesc')}
          >
            <RefreshCw size={14} className={isSyncing ? styles.spinIcon : undefined} />
          </button>
          <button
            type="button"
            className={styles.iconButton}
            onClick={() => void invokeMenubar('menubar_hide')}
            aria-label={t(locale, 'menubar.close')}
            title={t(locale, 'menubar.close')}
          >
            <X size={14} />
          </button>
        </div>
      </header>

      <div className={styles.body}>
        {bannerText ? (
          <div className={styles.errorBanner} role="alert">
            <TriangleAlert size={13} />
            <span>{bannerText}</span>
          </div>
        ) : null}
        {body}
      </div>

      <footer className={styles.footer}>
        <div className={styles.metaCol}>
          {metadata ? (
            <div className={styles.metaRow}>
              <span>{t(locale, isStale ? 'menubar.updatedAtStale' : 'menubar.updatedAt', { time: formatUpdatedAt(metadata.generatedAtMs, locale) })}</span>
              {isStale ? <span className={`${styles.badge} ${styles.badgeStale}`}>{t(locale, 'menubar.stale')}</span> : null}
            </div>
          ) : (
            <span>{t(locale, 'menubar.noCacheFooter')}</span>
          )}
          {sourceStates.length > 0 ? (
            <div className={styles.metaRow}>
              <span>{coveragePartial
                ? t(locale, 'menubar.coveragePartial', { covered: usableSources, total: sourceStates.length })
                : t(locale, 'menubar.coverageFull', { count: usableSources })}</span>
              {coveragePartial ? <span className={`${styles.badge} ${styles.badgePartial}`}>{t(locale, 'menubar.partial')}</span> : null}
            </div>
          ) : null}
        </div>
        <div className={styles.footerActions}>
          <button
            type="button"
            className={`btn ${styles.footerButton}`}
            onClick={() => void invokeMenubar('menubar_open_main')}
          >
            <ExternalLink size={12} /> {t(locale, 'menubar.openNatives')}
          </button>
          <button
            type="button"
            className={`btn ${styles.footerButton}`}
            onClick={() => void invokeMenubar('menubar_quit')}
          >
            {t(locale, 'menubar.quit')}
          </button>
        </div>
      </footer>
    </div>
  );
}

function MetricCard({
  icon,
  label,
  value,
  hint,
}: {
  icon: React.ReactNode;
  label: string;
  value: string;
  hint?: string;
}) {
  return (
    <div className={styles.metricCard}>
      <span className={styles.metricLabel}>{icon} {label}</span>
      <span className={styles.metricValue}>{value}</span>
      {hint ? <span className={styles.metricHint}>{hint}</span> : null}
    </div>
  );
}

function ActivityStat({
  icon,
  label,
  value,
  hint,
}: {
  icon: React.ReactNode;
  label: string;
  value: string;
  hint?: string;
}) {
  return (
    <div className={styles.activityStat}>
      <span className={styles.activityStatIcon}>{icon}</span>
      <div style={{ minWidth: 0 }}>
        <div className={styles.activityStatValue}>{value}</div>
        <div className={styles.activityStatLabel} title={hint ?? label}>{label}</div>
      </div>
    </div>
  );
}
