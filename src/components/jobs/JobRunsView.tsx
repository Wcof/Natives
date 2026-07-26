'use client';

/**
 * JobRunsView — 任务运行历史（task_runs，契约第 2 节状态机全集展示）
 *
 * 数据只来自 job_runs_list 的真实 run 行——不模拟、不推断进度（R-F2）。
 * 手动刷新 + 加载更多分页；不做后台轮询（R-E11）。
 */

import { Fragment, useCallback, useEffect, useState } from 'react';
import { RefreshCw, History, X } from 'lucide-react';
import { t, useLocale } from '@/i18n';
import { EmptyState, ErrorState, LoadingState } from '@/components/ui/EmptyState';
import JobStatusBadge from './JobStatusBadge';
import { jobRunsList, type JobRun } from '@/lib/jobs-api';
import { runStatusTone, gmtOffsetLabel } from '@/lib/jobs-view';
import { fmtDateTime } from '@/lib/format';

const PAGE_SIZE = 50;

interface JobRunsViewProps {
  /** 仅看某个任务的历史；null = 全部 */
  jobFilter: { id: string; name: string } | null;
  onClearFilter: () => void;
  /** job id → 名称（列表页数据），用于「任务」列展示 */
  jobNames: ReadonlyMap<string, string>;
}

function runJobId(run: JobRun): string | null {
  return run.task_id ?? run.job_id ?? null;
}

function fmtIso(iso: string | null | undefined): string {
  if (!iso) return '—';
  const ms = Date.parse(iso);
  return Number.isFinite(ms) ? fmtDateTime(ms) : iso;
}

const CELL: React.CSSProperties = {
  padding: '6px 10px',
  fontSize: '0.8125rem',
  color: 'var(--text-secondary)',
  borderBottom: '1px solid var(--border-subtle)',
  verticalAlign: 'top',
  whiteSpace: 'nowrap',
};

