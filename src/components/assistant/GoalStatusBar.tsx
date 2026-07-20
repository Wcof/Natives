'use client';

import { useEffect, useState } from 'react';
import { Loader2, Pause, Play, Trash2 } from 'lucide-react';
import type { Run } from '@/lib/assistant-protocol';
import { isActiveRunStatus, isTerminalRunStatus } from '@/lib/assistant-protocol';

export interface GoalStatusBarProps {
  goalTitle: string;
  instruction?: string | null;
  run: Run | null;
  locale: string;
  tokenLabel?: string;
  /** true when the active run is cancelled/interrupted and can be resumed via retry. */
  canResume?: boolean;
  onPause?: () => void;
  onResume?: () => void;
  onDelete?: () => void;
}

function statusLabel(status: string | undefined, zh: boolean): string {
  if (!status) return zh ? '待命' : 'Idle';
  const map: Record<string, [string, string]> = {
    connecting: ['连接中', 'Connecting'],
    preparing: ['准备中', 'Preparing'],
    reasoning: ['思考中', 'Reasoning'],
    generating: ['生成中', 'Generating'],
    running: ['运行中', 'Running'],
    running_tool: ['执行工具', 'Running tool'],
    waiting_permission: ['等待权限', 'Waiting permission'],
    waiting_user: ['等待回答', 'Waiting for you'],
    waiting_subagent: ['等待子任务', 'Waiting subagent'],
    compacting: ['压缩上下文', 'Compacting'],
    reconnecting: ['重连中', 'Reconnecting'],
    recovering: ['恢复中', 'Recovering'],
    cancelling: ['正在停止', 'Cancelling'],
    completed: ['已完成', 'Completed'],
    failed: ['失败', 'Failed'],
    interrupted: ['已暂停', 'Paused'],
    background_watching: ['后台监视', 'Background'],
    queued: ['排队中', 'Queued'],
  };
  const pair = map[status];
  if (!pair) return status;
  return zh ? pair[0] : pair[1];
}

function formatElapsed(ms: number): string {
  if (!Number.isFinite(ms) || ms < 0) return '0.0s';
  const s = ms / 1000;
  if (s < 60) return `${s.toFixed(1)}s`;
  const m = Math.floor(s / 60);
  const rem = Math.floor(s % 60);
  if (m < 60) return `${m}m ${rem}s`;
  const h = Math.floor(m / 60);
  return `${h}h ${m % 60}m`;
}

function elapsedMs(startedAt?: string | null): number {
  if (!startedAt) return 0;
  const ms = Date.now() - Date.parse(startedAt);
  return Number.isFinite(ms) && ms > 0 ? ms : 0;
}

/**
 * Goal-mode chrome: always visible for goal conversations (not only while a run is active).
 * Shows the goal instruction, live elapsed time, pause / resume / delete actions.
 * Chat/agent modes must not render this surface.
 */
export default function GoalStatusBar({
  goalTitle,
  instruction,
  run,
  locale,
  tokenLabel,
  canResume = false,
  onPause,
  onResume,
  onDelete,
}: GoalStatusBarProps) {
  const zh = locale.startsWith('zh');
  const active = run ? isActiveRunStatus(run.status) : false;
  const terminal = run ? isTerminalRunStatus(run.status) : true;
  const paused =
    run?.status === 'interrupted' || run?.status === 'cancelled' || run?.status === 'cancelling';

  // Tick elapsed while active so the timer feels live.
  const [, setTick] = useState(0);
  useEffect(() => {
    if (!active || !run?.startedAt) return;
    const id = window.setInterval(() => setTick((n) => n + 1), 250);
    return () => window.clearInterval(id);
  }, [active, run?.startedAt, run?.id]);

  const elapsed = run?.startedAt ? formatElapsed(elapsedMs(run.startedAt)) : '';
  const body = (instruction ?? '').trim() || goalTitle;

  return (
    <div
      className="border-t border-[var(--border)] bg-[var(--surface)] px-4 py-2.5"
      role="region"
      aria-label={zh ? 'Goal 任务' : 'Goal task'}
      data-goal-status-bar="1"
    >
      <div className="flex items-start gap-3">
        {active && !terminal ? (
          <Loader2
            size={14}
            className="mt-0.5 shrink-0 animate-spin text-[var(--primary)]"
            aria-hidden
          />
        ) : (
          <span
            className="mt-1 inline-block h-2 w-2 shrink-0 rounded-full bg-[var(--text-disabled)]"
            aria-hidden
          />
        )}
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2 text-xs text-[var(--text-secondary)]">
            <span className="rounded bg-[var(--surface-hover)] px-1.5 py-0.5 font-medium uppercase tracking-wide text-[var(--text-disabled)]">
              Goal
            </span>
            <span className="font-medium text-[var(--text)]">
              {statusLabel(run?.status, zh)}
              {run?.activity ? `：${run.activity}` : ''}
            </span>
            {elapsed && (
              <span className="tabular-nums text-[var(--text-disabled)]" title={zh ? '耗时' : 'Elapsed'}>
                {elapsed}
              </span>
            )}
            {tokenLabel && (
              <span className="text-[var(--text-disabled)]">{tokenLabel}</span>
            )}
          </div>
          <p
            className="mt-1 line-clamp-2 text-sm leading-5 text-[var(--text)]"
            title={body}
          >
            {body}
          </p>
        </div>
        <div className="flex shrink-0 items-center gap-1">
          {active && onPause && (
            <button
              type="button"
              onClick={onPause}
              className="inline-flex items-center gap-1 rounded px-2 py-1 text-xs text-[var(--text-secondary)] hover:bg-[var(--surface-hover)]"
              title={zh ? '暂停' : 'Pause'}
            >
              <Pause size={12} />
              {zh ? '暂停' : 'Pause'}
            </button>
          )}
          {!active && canResume && onResume && (
            <button
              type="button"
              onClick={onResume}
              className="inline-flex items-center gap-1 rounded px-2 py-1 text-xs text-[var(--text-secondary)] hover:bg-[var(--surface-hover)]"
              title={zh ? '继续' : 'Resume'}
            >
              <Play size={12} />
              {zh ? '继续' : 'Resume'}
            </button>
          )}
          {onDelete && (
            <button
              type="button"
              onClick={onDelete}
              className="inline-flex items-center gap-1 rounded px-2 py-1 text-xs text-[var(--danger)] hover:bg-[var(--surface-hover)]"
              title={zh ? '删除 Goal' : 'Delete goal'}
            >
              <Trash2 size={12} />
              {zh ? '删除' : 'Delete'}
            </button>
          )}
        </div>
      </div>
      {paused && (
        <p className="mt-1 pl-5 text-[11px] text-[var(--text-disabled)]">
          {zh ? '任务已暂停，可点「继续」恢复或「删除」结束。' : 'Paused — resume or delete this goal.'}
        </p>
      )}
    </div>
  );
}
