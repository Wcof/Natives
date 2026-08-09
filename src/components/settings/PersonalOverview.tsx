'use client';

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  Activity,
  ArrowRight,
  BarChart3,
  Copy,
  Database,
  FolderCode,
  FolderPlus,
  HardDrive,
  MessageSquare,
  RefreshCw,
  TriangleAlert,
  Users,
} from 'lucide-react';
import { t, type Locale } from '@/i18n';
import { classifyError } from '@/lib/error-classifier';
import { diskApi } from '@/lib/files-api';
import { fmtCount } from '@/lib/format';
import type { ProjectSummary } from '@/lib/tauri-adapter';
import type { UsageViewRequest } from '@/types/usage';
import { useUsageData } from '@/hooks/useUsageData';
import { useToast } from '@/components/ui/Toast';
import { EmptyState } from '@/components/ui/EmptyState';
import {
  buildOverviewTrend,
  summarizeOverviewUsage,
} from './personal-overview-data';
import styles from './PersonalOverview.module.css';

type OverviewRange = '7d' | '30d';

interface DiskInfo {
  totalBytes: number;
  usedBytes: number;
  availableBytes: number;
}

interface OverviewError {
  scope: 'usage' | 'projects' | 'storage';
  message: string;
}

interface PersonalOverviewProps {
  locale: Locale;
  onNavigate?: (view: string) => void;
}

function localDateKey(date = new Date()): string {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, '0');
  const day = String(date.getDate()).padStart(2, '0');
  return `${year}-${month}-${day}`;
}

function isDiskInfo(value: unknown): value is DiskInfo {
  if (!value || typeof value !== 'object') return false;
  const candidate = value as Partial<DiskInfo>;
  return [candidate.totalBytes, candidate.usedBytes, candidate.availableBytes]
    .every((item) => typeof item === 'number' && Number.isFinite(item) && item >= 0);
}

function formatBytes(bytes: number, locale: Locale): string {
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let value = bytes;
  let index = 0;
  while (value >= 1024 && index < units.length - 1) {
    value /= 1024;
    index += 1;
  }
  return `${new Intl.NumberFormat(locale.startsWith('zh') ? 'zh-CN' : 'en-US', {
    maximumFractionDigits: value < 10 ? 1 : 0,
  }).format(value)} ${units[index]}`;
}

function formatUpdatedAt(value: number, locale: Locale): string {
  return new Intl.DateTimeFormat(locale.startsWith('zh') ? 'zh-CN' : 'en-US', {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  }).format(value);
}

function formatProjectTime(value: string | null | undefined, locale: Locale): string {
  if (!value) return t(locale, 'settings.overviewUnavailable');
  const timestamp = Date.parse(value);
  if (!Number.isFinite(timestamp)) return t(locale, 'settings.overviewUnavailable');

  const diffMinutes = Math.round((timestamp - Date.now()) / 60_000);
  const formatter = new Intl.RelativeTimeFormat(
    locale.startsWith('zh') ? 'zh-CN' : 'en-US',
    { numeric: 'auto' },
  );
  if (Math.abs(diffMinutes) < 60) return formatter.format(diffMinutes, 'minute');
  const diffHours = Math.round(diffMinutes / 60);
  if (Math.abs(diffHours) < 24) return formatter.format(diffHours, 'hour');
  const diffDays = Math.round(diffHours / 24);
  if (Math.abs(diffDays) < 7) return formatter.format(diffDays, 'day');
  return new Intl.DateTimeFormat(locale.startsWith('zh') ? 'zh-CN' : 'en-US', {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
  }).format(timestamp);
}

function valueOrUnavailable(value: number | null, locale: Locale): string {
  return value === null
    ? t(locale, 'settings.overviewUnavailable')
    : fmtCount(value, locale);
}

