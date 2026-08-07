'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import {
  ArrowRight,
  Copy,
  FolderCode,
  FolderPlus,
  HardDrive,
  RefreshCw,
  TriangleAlert,
} from 'lucide-react';
import { t, type Locale } from '@/i18n';
import { classifyError } from '@/lib/error-classifier';
import { diskApi } from '@/lib/files-api';
import type { ProjectSummary } from '@/lib/tauri-adapter';
import { useToast } from '@/components/ui/Toast';
import { EmptyState } from '@/components/ui/EmptyState';
import { UsageDashboard } from '@/components/dashboard/UsageDashboard';
import styles from './PersonalOverview.module.css';

interface DiskInfo {
  totalBytes: number;
  usedBytes: number;
  availableBytes: number;
}

interface OverviewError {
  scope: 'projects' | 'storage';
  message: string;
}

interface PersonalOverviewProps {
  locale: Locale;
  onNavigate?: (view: string) => void;
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

export default function PersonalOverview({ locale, onNavigate }: PersonalOverviewProps) {
  const { toast } = useToast();
  const requestIdRef = useRef(0);
  const [projects, setProjects] = useState<ProjectSummary[]>([]);
  const [projectsReady, setProjectsReady] = useState(false);
  const [disk, setDisk] = useState<DiskInfo | null>(null);
  const [addingProject, setAddingProject] = useState(false);
  const [errors, setErrors] = useState<OverviewError[]>([]);

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

  const diskPercent = disk && disk.totalBytes > 0 ? (disk.usedBytes / disk.totalBytes) * 100 : null;
  const recentProjects = projects.slice(0, 5);

  return (
    <div className={styles.root}>
      <header className={styles.header}>
        <div>
          <h2 className={styles.heading}>{t(locale, 'settings.tabOverview')}</h2>
          <p className={styles.subtitle}>{t(locale, 'settings.overviewDesc')}</p>
        </div>
      </header>

      {errors.length > 0 ? (
        <div className={styles.errorBanner} role="alert">
          <TriangleAlert size={14} />
          <span>{errors.map((error) => error.message).join(' · ')}</span>
        </div>
      ) : null}

      {/* 复刻《个人主页》（UsageDashboard）的全部用量分析、看板与图表，并将存储概览与近期项目作为 children，实现《数据来源与覆盖》置于最底部 */}
      <UsageDashboard>
        <div className={styles.environmentGrid}>
          <section className={styles.card}>
            <div className={styles.cardHeader}>
              <h3 className={styles.cardTitle}>
                <HardDrive size={14} /> {t(locale, 'settings.overviewStorage')}
              </h3>
            </div>
            {disk && diskPercent !== null ? (
              <div className={styles.storageContent}>
                <div>
                  <div className={styles.storageValue}>
                    {formatBytes(disk.usedBytes, locale)}{' '}
                    <span className={styles.storageSubText}>/ {formatBytes(disk.totalBytes, locale)}</span>
                  </div>
                  <div className={styles.storageMeta}>{t(locale, 'settings.overviewSystemDisk')}</div>
                </div>
                <div>
                  <div className={styles.storageTrack}>
                    <div className={styles.storageFill} style={{ width: `${Math.min(100, diskPercent)}%` }} />
                  </div>
                  <div className={styles.storageMeta}>
                    {t(locale, 'settings.overviewStorageUsed', { percent: Math.round(diskPercent) })} ·{' '}
                    {t(locale, 'settings.overviewStorageAvailable', { size: formatBytes(disk.availableBytes, locale) })}
                  </div>
                </div>
              </div>
            ) : (
              <div className={styles.emptyCompact}>{t(locale, 'settings.overviewStorageUnavailable')}</div>
            )}
          </section>

          <div className={styles.lowerGrid}>
            <section className={`${styles.card} ${styles.projectsCard}`}>
              <div className={styles.cardHeader}>
                <h3 className={styles.cardTitle}>
                  <FolderCode size={14} /> {t(locale, 'settings.overviewRecentProjects')}
                </h3>
                {projects.length > 5 ? (
                  <button
                    type="button"
                    className={styles.button}
                    onClick={() => onNavigate?.('assistant')}
                    disabled={!onNavigate}
                  >
                    {t(locale, 'settings.overviewAllProjects')} <ArrowRight size={12} />
                  </button>
                ) : null}
              </div>
              {recentProjects.length > 0 ? (
                <div className={styles.projectsList}>
                  {recentProjects.map((project) => (
                    <div className={styles.projectRow} key={project.id}>
                      <div className={styles.projectIdentity}>
                        <span className={styles.projectIcon}>
                          <FolderCode size={14} />
                        </span>
                        <div style={{ minWidth: 0 }}>
                          <div className={styles.projectName}>{project.label}</div>
                          <div className={styles.projectPath} title={project.path}>
                            {project.path}
                          </div>
                        </div>
                      </div>
                      <span className={styles.projectMeta}>
                        {t(locale, 'settings.overviewConversationCount', { count: project.conversationCount })}
                      </span>
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

            <section className={styles.card}>
              <div className={styles.cardHeader}>
                <h3 className={styles.cardTitle}>{t(locale, 'settings.overviewQuickActions')}</h3>
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
                  onClick={() => void loadEnvironmentData()}
                />
              </div>
            </section>
          </div>
        </div>
      </UsageDashboard>
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

