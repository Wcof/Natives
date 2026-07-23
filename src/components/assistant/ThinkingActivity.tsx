'use client';

/**
 * Live thinking / tool activity strip for the conversation timeline.
 * Owns its own wall-clock tick so "思考过程 · Ns" stays smooth even when
 * the parent only re-renders on sparse stream events.
 */

import React, { useEffect, useState } from 'react';
import type {
  TimelineThinkingActivity,
  TimelineToolActivity,
} from '@/lib/assistant-timeline';
import { genericThinkingTitle } from '@/lib/assistant-timeline';
import { formatElapsed } from '@/lib/assistant-message-view';

export interface ThinkingActivityProps {
  locale: string;
  thinking?: TimelineThinkingActivity | null;
  tools?: TimelineToolActivity[];
  /**
   * Epoch ms when thinking started. Required for a live elapsed label.
   * Prefer reasoningStartedAt; fall back to run/message start.
   */
  thinkingStartedAtMs?: number | null;
  /**
   * Epoch ms when thinking finished. While thinking.live is true this is
   * ignored so the counter keeps moving until the strip unmounts.
   */
  thinkingFinishedAtMs?: number | null;
}

function statusLabel(status: TimelineToolActivity['status'], zh: boolean): string {
  switch (status) {
    case 'pending':
      return zh ? '等待' : 'pending';
    case 'running':
      return zh ? '执行中' : 'running';
    case 'completed':
      return zh ? '完成' : 'done';
    case 'failed':
      return zh ? '失败' : 'failed';
    case 'rejected':
      return zh ? '拒绝' : 'rejected';
    default:
      return status;
  }
}

export default function ThinkingActivity({
  locale,
  thinking,
  tools = [],
  thinkingStartedAtMs = null,
  thinkingFinishedAtMs = null,
}: ThinkingActivityProps) {
  const zh = locale.startsWith('zh');
  const live = Boolean(thinking?.live);
  const started =
    thinkingStartedAtMs != null && Number.isFinite(thinkingStartedAtMs)
      ? thinkingStartedAtMs
      : null;

  // Local clock: 100ms matches formatElapsed's 0.1s display step.
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!live || started == null) return;
    setNow(Date.now());
    const timer = window.setInterval(() => setNow(Date.now()), 100);
    return () => window.clearInterval(timer);
  }, [live, started]);

  if (!thinking && tools.length === 0) return null;

  let thinkingDurationLabel: string | null = null;
  if (thinking && started != null) {
    const end = live
      ? now
      : thinkingFinishedAtMs != null && Number.isFinite(thinkingFinishedAtMs)
        ? thinkingFinishedAtMs
        : now;
    thinkingDurationLabel = formatElapsed(Math.max(0, end - started));
  }

  return (
    <div
      className="mb-3 space-y-2 rounded-lg border border-[var(--border-subtle)] bg-[var(--surface-hover)]/50 px-3 py-2"
      data-thinking-activity="1"
    >
      {thinking && (
        <div
          className="text-xs text-[var(--text-secondary)]"
          data-thinking-live={thinking.live ? '1' : '0'}
        >
          <div className="mb-1 flex items-center gap-1.5 font-medium">
            {thinking.live && (
              <span
                className="h-1.5 w-1.5 shrink-0 animate-pulse rounded-full bg-[var(--primary)]"
                aria-hidden
              />
            )}
            <span>{genericThinkingTitle(locale)}</span>
            {thinkingDurationLabel ? (
              <span className="tabular-nums text-[var(--text-disabled)]">
                · {thinkingDurationLabel}
              </span>
            ) : null}
          </div>
          {thinking.text ? (
            <div className="max-h-[160px] overflow-y-auto whitespace-pre-wrap italic text-[var(--text-secondary)]">
              {thinking.text}
            </div>
          ) : (
            <div className="text-[var(--text-disabled)]">{zh ? '…' : '…'}</div>
          )}
        </div>
      )}

      {tools.length > 0 && (
        <ul className="space-y-1" data-tool-activity="1">
          {tools.map((tool) => (
            <li
              key={tool.toolCallId}
              className="flex items-center gap-2 font-mono text-[11px] text-[var(--text-secondary)]"
              style={{ paddingLeft: tool.depth * 12 }}
              data-tool-status={tool.status}
              data-tool-depth={tool.depth}
            >
              <span
                className={`h-1.5 w-1.5 shrink-0 rounded-full ${
                  tool.status === 'failed'
                    ? 'bg-[var(--danger)]'
                    : tool.status === 'completed'
                      ? 'bg-[var(--success)]'
                      : tool.status === 'running'
                        ? 'bg-[var(--primary)] animate-pulse'
                        : 'bg-[var(--warning)]'
                }`}
                aria-hidden
              />
              <span className="truncate">{tool.toolName}</span>
              <span className="ml-auto shrink-0 text-[var(--text-disabled)]">
                {statusLabel(tool.status, zh)}
              </span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
