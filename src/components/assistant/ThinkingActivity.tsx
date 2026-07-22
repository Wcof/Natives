'use client';

/**
 * Live thinking / tool activity strip for the conversation timeline.
 * Presentational only — derivation happens in assistant-timeline helpers.
 */

import React from 'react';
import type {
  TimelineThinkingActivity,
  TimelineToolActivity,
} from '@/lib/assistant-timeline';
import { genericThinkingTitle } from '@/lib/assistant-timeline';

export interface ThinkingActivityProps {
  locale: string;
  thinking?: TimelineThinkingActivity | null;
  tools?: TimelineToolActivity[];
  /** Elapsed label for live thinking, e.g. "3.2s". */
  thinkingDurationLabel?: string | null;
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
  thinkingDurationLabel,
}: ThinkingActivityProps) {
  const zh = locale.startsWith('zh');
  if (!thinking && tools.length === 0) return null;

  return (
    <div
      className="mb-3 space-y-2 rounded-lg border border-[var(--border-subtle)] bg-[var(--surface-hover)]/50 px-3 py-2"
      data-thinking-activity="1"
    >
      {thinking && (
        <div className="text-xs text-[var(--text-secondary)]" data-thinking-live={thinking.live ? '1' : '0'}>
          <div className="mb-1 flex items-center gap-1.5 font-medium">
            {thinking.live && (
              <span
                className="h-1.5 w-1.5 shrink-0 animate-pulse rounded-full bg-[var(--primary)]"
                aria-hidden
              />
            )}
            <span>{genericThinkingTitle(locale)}</span>
            {thinkingDurationLabel ? (
              <span className="tabular-nums text-[var(--text-disabled)]">· {thinkingDurationLabel}</span>
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
