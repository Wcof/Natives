'use client';

import { useEffect, useState } from 'react';
import { Loader2, Pause, Play, Trash2 } from 'lucide-react';
import { t } from '@/i18n';
import type { Run } from '@/lib/assistant-protocol';
import { isActiveRunStatus, isTerminalRunStatus } from '@/lib/assistant-protocol';
import { runStatusLabel, type DisplayRunStatus } from '@/lib/assistant-run-status-labels';

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

// Goal mode presents interruption as pause (paired with the Resume button),
// so it overrides the canonical 已中断 / Interrupted wording.
const GOAL_STATUS_OVERRIDES: Partial<Record<'interrupted' | 'cancelled', string>> = {
  interrupted: 'runStatus.paused',
  cancelled: 'runStatus.paused',
};

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
  const active = run ? isActiveRunStatus(run.status) : false;
  const terminal = run ? isTerminalRunStatus(run.status) : true;
  const paused =
    run?.status === 'interrupted' || run?.status === 'cancelled' || run?.status === 'cancelling';

  // Tick elapsed while active so the timer feels live.
  const [, setTick] = useState(0);
  useEffect(() => {
    if (!active || !run?.startedAt) return;
    const id = window.setInterval(() => setTick((n) => n + 1), 1000);
    return () => window.clearInterval(id);
  }, [active, run?.startedAt, run?.id]);

  const elapsed = run?.startedAt ? formatElapsed(elapsedMs(run.startedAt)) : '';
  const body = (instruction ?? '').trim() || goalTitle;
  const status = run?.status as DisplayRunStatus | null | undefined;

  return (
    <div
      className="border-t border-[var(--border)] bg-[var(--surface)] px-4 py-2.5"
      role="region"
      aria-label={t(locale, 'assistant.goalTask')}
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
              {runStatusLabel(locale, status, GOAL_STATUS_OVERRIDES)}
              {run?.activity ? `：${run.activity}` : ''}
            </span>
            {elapsed && (
              <span className="tabular-nums text-[var(--text-disabled)]" title={t(locale, 'goalBar.elapsed')}>
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
              title={t(locale, 'assistant.goalPause')}
            >
              <Pause size={12} />
              {t(locale, 'assistant.goalPause')}
            </button>
          )}
          {!active && canResume && onResume && (
            <button
              type="button"
              onClick={onResume}
              className="inline-flex items-center gap-1 rounded px-2 py-1 text-xs text-[var(--text-secondary)] hover:bg-[var(--surface-hover)]"
              title={t(locale, 'assistant.goalResume')}
            >
              <Play size={12} />
              {t(locale, 'assistant.goalResume')}
            </button>
          )}
          {onDelete && (
            <button
              type="button"
              onClick={onDelete}
              className="inline-flex items-center gap-1 rounded px-2 py-1 text-xs text-[var(--danger)] hover:bg-[var(--surface-hover)]"
              title={t(locale, 'assistant.goalDelete')}
            >
              <Trash2 size={12} />
              {t(locale, 'common.delete')}
            </button>
          )}
        </div>
      </div>
      {paused && (
        <p className="mt-1 pl-5 text-[11px] text-[var(--text-disabled)]">
          {t(locale, 'assistant.goalPausedHint')}
        </p>
      )}
    </div>
  );
}
