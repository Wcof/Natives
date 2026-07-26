'use client';

/**
 * JobsPage — 任务模块主界面（列表 / 运行历史两个页签）
 *
 * 数据源：job_list / job_get / job_set_enabled / job_delete / job_run_now
 * （契约第 5 节）。dispatcher_wired=false 时「立即执行」禁用并展示原因，
 * 三态诚实（R-F2）：不显示任何假进度/假状态。
 *
 * 轮询：15s（不低于 10s 下限），页面隐藏暂停（technical/04-performance.md），
 * cleanup 齐全（frontend/02-state-and-data.md）。
 */

import { useCallback, useEffect, useMemo, useState } from 'react';
import { CalendarClock, Play, Pencil, Trash2, Plus, History, AlertTriangle } from 'lucide-react';
import { t, useLocale } from '@/i18n';
import { useToast } from '@/components/ui/Toast';
import ConfirmDialog from '@/components/ui/ConfirmDialog';
import { EmptyState, ErrorState, LoadingState } from '@/components/ui/EmptyState';
import JobStatusBadge from './JobStatusBadge';
import JobFormModal from './JobFormModal';
import JobRunsView from './JobRunsView';
import {
  jobList,
  jobGet,
  jobDelete,
  jobSetEnabled,
  jobRunNow,
  extractJobErrorCode,
  type JobDetail,
  type JobListResult,
  type JobSummary,
} from '@/lib/jobs-api';
import { parseJobLastStatus, scheduleSummarySpec } from '@/lib/jobs-view';
import { fmtDateTime, fmtDurationCompact } from '@/lib/format';

/** 轮询间隔 — 标准要求普通轮询不短于 10s；页面隐藏时暂停 */
const POLL_INTERVAL_MS = 15_000;

type JobsTab = 'list' | 'runs';

function fmtIso(iso: string | null | undefined): string {
  if (!iso) return '—';
  const ms = Date.parse(iso);
  return Number.isFinite(ms) ? fmtDateTime(ms) : iso;
}