export default function JobRunsView({ jobFilter, onClearFilter, jobNames }: JobRunsViewProps) {
  const locale = useLocale();
  const [runs, setRuns] = useState<JobRun[]>([]);
  const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [loadMoreError, setLoadMoreError] = useState<string | null>(null);
  const [expandedId, setExpandedId] = useState<string | null>(null);

  const load = useCallback(
    async (offset: number) => {
      const result = await jobRunsList({
        job_id: jobFilter?.id,
        limit: PAGE_SIZE,
        offset,
      });
      setTotal(result.total);
      // 追加时按 id 去重：分页期间产生的新 run 会让 offset 漂移出重复行
      setRuns((prev) => {
        if (offset === 0) return result.runs;
        const known = new Set(prev.map((r) => r.id));
        return [...prev, ...result.runs.filter((r) => !known.has(r.id))];
      });
    },
    [jobFilter?.id],
  );

  const reload = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      await load(0);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }, [load]);

  // 首个语句即 await，effect 内不产生同步 setState；筛选变化由父级 key 触发重挂载
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        await load(0);
      } catch (err) {
        if (!cancelled) setError(err instanceof Error ? err.message : String(err));
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [load]);

  const handleLoadMore = async () => {
    setLoadingMore(true);
    setLoadMoreError(null);
    try {
      await load(runs.length);
    } catch (err) {
      // 分页失败只提示在表尾，保留已加载数据（不整表替换为 ErrorState）
      setLoadMoreError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoadingMore(false);
    }
  };

  const jobName = (run: JobRun): string => {
    const id = runJobId(run);
    if (jobFilter) return jobFilter.name;
    if (id && jobNames.has(id)) return jobNames.get(id)!;
    return t(locale, 'jobs.runs.unknownJob');
  };

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 10, minHeight: 0 }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
        {jobFilter && (
          <span
            style={{
              display: 'inline-flex',
              alignItems: 'center',
              gap: 6,
              padding: '2px 10px',
              borderRadius: 999,
              border: '1px solid var(--border)',
              fontSize: '0.75rem',
              color: 'var(--text-secondary)',
            }}
          >
            {t(locale, 'jobs.runs.filteredBy', { name: jobFilter.name })}
            <button
              type="button"
              onClick={onClearFilter}
              aria-label={t(locale, 'jobs.runs.clearFilter')}
              title={t(locale, 'jobs.runs.clearFilter')}
              style={{ display: 'inline-flex', background: 'none', border: 'none', cursor: 'pointer', color: 'var(--text-disabled)', padding: 0 }}
            >
              <X size={12} />
            </button>
          </span>
        )}
        <span style={{ fontSize: '0.75rem', color: 'var(--text-disabled)' }}>
          {t(locale, 'jobs.runs.total', { total })}
        </span>
        <span style={{ fontSize: '0.6875rem', color: 'var(--text-disabled)' }}>
          {t(locale, 'jobs.form.timezoneHint', { offset: gmtOffsetLabel() })}
        </span>
        <div style={{ flex: 1 }} />
        <button
          type="button"
          className="btn-ghost"
          onClick={() => void reload()}
          disabled={loading}
          style={{ display: 'inline-flex', alignItems: 'center', gap: 6, fontSize: '0.8125rem' }}
        >
          <RefreshCw size={13} />
          {t(locale, 'common.refresh')}
        </button>
      </div>

      {loading ? (
        <LoadingState message={t(locale, 'common.loading')} />
      ) : error ? (
        <ErrorState message={`${t(locale, 'jobs.runs.loadFailed')} — ${error}`} onRetry={() => void reload()} />
      ) : runs.length === 0 ? (
        <EmptyState
          icon={<History size={32} style={{ color: 'var(--text-disabled)' }} />}
          title={t(locale, 'jobs.runs.empty')}
          description={t(locale, 'jobs.runs.emptyDescription')}
        />
      ) : (
        <div style={{ overflow: 'auto', minHeight: 0 }}>
          <table style={{ width: '100%', borderCollapse: 'collapse' }}>
            <thead>
              <tr>
                {(['colJob', 'colStatus', 'colTrigger', 'colStarted', 'colFinished', 'colError'] as const).map(
                  (key) => (
                    <th
                      key={key}
                      style={{
                        ...CELL,
                        textAlign: 'left',
                        fontSize: '0.6875rem',
                        fontWeight: 600,
                        textTransform: 'uppercase',
                        letterSpacing: '0.06em',
                        color: 'var(--text-disabled)',
                      }}
                    >
                      {t(locale, `jobs.runs.${key}`)}
                    </th>
                  ),
                )}
              </tr>
            </thead>
            <tbody>
              {runs.map((run) => {
                const status = String(run.status ?? '');
                const statusLabel =
                  t(locale, `jobs.runStatus.${status}`) === `jobs.runStatus.${status}`
                    ? status
                    : t(locale, `jobs.runStatus.${status}`);
                const trigger = run.trigger ? String(run.trigger) : null;
                const triggerLabel = trigger
                  ? t(locale, `jobs.trigger.${trigger}`) === `jobs.trigger.${trigger}`
                    ? trigger
                    : t(locale, `jobs.trigger.${trigger}`)
                  : '—';
                const errorText = run.error_code ?? run.error ?? null;
                const expanded = expandedId === run.id;
                const detailRows: Array<[string, string | null | undefined]> = [
                  ['detailError', run.error],
                  ['detailDetail', run.detail],
                  ['detailResult', run.result_summary],
                  ['detailRunId', run.run_id],
                  ['detailConversation', run.conversation_id],
                ];
                return (
                  <Fragment key={run.id}>
                    <tr
                      onClick={() => setExpandedId(expanded ? null : run.id)}
                      onKeyDown={(e) => {
                        if (e.key === 'Enter' || e.key === ' ') {
                          e.preventDefault();
                          setExpandedId(expanded ? null : run.id);
                        }
                      }}
                      tabIndex={0}
                      aria-expanded={expanded}
                      style={{ cursor: 'pointer' }}
                    >
                      <td style={{ ...CELL, color: 'var(--text)', maxWidth: 180, overflow: 'hidden', textOverflow: 'ellipsis' }}>
                        {jobName(run)}
                      </td>
                      <td style={CELL}>
                        <JobStatusBadge
                          tone={runStatusTone(status)}
                          label={statusLabel}
                          title={run.detail ?? undefined}
                        />
                      </td>
                      <td style={CELL}>{triggerLabel}</td>
                      <td style={CELL}>{fmtIso(run.started_at)}</td>
                      <td style={CELL}>{fmtIso(run.finished_at)}</td>
                      <td style={{ ...CELL, color: errorText ? 'var(--danger)' : 'var(--text-disabled)', maxWidth: 220, overflow: 'hidden', textOverflow: 'ellipsis' }} title={errorText ?? undefined}>
                        {errorText ?? '—'}
                      </td>
                    </tr>
                    {expanded && (
                      <tr>
                        <td colSpan={6} style={{ ...CELL, whiteSpace: 'normal', background: 'var(--surface)' }}>
                          <dl style={{ display: 'flex', flexDirection: 'column', gap: 6, margin: 0, fontSize: '0.75rem' }}>
                            {detailRows.map(([key, value]) =>
                              value ? (
                                <div key={key} style={{ display: 'flex', gap: 8 }}>
                                  <dt style={{ flexShrink: 0, fontWeight: 600, color: 'var(--text-secondary)' }}>
                                    {t(locale, `jobs.runs.${key}`)}
                                  </dt>
                                  <dd style={{ margin: 0, color: 'var(--text)', overflowWrap: 'anywhere', whiteSpace: 'pre-wrap', maxHeight: 200, overflow: 'auto' }}>
                                    {value}
                                  </dd>
                                </div>
                              ) : null,
                            )}
                            {detailRows.every(([, value]) => !value) && (
                              <span style={{ color: 'var(--text-disabled)' }}>{t(locale, 'jobs.runs.detailEmpty')}</span>
                            )}
                          </dl>
                        </td>
                      </tr>
                    )}
                  </Fragment>
                );
              })}
            </tbody>
          </table>
          {loadMoreError && (
            <div role="alert" style={{ padding: '8px 10px', fontSize: '0.75rem', color: 'var(--danger)' }}>
              {t(locale, 'jobs.runs.loadFailed')} — {loadMoreError}
            </div>
          )}
          {runs.length < total && (
            <div style={{ display: 'flex', justifyContent: 'center', padding: 10 }}>
              <button
                type="button"
                className="btn"
                onClick={() => void handleLoadMore()}
                disabled={loadingMore}
                style={{ fontSize: '0.8125rem' }}
              >
                {loadingMore ? t(locale, 'common.loading') : t(locale, 'jobs.runs.loadMore')}
              </button>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