export default function PersonalOverview({ locale, onNavigate }: PersonalOverviewProps) {
  const { toast } = useToast();
  const requestIdRef = useRef(0);
  const [range, setRange] = useState<OverviewRange>('30d');
  const [projects, setProjects] = useState<ProjectSummary[]>([]);
  const [projectsReady, setProjectsReady] = useState(false);
  const [disk, setDisk] = useState<DiskInfo | null>(null);
  const [addingProject, setAddingProject] = useState(false);
  const [errors, setErrors] = useState<OverviewError[]>([]);

  // Usage snapshot data lives in the shared usage-data hook (single source of
  // truth with the dashboard Feature) — Settings composes its own presentation.
  const buildViewRequest = useCallback(
    (): UsageViewRequest => {
      const timeZone = Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC';
      return { preset: range, timeZone, projectPath: null };
    },
    [range],
  );
  const {
    state: usageState,
    errorMsg: usageError,
    isSyncing: syncing,
    sync: syncUsageData,
  } = useUsageData(buildViewRequest);

  const loadEnvironmentData = useCallback(async () => {
    const requestId = ++requestIdRef.current;
    setErrors([]);

    const results = await Promise.allSettled([
      (async () => {
        const api = window.nativesAPI?.project;
        if (!api?.list) throw new Error('project API not available');
        return api.list();
      })(),
      (async () => {
        const value = await diskApi().systemInfo();
        if (!isDiskInfo(value)) throw new Error('invalid disk information');
        return value;
      })(),
    ]);

    if (requestId !== requestIdRef.current) return;
    const nextErrors: OverviewError[] = [];
    const [projectResult, diskResult] = results;

    if (projectResult.status === 'fulfilled') {
      setProjects(projectResult.value ?? []);
      setProjectsReady(true);
    } else {
      nextErrors.push({ scope: 'projects', message: classifyError(projectResult.reason).userMessage });
    }

    if (diskResult.status === 'fulfilled') {
      setDisk(diskResult.value);
    } else {
      nextErrors.push({ scope: 'storage', message: classifyError(diskResult.reason).userMessage });
    }

    setErrors(nextErrors);
  }, []);

  useEffect(() => {
    void loadEnvironmentData();
    return () => {
      requestIdRef.current += 1;
    };
  }, [loadEnvironmentData]);

  const addProject = useCallback(async () => {
    setAddingProject(true);
    try {
      const path = await window.nativesAPI?.dialog?.pickDirectory?.();
      if (!path) return;
      const api = window.nativesAPI?.project;
      if (!api?.register || !api.list) throw new Error('project API not available');
      await api.register(path);
      setProjects(await api.list());
      setProjectsReady(true);
      setErrors((current) => current.filter((error) => error.scope !== 'projects'));
      toast(t(locale, 'settings.overviewProjectAdded'), 'success');
    } catch (error) {
      const message = classifyError(error).userMessage;
      setErrors((current) => [
        ...current.filter((item) => item.scope !== 'projects'),
        { scope: 'projects', message },
      ]);
      toast(message, 'error');
    } finally {
      setAddingProject(false);
    }
  }, [locale, toast]);

  const syncUsage = useCallback(async () => {
    const outcome = await syncUsageData();
    if (outcome.ok) {
      setErrors((current) => current.filter((error) => error.scope !== 'usage'));
      toast(t(locale, 'settings.overviewSyncSuccess'), 'success');
    } else if (outcome.message) {
      const message = outcome.message;
      setErrors((current) => [
        ...current.filter((item) => item.scope !== 'usage'),
        { scope: 'usage', message },
      ]);
      toast(message, 'error');
    }
  }, [locale, syncUsageData, toast]);

  const usage = usageState.kind === 'ready' ? usageState.data : null;
  const metadata = usageState.kind === 'ready' ? usageState.metadata : null;
  const cacheMissing = usageState.kind === 'missing-cache';
  const loading = usageState.kind === 'reading-cache';

  const summary = useMemo(
    () => (usage ? summarizeOverviewUsage(usage, localDateKey()) : null),
    [usage],
  );
  const trend = useMemo(() => (usage ? buildOverviewTrend(usage) : []), [usage]);
  const visibleTrend = range === '7d' ? trend.slice(-7) : trend.slice(-30);
  const maxTrend = Math.max(...visibleTrend.map((point) => point.totalTokens), 1);
  const ioTotal = (summary?.inputTokens ?? 0) + (summary?.outputTokens ?? 0);
  const inputPercent = ioTotal > 0 ? ((summary?.inputTokens ?? 0) / ioTotal) * 100 : null;
  const outputPercent = inputPercent === null ? null : 100 - inputPercent;
  const diskPercent = disk && disk.totalBytes > 0 ? (disk.usedBytes / disk.totalBytes) * 100 : null;
  const recentProjects = projects.slice(0, 5);

  const bannerErrors = useMemo(() => {
    const env = errors.filter((error) => error.scope !== 'usage');
    if (usageError) {
      return [...env, { scope: 'usage' as const, message: usageError }];
    }
    return env;
  }, [errors, usageError]);

  return (
    <div className={styles.root}>
      <header className={styles.header}>
        <div>
          <h2 className={styles.heading}>{t(locale, 'settings.tabOverview')}</h2>
          <p className={styles.subtitle}>{t(locale, 'settings.overviewDesc')}</p>
        </div>
        <div className={styles.syncGroup}>
          {metadata ? (
            <span className={styles.updatedAt}>
              {t(locale, 'settings.overviewUpdatedAt', { time: formatUpdatedAt(metadata.generatedAtMs, locale) })}
            </span>
          ) : null}
          <button
            type="button"
            className={styles.button}
            onClick={() => void syncUsage()}
            disabled={syncing}
          >
            <RefreshCw size={13} className={syncing ? 'animate-spin' : undefined} />
            {syncing ? t(locale, 'settings.overviewSyncing') : t(locale, 'settings.overviewSync')}
          </button>
        </div>
      </header>

      {bannerErrors.length > 0 ? (
        <div className={styles.errorBanner} role="alert">
          <TriangleAlert size={14} />
          <span>{bannerErrors.map((error) => error.message).join(' · ')}</span>
        </div>
      ) : null}

      <div className={styles.metricsGrid}>
        {loading && !usage ? (
          Array.from({ length: 4 }, (_, index) => (
            <div key={index} className={`${styles.card} ${styles.skeleton}`} />
          ))
        ) : (
          <>
            <MetricCard
              icon={<Activity size={14} />}
              label={t(locale, 'settings.overviewTodayTokens')}
              value={valueOrUnavailable(summary?.todayTokens ?? null, locale)}
              hint={t(locale, 'settings.overviewTodayHint')}
            />
            <MetricCard
              icon={<BarChart3 size={14} />}
              label={t(locale, range === '7d' ? 'settings.overviewSevenDayTokens' : 'settings.overviewThirtyDayTokens')}
              value={valueOrUnavailable(summary?.totalTokens ?? null, locale)}
              hint={usage
                ? t(locale, 'settings.overviewSources', { count: usage.sources.filter((source) => source.state === 'ok' || source.state === 'partial').length })
                : t(locale, 'settings.overviewUnavailable')}
            />
            <MetricCard
              icon={<Database size={14} />}
              label={t(locale, 'settings.overviewInputOutput')}
              value={summary?.inputTokens === null && summary?.outputTokens === null
                ? t(locale, 'settings.overviewUnavailable')
                : `${valueOrUnavailable(summary?.inputTokens ?? null, locale)} / ${valueOrUnavailable(summary?.outputTokens ?? null, locale)}`}
              hint={inputPercent !== null && outputPercent !== null ? (
                <>
                  <span className={styles.splitBar} aria-hidden="true">
                    <span className={styles.splitInput} style={{ width: `${inputPercent}%` }} />
                    <span className={styles.splitOutput} style={{ width: `${outputPercent}%` }} />
                  </span>
                  <span className={styles.splitLegend}>
                    <span>{t(locale, 'settings.overviewInput')} {Math.round(inputPercent)}%</span>
                    <span>{t(locale, 'settings.overviewOutput')} {Math.round(outputPercent)}%</span>
                  </span>
                </>
              ) : t(locale, 'settings.overviewBreakdownUnavailable')}
            />
            <MetricCard
              icon={<MessageSquare size={14} />}
              label={t(locale, 'settings.overviewAverageMessages')}
              value={summary?.averageMessagesPerSession === null || summary?.averageMessagesPerSession === undefined
                ? t(locale, 'settings.overviewUnavailable')
                : new Intl.NumberFormat(locale.startsWith('zh') ? 'zh-CN' : 'en-US', { maximumFractionDigits: 1 }).format(summary.averageMessagesPerSession)}
              hint={t(locale, 'settings.overviewAverageMessagesHint')}
            />
          </>
        )}
      </div>

      <div className={styles.activityGrid}>
        <section className={`${styles.card} ${styles.sectionCard}`}>
          <div className={styles.sectionHeader}>
            <h3 className={styles.sectionTitle}><BarChart3 size={14} /> {t(locale, 'settings.overviewActivity')}</h3>
            <div className={styles.rangeControl} aria-label={t(locale, 'settings.overviewRange')}>
              {(['7d', '30d'] as const).map((item) => (
                <button
                  key={item}
                  type="button"
                  className={`${styles.rangeButton} ${range === item ? styles.rangeButtonActive : ''}`}
                  onClick={() => setRange(item)}
                  aria-pressed={range === item}
                >
                  {t(locale, item === '7d' ? 'settings.overviewSevenDays' : 'settings.overviewThirtyDays')}
                </button>
              ))}
            </div>
          </div>
          {visibleTrend.length > 0 ? (
            <div className={styles.trend} aria-label={t(locale, 'settings.overviewTokenTrend')}>
              {visibleTrend.map((point) => (
                <div key={point.date} className={styles.trendItem} title={`${point.date}: ${point.totalTokens.toLocaleString()}`}>
                  <div className={styles.trendRail}>
                    <span className={styles.trendBar} style={{ height: `${Math.max(2, (point.totalTokens / maxTrend) * 100)}%` }} />
                  </div>
                  <span className={styles.trendDate}>{point.date.slice(5)}</span>
                </div>
              ))}
            </div>
          ) : (
            <EmptyState
              icon={<BarChart3 size={24} />}
              title={cacheMissing ? t(locale, 'settings.overviewNoUsageTitle') : t(locale, 'settings.overviewNoTrendTitle')}
              description={cacheMissing ? t(locale, 'settings.overviewNoUsage') : t(locale, 'settings.overviewNoTrend')}
              action={cacheMissing ? { label: t(locale, 'settings.overviewSync'), onClick: () => void syncUsage() } : undefined}
            />
          )}
        </section>

        <section className={`${styles.card} ${styles.sectionCard}`}>
          <div className={styles.sectionHeader}>
            <h3 className={styles.sectionTitle}><Activity size={14} /> {t(locale, 'settings.overviewActivitySummary')}</h3>
          </div>
          <div className={styles.activityStats}>
            <ActivityStat icon={<FolderCode size={15} />} label={t(locale, 'settings.overviewActiveProjects')} value={summary?.activeProjects ?? null} locale={locale} />
            <ActivityStat icon={<Users size={15} />} label={t(locale, 'settings.overviewSessions')} value={summary?.sessions ?? null} locale={locale} />
            <ActivityStat icon={<MessageSquare size={15} />} label={t(locale, 'settings.overviewMessages')} value={summary?.messages ?? null} locale={locale} />
            <ActivityStat icon={<FolderCode size={15} />} label={t(locale, 'settings.overviewRegisteredProjects')} value={projectsReady ? projects.length : null} locale={locale} />
          </div>
        </section>
      </div>

      <section className={`${styles.card} ${styles.sectionCard}`}>
        <div className={styles.sectionHeader}>
          <h3 className={styles.sectionTitle}><HardDrive size={14} /> {t(locale, 'settings.overviewStorage')}</h3>
        </div>
        {disk && diskPercent !== null ? (
          <div className={styles.storageContent}>
            <div>
              <div className={styles.storageValue}>{formatBytes(disk.usedBytes, locale)} <span className={styles.projectMeta}>/ {formatBytes(disk.totalBytes, locale)}</span></div>
              <div className={styles.storageMeta}>{t(locale, 'settings.overviewSystemDisk')}</div>
            </div>
            <div>
              <div className={styles.storageTrack}>
                <div className={styles.storageFill} style={{ width: `${Math.min(100, diskPercent)}%` }} />
              </div>
              <div className={styles.storageMeta}>
                {t(locale, 'settings.overviewStorageUsed', { percent: Math.round(diskPercent) })} · {t(locale, 'settings.overviewStorageAvailable', { size: formatBytes(disk.availableBytes, locale) })}
              </div>
            </div>
          </div>
        ) : (
          <div className={styles.emptyCompact}>{t(locale, 'settings.overviewStorageUnavailable')}</div>
        )}
      </section>

      <div className={styles.lowerGrid}>
        <section className={`${styles.card} ${styles.sectionCard} ${styles.projectsCard}`}>
          <div className={styles.sectionHeader}>
            <h3 className={styles.sectionTitle}><FolderCode size={14} /> {t(locale, 'settings.overviewRecentProjects')}</h3>
            {projects.length > 5 ? (
              <button type="button" className={styles.button} onClick={() => onNavigate?.('assistant')} disabled={!onNavigate}>
                {t(locale, 'settings.overviewAllProjects')} <ArrowRight size={12} />
              </button>
            ) : null}
          </div>
          {recentProjects.length > 0 ? (
            <div className={styles.projectsList}>
              {recentProjects.map((project) => (
                <div className={styles.projectRow} key={project.id}>
                  <div className={styles.projectIdentity}>
                    <span className={styles.projectIcon}><FolderCode size={14} /></span>
                    <div style={{ minWidth: 0 }}>
                      <div className={styles.projectName}>{project.label}</div>
                      <div className={styles.projectPath} title={project.path}>{project.path}</div>
                    </div>
                  </div>
                  <span className={styles.projectMeta}>{t(locale, 'settings.overviewConversationCount', { count: project.conversationCount })}</span>
                  <span className={`${styles.projectMeta} ${!project.exists ? styles.projectMissing : ''}`}>
                    {project.exists
                      ? formatProjectTime(project.lastOpenedAt, locale)
                      : t(locale, 'settings.overviewProjectMissing')}
                  </span>
                  <button
                    type="button"
                    className={styles.iconButton}
                    aria-label={t(locale, 'settings.overviewCopyPath')}
                    title={t(locale, 'settings.overviewCopyPath')}
                    onClick={async () => {
                      try {
                        const clipboard = window.nativesAPI?.clipboard;
                        if (!clipboard?.write) throw new Error('clipboard API not available');
                        await clipboard.write(project.path);
                        toast(t(locale, 'settings.overviewPathCopied'), 'success');
                      } catch (error) {
                        toast(classifyError(error).userMessage, 'error');
                      }
                    }}
                  >
                    <Copy size={13} />
                  </button>
                </div>
              ))}
            </div>
          ) : projectsReady ? (
            <EmptyState
              icon={<FolderCode size={24} />}
              title={t(locale, 'settings.overviewNoProjectsTitle')}
              description={t(locale, 'settings.overviewNoProjects')}
              action={{ label: t(locale, 'settings.overviewAddProject'), onClick: () => void addProject() }}
            />
          ) : (
            <div className={styles.emptyCompact}>{t(locale, 'settings.overviewUnavailable')}</div>
          )}
        </section>

        <section className={`${styles.card} ${styles.sectionCard}`}>
          <div className={styles.sectionHeader}>
            <h3 className={styles.sectionTitle}>{t(locale, 'settings.overviewQuickActions')}</h3>
          </div>
          <div className={styles.quickActions}>
            <QuickAction
              icon={<FolderPlus size={16} />}
              title={t(locale, 'settings.overviewAddProject')}
              description={t(locale, 'settings.overviewAddProjectDesc')}
              onClick={() => void addProject()}
              disabled={addingProject}
            />
            <QuickAction
              icon={<RefreshCw size={16} />}
              title={t(locale, 'settings.overviewSync')}
              description={t(locale, 'settings.overviewSyncDesc')}
              onClick={() => void syncUsage()}
              disabled={syncing}
            />
            <QuickAction
              icon={<BarChart3 size={16} />}
              title={t(locale, 'settings.overviewOpenUsage')}
              description={t(locale, 'settings.overviewOpenUsageDesc')}
              onClick={() => onNavigate?.('dashboard')}
              disabled={!onNavigate}
            />
          </div>
        </section>
      </div>
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
  hint: React.ReactNode;
}) {
  return (
    <div className={`${styles.card} ${styles.metricCard}`}>
      <span className={styles.metricLabel}>{icon} {label}</span>
      <span className={styles.metricValue}>{value}</span>
      <span className={styles.metricHint}>{hint}</span>
    </div>
  );
}

function ActivityStat({
  icon,
  label,
  value,
  locale,
}: {
  icon: React.ReactNode;
  label: string;
  value: number | null;
  locale: Locale;
}) {
  return (
    <div className={styles.activityStat}>
      <span className={styles.activityStatIcon}>{icon}</span>
      <div>
        <div className={styles.activityStatValue}>{valueOrUnavailable(value, locale)}</div>
        <div className={styles.projectMeta}>{label}</div>
      </div>
    </div>
  );
}

function QuickAction({
  icon,
  title,
  description,
  onClick,
  disabled,
}: {
  icon: React.ReactNode;
  title: string;
  description: string;
  onClick: () => void;
  disabled?: boolean;
}) {
  return (
    <button type="button" className={styles.quickAction} onClick={onClick} disabled={disabled}>
      <span className={styles.quickActionIcon}>{icon}</span>
      <span className={styles.quickActionCopy}>
        <strong>{title}</strong>
        <span>{description}</span>
      </span>
      <ArrowRight size={14} />
    </button>
  );
}
