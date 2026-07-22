'use client';

import { Loader2, Square, ArrowDownToLine } from 'lucide-react';
import type { Run } from '@/lib/assistant-protocol';
import { isActiveRunStatus, isTerminalRunStatus } from '@/lib/assistant-protocol';

interface RunStatusBarProps {
  run: Run | null;
  locale: string;
  tokenLabel?: string;
  queueCount?: number;
  connectionHint?: string | null;
  onStop?: () => void;
  onBackground?: () => void;
}

function statusLabel(status: string, zh: boolean): string {
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
    interrupted: ['已中断', 'Interrupted'],
    background_watching: ['后台监视', 'Background'],
    queued: ['排队中', 'Queued'],
  };
  const pair = map[status];
  if (!pair) return status;
  return zh ? pair[0] : pair[1];
}

function elapsed(startedAt?: string | null): string {
  if (!startedAt) return '';
  const ms = Date.now() - Date.parse(startedAt);
  if (!Number.isFinite(ms) || ms < 0) return '';
  const s = ms / 1000;
  return s < 60 ? `${s.toFixed(1)}s` : `${Math.floor(s / 60)}m ${Math.floor(s % 60)}s`;
}

export default function RunStatusBar({
  run,
  locale,
  tokenLabel,
  queueCount = 0,
  connectionHint,
  onStop,
  onBackground,
}: RunStatusBarProps) {
  const zh = locale.startsWith('zh');
  const active = run ? isActiveRunStatus(run.status) : false;
  const terminal = run ? isTerminalRunStatus(run.status) : false;
  // Idle / terminal leftovers must not paint 准备中 / 后台 / 停止.
  if (!connectionHint && (!active || terminal)) return null;

  const status = run?.status ?? 'connecting';
  // Avoid flashing "准备中 0.0s" before the engine has a real start clock.
  const displayStatus =
    status === 'preparing' && !run?.startedAt && !connectionHint ? 'connecting' : status;
  const showSpinner = Boolean(connectionHint) || (active && !terminal);
  const elapsedLabel = run?.startedAt && active && !terminal ? elapsed(run.startedAt) : '';

  return (
    <div
      className="flex items-center gap-3 border-t border-[var(--border)] bg-[var(--surface)] px-4 py-2 text-xs text-[var(--text-secondary)]"
      role="status"
      aria-live="polite"
      aria-atomic="true"
      data-testid="run-status-bar"
      data-run-status={status}
    >
      {showSpinner ? (
        <Loader2 size={14} className="animate-spin text-[var(--primary)] shrink-0" aria-hidden />
      ) : null}
      <span className="font-medium text-[var(--text)]">
        {connectionHint ?? statusLabel(displayStatus, zh)}
        {run?.activity ? `：${run.activity}` : ''}
      </span>
      {elapsedLabel ? (
        <span className="text-[var(--text-disabled)] tabular-nums">{elapsedLabel}</span>
      ) : null}
      {tokenLabel ? <span className="text-[var(--text-disabled)]">{tokenLabel}</span> : null}
      {queueCount > 0 ? (
        <span className="rounded bg-[var(--surface-hover)] px-1.5 py-0.5">
          {zh ? `队列 ${queueCount}` : `Queue ${queueCount}`}
        </span>
      ) : null}
      <div className="ml-auto flex items-center gap-1">
        {active && !terminal && onBackground ? (
          <button
            type="button"
            onClick={onBackground}
            className="inline-flex items-center gap-1 rounded px-2 py-1 hover:bg-[var(--surface-hover)]"
            title={zh ? '转后台' : 'Background'}
          >
            <ArrowDownToLine size={12} />
            {zh ? '后台' : 'Background'}
          </button>
        ) : null}
        {active && !terminal && onStop ? (
          <button
            type="button"
            onClick={onStop}
            className="inline-flex items-center gap-1 rounded px-2 py-1 text-[var(--danger)] hover:bg-[var(--surface-hover)]"
            title={zh ? '停止 (⌘.)' : 'Stop (⌘.)'}
          >
            <Square size={12} />
            {zh ? '停止' : 'Stop'}
          </button>
        ) : null}
      </div>
    </div>
  );
}
