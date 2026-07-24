'use client';

/**
 * Live thinking / tool activity strip for the conversation timeline.
 * Owns its own wall-clock tick so "思考过程 · Ns" stays smooth even when
 * the parent only re-renders on sparse stream events.
 */

import React, { useState } from 'react';
import { ChevronDown, Eye, FilePenLine, Terminal, Wrench } from 'lucide-react';
import type {
  TimelineThinkingActivity,
  TimelineToolActivity,
} from '@/lib/assistant-timeline';
import { genericThinkingTitle } from '@/lib/assistant-timeline';
import { formatElapsed } from '@/lib/assistant-message-view';
import DiffViewer from './DiffViewer';

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
  nowMs?: number;
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

function toolDetails(tool: TimelineToolActivity): { kind: 'terminal' | 'edit' | 'view' | 'other'; target: string } {
  const input = tool.input && typeof tool.input === 'object'
    ? tool.input as Record<string, unknown>
    : {};
  const name = tool.toolName.toLowerCase();
  const path = ['path', 'file_path', 'filePath', 'file', 'target', 'filename']
    .map((key) => input[key])
    .find((value): value is string => typeof value === 'string' && value.length > 0);
  const command = ['command', 'cmd', 'script']
    .map((key) => input[key])
    .find((value): value is string => typeof value === 'string' && value.length > 0);
  if (/terminal|shell|bash|exec|command/.test(name)) return { kind: 'terminal', target: command ?? tool.toolName };
  if (/write|patch|edit|replace|delete|remove/.test(name)) return { kind: 'edit', target: path ?? tool.toolName };
  if (/read|view|list|glob|grep|search/.test(name)) return { kind: 'view', target: path ?? String(input.query ?? tool.toolName) };
  return { kind: 'other', target: tool.toolName };
}

function toolTitle(tool: TimelineToolActivity, zh: boolean): string {
  const { kind, target } = toolDetails(tool);
  const running = tool.status === 'pending' || tool.status === 'running';
  if (kind === 'terminal') return zh ? `${running ? '正在运行' : '运行'} ${target}` : `${running ? 'Running' : 'Ran'} ${target}`;
  if (kind === 'edit') return zh ? `${running ? '正在编辑' : '编辑'} ${target}` : `${running ? 'Editing' : 'Edited'} ${target}`;
  if (kind === 'view') return zh ? `${running ? '正在查看' : '查看'} ${target}` : `${running ? 'Viewing' : 'Viewed'} ${target}`;
  return zh ? `${running ? '正在运行' : '执行'} ${target}` : `${running ? 'Running' : 'Ran'} ${target}`;
}

function toolValue(value: unknown): string | null {
  if (value == null) return null;
  if (typeof value === 'string') return value;
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}

export default function ThinkingActivity({
  locale,
  thinking,
  tools = [],
  thinkingStartedAtMs = null,
  thinkingFinishedAtMs = null,
  nowMs = 0,
}: ThinkingActivityProps) {
  const zh = locale.startsWith('zh');
  const live = Boolean(thinking?.live);
  const started =
    thinkingStartedAtMs != null && Number.isFinite(thinkingStartedAtMs)
      ? thinkingStartedAtMs
      : null;

  const [expandedTools, setExpandedTools] = useState<Record<string, boolean>>({});
  if (!thinking && tools.length === 0) return null;

  let thinkingDurationLabel: string | null = null;
  if (thinking && started != null) {
    const end = live
      ? nowMs
      : thinkingFinishedAtMs != null && Number.isFinite(thinkingFinishedAtMs)
        ? thinkingFinishedAtMs
        : nowMs;
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
        <ul className="space-y-1.5" data-tool-activity="1">
          {tools.map((tool) => (
            <li
              key={tool.toolCallId}
              className="overflow-hidden rounded-md border border-[var(--border-subtle)] bg-[var(--surface)]"
              style={{ paddingLeft: tool.depth * 12 }}
              data-tool-status={tool.status}
              data-tool-depth={tool.depth}
            >
              {(() => {
                const detail = toolDetails(tool);
                const Icon = detail.kind === 'terminal' ? Terminal : detail.kind === 'edit' ? FilePenLine : detail.kind === 'view' ? Eye : Wrench;
                const expanded = Boolean(expandedTools[tool.toolCallId]);
                const output = tool.outputText || toolValue(tool.output);
                return (
                  <>
                    <button
                      type="button"
                      className="flex w-full items-center gap-2 px-2.5 py-2 text-left font-mono text-[11px] text-[var(--text-secondary)] hover:bg-[var(--surface-hover)]"
                      onClick={() => setExpandedTools((current) => ({ ...current, [tool.toolCallId]: !expanded }))}
                      aria-expanded={expanded}
                    >
                      <span className="relative grid h-4 w-4 place-items-center" aria-hidden>
                        {(tool.status === 'pending' || tool.status === 'running') && <span className="absolute h-3 w-3 animate-ping rounded-full bg-[var(--primary)]/40" />}
                        <Icon size={13} className="relative" />
                      </span>
                      <span className={`min-w-0 flex-1 truncate ${(tool.status === 'pending' || tool.status === 'running') ? 'animate-pulse text-[var(--text)]' : ''}`}>{toolTitle(tool, zh)}</span>
                      <span className="shrink-0 text-[var(--text-disabled)]">{statusLabel(tool.status, zh)}</span>
                      <ChevronDown size={13} className={`shrink-0 transition-transform ${expanded ? 'rotate-180' : ''}`} />
                    </button>
                    {expanded && (
                      <div className="space-y-2 border-t border-[var(--border-subtle)] px-2.5 py-2">
                        {tool.fileChanges?.map((change) => (
                          <DiffViewer
                            key={change.path}
                            fileName={change.path}
                            oldContent={change.before}
                            newContent={change.after}
                            locale={locale}
                            defaultExpanded
                          />
                        ))}
                        {tool.input !== undefined && (
                          <section>
                            <div className="mb-1 text-[10px] font-medium text-[var(--text-disabled)]">{zh ? '指令' : 'Input'}</div>
                            <pre className="max-h-44 overflow-auto rounded bg-[var(--surface-hover)] p-2 whitespace-pre-wrap break-words text-[11px] text-[var(--text-secondary)]">{toolValue(tool.input)}</pre>
                          </section>
                        )}
                        {output && (
                          <section>
                            <div className="mb-1 text-[10px] font-medium text-[var(--text-disabled)]">{zh ? '输出' : 'Output'}</div>
                            <pre className="max-h-44 overflow-auto rounded bg-[var(--surface-hover)] p-2 whitespace-pre-wrap break-words text-[11px] text-[var(--text-secondary)]">{output}</pre>
                          </section>
                        )}
                      </div>
                    )}
                  </>
                );
              })()}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