export default function JobsPage() {
  const locale = useLocale();
  const { toast } = useToast();

  const [data, setData] = useState<JobListResult | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [tab, setTab] = useState<JobsTab>('list');
  const [runsFilter, setRunsFilter] = useState<{ id: string; name: string } | null>(null);
  const [formOpen, setFormOpen] = useState(false);
  const [editing, setEditing] = useState<JobDetail | null>(null);
  const [deleting, setDeleting] = useState<JobSummary | null>(null);
  const [busyId, setBusyId] = useState<string | null>(null);

  // 首个语句即 await，effect 内不产生同步 setState（react-hooks/set-state-in-effect）
  const refresh = useCallback(async (silent: boolean) => {
    try {
      const result = await jobList();
      setData(result);
      setLoadError(null);
    } catch (err) {
      // 静默轮询失败不清空已展示的数据，仅在首屏加载失败时全屏报错
      if (!silent) setLoadError(err instanceof Error ? err.message : String(err));
    } finally {
      if (!silent) setLoading(false);
    }
  }, []);

  // 手动重试（事件处理器内允许同步 setState）
  const retry = useCallback(() => {
    setLoading(true);
    setLoadError(null);
    void refresh(false);
  }, [refresh]);

  useEffect(() => {
    // 挂载即同步外部系统（job_list）；refresh 内所有 setState 均发生在 await 之后
    // eslint-disable-next-line react-hooks/set-state-in-effect
    void refresh(false);
    const timer = window.setInterval(() => {
      if (document.visibilityState === 'visible') void refresh(true);
    }, POLL_INTERVAL_MS);
    const onVisibility = () => {
      if (document.visibilityState === 'visible') void refresh(true);
    };
    document.addEventListener('visibilitychange', onVisibility);
    return () => {
      window.clearInterval(timer);
      document.removeEventListener('visibilitychange', onVisibility);
    };
  }, [refresh]);

  const jobs = useMemo(() => data?.jobs ?? [], [data]);
  const dispatcherWired = data?.dispatcher_wired ?? false;
  const jobNames = useMemo(() => {
    const map = new Map<string, string>();
    for (const job of jobs) map.set(job.id, job.name);
    return map;
  }, [jobs]);

  const reportError = useCallback(
    (err: unknown) => {
      const code = extractJobErrorCode(err);
      const message = code
        ? t(locale, `jobs.errors.${code}`)
        : t(locale, 'jobs.errors.unknown', {
            message: err instanceof Error ? err.message : String(err),
          });
      toast(message, 'error');
    },
    [locale, toast],
  );

  const handleToggleEnabled = async (job: JobSummary) => {
    setBusyId(job.id);
    try {
      await jobSetEnabled(job.id, !job.enabled);
      await refresh(true);
    } catch (err) {
      reportError(err);
    } finally {
      setBusyId(null);
    }
  };

  const handleRunNow = async (job: JobSummary) => {
    setBusyId(job.id);
    try {
      const receipt = await jobRunNow(job.id);
      toast(t(locale, 'jobs.runNowStarted', { runId: receipt.run_id }), 'success');
      await refresh(true);
    } catch (err) {
      reportError(err);
    } finally {
      setBusyId(null);
    }
  };

  const handleEdit = async (job: JobSummary) => {
    setBusyId(job.id);
    try {
      const detail = await jobGet(job.id);
      setEditing(detail);
      setFormOpen(true);
    } catch (err) {
      reportError(err);
    } finally {
      setBusyId(null);
    }
  };

  const handleDelete = async () => {
    if (!deleting) return;
    const target = deleting;
    setDeleting(null);
    try {
      await jobDelete(target.id);
      toast(t(locale, 'jobs.deleted'), 'success');
      await refresh(true);
    } catch (err) {
      reportError(err);
    }
  };

  const scheduleText = (job: JobSummary): string => {
    const spec = scheduleSummarySpec(job.schedule_type, job.schedule_value);
    switch (spec.kind) {
      case 'once':
        return t(locale, 'jobs.scheduleSummary.once', {
          time: spec.timeMs != null ? fmtDateTime(spec.timeMs) : spec.raw,
        });
      case 'interval':
        return t(locale, 'jobs.scheduleSummary.interval', {
          value: spec.seconds != null ? fmtDurationCompact(spec.seconds, locale) : spec.raw,
        });
      case 'cron':
        return t(locale, 'jobs.scheduleSummary.cron', { expr: spec.expr });
    }
  };

  const statusBadge = (job: JobSummary) => {
    const parsed = parseJobLastStatus(job.last_status);
    const label =
      parsed.key === 'raw'
        ? parsed.detail ?? ''
        : t(locale, `jobs.jobStatus.${parsed.key}`);
    return <JobStatusBadge tone={parsed.tone} label={label} title={parsed.detail} />;
  };

  return (
    <div style={{ height: '100%', display: 'flex', flexDirection: 'column', padding: '16px 20px', gap: 12, overflow: 'hidden' }}>
      {/* 页头：标题 + 页签 + 新建 */}
      <div style={{ display: 'flex', alignItems: 'center', gap: 12, flexShrink: 0 }}>
        <CalendarClock size={18} style={{ color: 'var(--primary)' }} />
        <h1 style={{ fontSize: '1rem', fontWeight: 600, color: 'var(--text)', margin: 0 }}>
          {t(locale, 'jobs.title')}
        </h1>
        <div style={{ display: 'flex', gap: 4, marginLeft: 8 }}>
          {(['list', 'runs'] as const).map((key) => (
            <button
              key={key}
              type="button"
              onClick={() => setTab(key)}
              aria-pressed={tab === key}
              className="btn-ghost"
              style={{
                fontSize: '0.8125rem',
                padding: '3px 10px',
                borderRadius: 8,
                background: tab === key ? 'var(--surface-hover)' : 'transparent',
                color: tab === key ? 'var(--text)' : 'var(--text-secondary)',
                fontWeight: tab === key ? 600 : 400,
              }}
            >
              {t(locale, key === 'list' ? 'jobs.tabList' : 'jobs.tabRuns')}
            </button>
          ))}
        </div>
        <div style={{ flex: 1 }} />
        <button
          type="button"
          className="btn btn-primary"
          onClick={() => {
            setEditing(null);
            setFormOpen(true);
          }}
          style={{ display: 'inline-flex', alignItems: 'center', gap: 6, fontSize: '0.8125rem' }}
        >
          <Plus size={14} />
          {t(locale, 'jobs.createJob')}
        </button>
      </div>

      {/* 执行链路未接线提示（三态诚实） */}
      {data != null && !dispatcherWired && (
        <div
          role="status"
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: 8,
            padding: '8px 12px',
            borderRadius: 10,
            border: '1px solid var(--warning)',
            color: 'var(--warning)',
            fontSize: '0.8125rem',
            flexShrink: 0,
          }}
        >
          <AlertTriangle size={14} style={{ flexShrink: 0 }} />
          {t(locale, 'jobs.notWiredBanner')}
        </div>
      )}

      <div style={{ flex: 1, minHeight: 0, overflow: 'auto' }}>
        {tab === 'runs' ? (
          <JobRunsView
            key={runsFilter?.id ?? '__all__'}
            jobFilter={runsFilter}
            onClearFilter={() => setRunsFilter(null)}
            jobNames={jobNames}
          />
        ) : loading ? (
          <LoadingState message={t(locale, 'common.loading')} />
        ) : loadError ? (
          <ErrorState
            message={`${t(locale, 'jobs.loadFailed')} — ${loadError}`}
            onRetry={retry}
          />
        ) : jobs.length === 0 ? (
          <EmptyState
            icon={<CalendarClock size={32} style={{ color: 'var(--text-disabled)' }} />}
            title={t(locale, 'jobs.emptyTitle')}
            description={t(locale, 'jobs.emptyDescription')}
            action={{
              label: t(locale, 'jobs.createJob'),
              onClick: () => {
                setEditing(null);
                setFormOpen(true);
              },
            }}
          />
        ) : (
          <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
            {jobs.map((job) => {
              const busy = busyId === job.id;
              return (
                <div
                  key={job.id}
                  style={{
                    display: 'flex',
                    alignItems: 'center',
                    gap: 12,
                    padding: '10px 14px',
                    borderRadius: 12,
                    border: '1px solid var(--border)',
                    background: 'var(--surface)',
                    opacity: job.enabled ? 1 : 0.65,
                  }}
                >
                  {/* enabled 开关 */}
                  <button
                    type="button"
                    role="switch"
                    aria-checked={job.enabled}
                    aria-label={t(locale, job.enabled ? 'jobs.disableJob' : 'jobs.enableJob')}
                    title={t(locale, job.enabled ? 'jobs.disableJob' : 'jobs.enableJob')}
                    disabled={busy}
                    onClick={() => void handleToggleEnabled(job)}
                    style={{
                      width: 34,
                      height: 18,
                      borderRadius: 999,
                      border: '1px solid var(--border)',
                      background: job.enabled ? 'var(--success)' : 'var(--surface-hover)',
                      position: 'relative',
                      cursor: busy ? 'wait' : 'pointer',
                      flexShrink: 0,
                      padding: 0,
                    }}
                  >
                    <span
                      style={{
                        position: 'absolute',
                        top: 1,
                        left: job.enabled ? 17 : 1,
                        width: 14,
                        height: 14,
                        borderRadius: '50%',
                        background: 'var(--surface)',
                        boxShadow: 'var(--shadow-card)',
                        transition: 'left 0.15s ease',
                      }}
                    />
                  </button>

                  {/* 名称 + schedule 摘要 */}
                  <div style={{ minWidth: 0, flex: 1 }}>
                    <div style={{ display: 'flex', alignItems: 'center', gap: 8, minWidth: 0 }}>
                      <span
                        style={{
                          fontSize: '0.875rem',
                          fontWeight: 600,
                          color: 'var(--text)',
                          overflow: 'hidden',
                          textOverflow: 'ellipsis',
                          whiteSpace: 'nowrap',
                        }}
                        title={job.description ?? job.name}
                      >
                        {job.name}
                      </span>
                      {statusBadge(job)}
                    </div>
                    <div
                      style={{
                        display: 'flex',
                        gap: 14,
                        fontSize: '0.75rem',
                        color: 'var(--text-disabled)',
                        marginTop: 2,
                        flexWrap: 'wrap',
                      }}
                    >
                      <span>{scheduleText(job)}</span>
                      <span>
                        {t(locale, 'jobs.nextRun')}: {job.enabled ? fmtIso(job.next_run) : '—'}
                      </span>
                      <span>
                        {t(locale, 'jobs.lastRun')}: {fmtIso(job.last_run_at)}
                      </span>
                    </div>
                  </div>

                  {/* 操作区 */}
                  <div style={{ display: 'flex', alignItems: 'center', gap: 4, flexShrink: 0 }}>
                    <button
                      type="button"
                      className="btn-ghost"
                      disabled={!dispatcherWired || busy}
                      onClick={() => void handleRunNow(job)}
                      aria-label={t(locale, 'jobs.runNow')}
                      title={
                        dispatcherWired
                          ? t(locale, 'jobs.runNow')
                          : t(locale, 'jobs.runNowNotWired')
                      }
                      style={{
                        display: 'inline-flex',
                        alignItems: 'center',
                        gap: 4,
                        fontSize: '0.75rem',
                        opacity: dispatcherWired ? 1 : 0.45,
                        cursor: dispatcherWired ? 'pointer' : 'not-allowed',
                      }}
                    >
                      <Play size={13} />
                      {t(locale, 'jobs.runNow')}
                    </button>
                    {!dispatcherWired && (
                      <span style={{ fontSize: '0.6875rem', color: 'var(--text-disabled)', maxWidth: 150 }}>
                        {t(locale, 'jobs.runNowNotWired')}
                      </span>
                    )}
                    <button
                      type="button"
                      className="btn-ghost"
                      onClick={() => {
                        setRunsFilter({ id: job.id, name: job.name });
                        setTab('runs');
                      }}
                      aria-label={t(locale, 'jobs.viewRuns')}
                      title={t(locale, 'jobs.viewRuns')}
                      style={{ display: 'inline-flex', padding: 6 }}
                    >
                      <History size={14} />
                    </button>
                    <button
                      type="button"
                      className="btn-ghost"
                      disabled={busy}
                      onClick={() => void handleEdit(job)}
                      aria-label={t(locale, 'common.edit')}
                      title={t(locale, 'common.edit')}
                      style={{ display: 'inline-flex', padding: 6 }}
                    >
                      <Pencil size={14} />
                    </button>
                    <button
                      type="button"
                      className="btn-ghost"
                      onClick={() => setDeleting(job)}
                      aria-label={t(locale, 'jobs.deleteJob')}
                      title={t(locale, 'jobs.deleteJob')}
                      style={{ display: 'inline-flex', padding: 6, color: 'var(--danger)' }}
                    >
                      <Trash2 size={14} />
                    </button>
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </div>

      <JobFormModal
        key={formOpen ? editing?.id ?? '__new__' : '__closed__'}
        open={formOpen}
        initial={editing}
        onClose={() => setFormOpen(false)}
        onSaved={(_job, created) => {
          toast(t(locale, created ? 'jobs.created' : 'jobs.saved'), 'success');
          void refresh(true);
        }}
      />

      <ConfirmDialog
        open={deleting != null}
        title={t(locale, 'jobs.deleteConfirmTitle')}
        message={t(locale, 'jobs.deleteConfirmMessage', { name: deleting?.name ?? '' })}
        confirmLabel={t(locale, 'common.delete')}
        cancelLabel={t(locale, 'common.cancel')}
        danger
        onConfirm={() => void handleDelete()}
        onCancel={() => setDeleting(null)}
      />
    </div>
  );
}
