'use client';

/**
 * Live thinking / tool activity strip for the conversation timeline.
 * Owns its own wall-clock tick so "思考过程 · Ns" stays smooth even when
 * the parent only re-renders on sparse stream events.
 */

import React, { useEffect, useRef, useState } from 'react';
import { ChevronDown, Eye, FilePenLine, Terminal, Wrench } from 'lucide-react';
import type {
  TimelineThinkingActivity,
  TimelineToolActivity,
} from '@/lib/assistant-timeline';
import { t } from '@/i18n';
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

const TOOL_STATUS_KEYS: Record<TimelineToolActivity['status'], string> = {
  pending: 'thinkingActivity.toolPending',
  running: 'thinkingActivity.toolRunning',
  completed: 'thinkingActivity.toolDone',
  failed: 'thinkingActivity.toolFailed',
  rejected: 'thinkingActivity.toolRejected',
};

function statusLabel(status: TimelineToolActivity['status'], locale: string): string {
  const key = TOOL_STATUS_KEYS[status];
  return key ? t(locale, key) : status;
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

function toolTitle(tool: TimelineToolActivity, locale: string): string {
  const { kind, target } = toolDetails(tool);
  const running = tool.status === 'pending' || tool.status === 'running';
  const keys: Record<'terminal' | 'edit' | 'view' | 'other', { running: string; done: string }> = {
    terminal: { running: 'thinkingActivity.toolTerminalRunning', done: 'thinkingActivity.toolTerminalDone' },
    edit: { running: 'thinkingActivity.toolEditRunning', done: 'thinkingActivity.toolEditDone' },
    view: { running: 'thinkingActivity.toolViewRunning', done: 'thinkingActivity.toolViewDone' },
    other: { running: 'thinkingActivity.toolOtherRunning', done: 'thinkingActivity.toolOtherDone' },
  };
  const pair = keys[kind];
  return t(locale, running ? pair.running : pair.done, { target });
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
  const live = Boolean(thinking?.live || tools.some(t => t.status === 'pending' || t.status === 'running'));
  const started =
    thinkingStartedAtMs != null && Number.isFinite(thinkingStartedAtMs)
      ? thinkingStartedAtMs
      : null;

  // Default: expanded while live/running, collapsed when done.
  const [timelineOpen, setTimelineOpen] = useState(live);
  const userToggledRef = useRef(false);

  useEffect(() => {
    if (userToggledRef.current) return;
    setTimelineOpen(live);
  }, [live]);

  const [expandedTools, setExpandedTools] = useState<Record<string, boolean>>({});
  const [openTaxonomies, setOpenTaxonomies] = useState<Record<string, boolean>>({ view: false, edit: false, terminal: false, other: false });
  const thinkingTextRef = useRef<HTMLDivElement>(null);
  const isUserScrolledUpRef = useRef(false);

  const handleThinkingScroll = (e: React.UIEvent<HTMLDivElement>) => {
    const el = e.currentTarget;
    const isNearBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 20;
    isUserScrolledUpRef.current = !isNearBottom;
  };

  useEffect(() => {
    if (live && thinkingTextRef.current && !isUserScrolledUpRef.current) {
      thinkingTextRef.current.scrollTop = thinkingTextRef.current.scrollHeight;
    }
  }, [live, thinking?.text]);

  if (!thinking && tools.length === 0) return null;

  let durationLabel: string | null = null;
  if (started != null) {
    const end = live
      ? nowMs
      : thinkingFinishedAtMs != null && Number.isFinite(thinkingFinishedAtMs)
        ? thinkingFinishedAtMs
        : nowMs;
    const diff = Math.max(0, end - started);
    if (diff >= 100) {
      durationLabel = formatElapsed(diff);
    }
  }

  // Bound tools to max 200 items for performance
  const displayTools = tools.slice(0, 200);

  // Group tools by taxonomy for multi-directive structure
  const taxonomyGroups = (() => {
    const map: Record<'view' | 'edit' | 'terminal' | 'other', TimelineToolActivity[]> = {
      view: [],
      edit: [],
      terminal: [],
      other: [],
    };
    for (const tool of displayTools) {
      const { kind } = toolDetails(tool);
      map[kind].push(tool);
    }
    return map;
  })();

  const toggleTaxonomy = (key: string) => {
    setOpenTaxonomies((prev) => ({ ...prev, [key]: !prev[key] }));
  };

  const taxonomyKeys: Record<string, { labelKey: string; icon: typeof Eye }> = {
    view: { labelKey: 'thinkingActivity.taxonomyView', icon: Eye },
    edit: { labelKey: 'thinkingActivity.taxonomyEdit', icon: FilePenLine },
    terminal: { labelKey: 'thinkingActivity.taxonomyTerminal', icon: Terminal },
    other: { labelKey: 'thinkingActivity.taxonomyOther', icon: Wrench },
  };

  return (
    <div
      className="mb-3 space-y-2 rounded-lg border border-[var(--border-subtle)] bg-[var(--surface-hover)]/50 px-3 py-2"
      data-thinking-activity="1"
    >
      {/* "已执行" Timeline Header */}
      <button
        type="button"
        onClick={() => {
          userToggledRef.current = true;
          setTimelineOpen(open => !open);
        }}
        className="flex w-full items-center gap-1.5 text-xs text-[var(--text-secondary)] font-medium hover:text-[var(--text)] transition-colors"
        aria-expanded={timelineOpen}
      >
        {live && (
          <span
            className="h-1.5 w-1.5 shrink-0 animate-pulse rounded-full bg-[var(--primary)]"
            aria-hidden
          />
        )}
        <span>{live ? t(locale, 'thinkingActivity.running') : t(locale, 'thinkingActivity.completed')}</span>
        <span>{t(locale, 'thinkingActivity.executed')}</span>
        {durationLabel ? (
          <span className="tabular-nums text-[var(--text-disabled)]">
            {durationLabel}
          </span>
        ) : null}
        <ChevronDown
          size={13}
          className={`ml-auto text-[var(--text-disabled)] transition-transform ${timelineOpen ? 'rotate-180' : ''}`}
        />
      </button>

      {/* Expanded Body */}
      {timelineOpen && (
        <div className="space-y-2 pt-1">
          {thinking && (
            <div
              className="text-xs text-[var(--text-secondary)]"
              data-thinking-live={thinking.live ? '1' : '0'}
            >
              {thinking.live && thinking.text ? (
                <div
                  ref={thinkingTextRef}
                  onScroll={handleThinkingScroll}
                  className="max-h-[160px] overflow-y-auto whitespace-pre-wrap italic text-[var(--text-secondary)]"
                >
                  {thinking.text}
                </div>
              ) : !thinking.live ? (
                <div className="space-y-1">
                  {thinking.summary ? (
                    <div className="font-medium text-[var(--text)]">{thinking.summary}</div>
                  ) : thinking.text ? (
                    <div className="line-clamp-2 text-[var(--text-secondary)]">{thinking.text}</div>
                  ) : (
                    <div className="text-[var(--text-disabled)]">{t(locale, 'thinkingActivity.thinkingComplete')}</div>
                  )}
                  {thinking.summaryStatus === 'failed' && (
                    <span className="text-[10px] text-[var(--warning)]">{t(locale, 'thinkingActivity.summaryFailed')}</span>
                  )}
                </div>
              ) : (
                <div className="text-[var(--text-disabled)]">{'…'}</div>
              )}
            </div>
          )}

          {displayTools.length > 0 && (
            <div className="space-y-2" data-tool-activity="1">
              {/* Taxonomy Layer for Multi-Directives */}
              {displayTools.length > 1 ? (
                <div className="space-y-1.5">
                  {(['view', 'edit', 'terminal', 'other'] as const).map((kind) => {
                    const group = taxonomyGroups[kind];
                    if (group.length === 0) return null;
                    const meta = taxonomyKeys[kind] ?? { labelKey: 'thinkingActivity.taxonomyOther', icon: Wrench };
                    const Icon = meta.icon;
                    const isOpen = Boolean(openTaxonomies[kind]);
                    return (
                      <div key={kind} className="rounded-md border border-[var(--border-subtle)] bg-[var(--surface)]">
                        <button
                          type="button"
                          onClick={() => toggleTaxonomy(kind)}
                          className="flex w-full items-center gap-2 px-2.5 py-1.5 text-left text-xs font-medium text-[var(--text-secondary)] hover:bg-[var(--surface-hover)]"
                        >
                          <Icon size={13} />
                          <span className="flex-1">{t(locale, meta.labelKey)}</span>
                          <span className="rounded-full bg-[var(--surface-hover)] px-1.5 py-0.5 text-[10px] text-[var(--text-disabled)]">
                            {group.length}
                          </span>
                          <ChevronDown size={13} className={`transition-transform ${isOpen ? 'rotate-180' : ''}`} />
                        </button>
                        {isOpen && (
                          <ul className="space-y-1 p-1 border-t border-[var(--border-subtle)]">
                            {group.map((tool) => (
                              <RenderToolItem
                                key={tool.toolCallId}
                                tool={tool}
                                locale={locale}
                                expanded={Boolean(expandedTools[tool.toolCallId])}
                                onToggle={() =>
                                  setExpandedTools((current) => ({
                                    ...current,
                                    [tool.toolCallId]: !current[tool.toolCallId],
                                  }))
                                }
                              />
                            ))}
                          </ul>
                        )}
                      </div>
                    );
                  })}
                </div>
              ) : (
                <ul className="space-y-1.5">
                  {displayTools.map((tool) => (
                    <RenderToolItem
                      key={tool.toolCallId}
                      tool={tool}
                      locale={locale}
                      expanded={Boolean(expandedTools[tool.toolCallId])}
                      onToggle={() =>
                        setExpandedTools((current) => ({
                          ...current,
                          [tool.toolCallId]: !current[tool.toolCallId],
                        }))
                      }
                    />
                  ))}
                </ul>
              )}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function RenderToolItem({
  tool,
  locale,
  expanded,
  onToggle,
}: {
  tool: TimelineToolActivity;
  locale: string;
  expanded: boolean;
  onToggle: () => void;
}) {
  const detail = toolDetails(tool);
  const Icon = detail.kind === 'terminal' ? Terminal : detail.kind === 'edit' ? FilePenLine : detail.kind === 'view' ? Eye : Wrench;
  const output = tool.outputText || toolValue(tool.output);
  return (
    <li
      className="overflow-hidden rounded-md border border-[var(--border-subtle)] bg-[var(--surface)]"
      style={{ paddingLeft: tool.depth * 12 }}
      data-tool-status={tool.status}
      data-tool-depth={tool.depth}
    >
      <button
        type="button"
        className="flex w-full items-center gap-2 px-2.5 py-1.5 text-left font-mono text-[11px] text-[var(--text-secondary)] hover:bg-[var(--surface-hover)]"
        onClick={onToggle}
        aria-expanded={expanded}
      >
        <span className="relative grid h-4 w-4 place-items-center" aria-hidden>
          {(tool.status === 'pending' || tool.status === 'running') && (
            <span className="absolute h-3 w-3 animate-ping rounded-full bg-[var(--primary)]/40" />
          )}
          <Icon size={13} className="relative" />
        </span>
        <span className={`min-w-0 flex-1 truncate ${(tool.status === 'pending' || tool.status === 'running') ? 'animate-pulse text-[var(--text)]' : ''}`}>
          {toolTitle(tool, locale)}
        </span>
        <span className="shrink-0 text-[var(--text-disabled)]">{statusLabel(tool.status, locale)}</span>
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
              <div className="mb-1 text-[10px] font-medium text-[var(--text-disabled)]">{t(locale, 'thinkingActivity.inputLabel')}</div>
              <pre className="max-h-44 overflow-auto rounded bg-[var(--surface-hover)] p-2 whitespace-pre-wrap break-words text-[11px] text-[var(--text-secondary)]">{toolValue(tool.input)}</pre>
            </section>
          )}
          {output && (
            <section>
              <div className="mb-1 text-[10px] font-medium text-[var(--text-disabled)]">{t(locale, 'thinkingActivity.outputLabel')}</div>
              <pre className="max-h-44 overflow-auto rounded bg-[var(--surface-hover)] p-2 whitespace-pre-wrap break-words text-[11px] text-[var(--text-secondary)]">{output}</pre>
            </section>
          )}
        </div>
      )}
    </li>
  );
}
