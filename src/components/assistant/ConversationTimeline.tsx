'use client';

import { memo, useEffect, useMemo, useRef, useState } from 'react';
import { ArrowDown, Check, Copy, FileDiff, RefreshCw, Undo2 } from 'lucide-react';
import { formatRunElapsed, messagePlainText } from '@/lib/assistant-message-view';
import type { RunEvent } from '@/lib/assistant-protocol';
import {
  deriveToolActivityFromEvents,
  extractThinking,
  extractLiveThinking,
  filterTimelineBodyBlocks,
  summarizeConversationChanges,
} from '@/lib/assistant-timeline';
import { renderBlocks, type ContentBlock } from './blocks';
import ThinkingActivity from './ThinkingActivity';
import type { FileChange } from '@/lib/assistant-protocol';

export interface Message {
  id: string;
  role: 'system' | 'user' | 'assistant';
  contentBlocks: ContentBlock[];
  status: string;
  createdAt: string;
  /** Owning run for live activity + duration. */
  runId?: string | null;
  inputTokens?: number;
  outputTokens?: number;
  startedAt?: string;
  finishedAt?: string;
  reasoningStartedAt?: string | null;
  reasoningFinishedAt?: string | null;
}

interface ConversationTimelineProps {
  messages: Message[];
  loading: boolean;
  locale: string;
  onRetry?: () => void;
  /** Full event streams keyed by run id (for live tool nesting). */
  eventsByRun?: Record<string, RunEvent[]>;
  /** Visible surface events/files: root includes child agents; child is self-only. */
  changeEvents?: RunEvent[];
  fileChanges?: FileChange[];
  onRollbackChanges?: (changes: Array<{ path: string; runId?: string }>) => Promise<boolean>;
  hasMoreOlder?: boolean;
  loadingOlder?: boolean;
  onLoadOlder?: () => void | Promise<void>;
}

const NEAR_BOTTOM_PX = 80;

// ── Coarse render window (R-P4: never mount the full history) ──────────────
// Only the newest TIMELINE_WINDOW_SIZE rows mount; older rows are revealed in
// TIMELINE_WINDOW_STEP increments via the "show earlier" button. Host paging
// (hasMoreOlder/onLoadOlder) only engages once every local row is revealed.

/** Max message rows mounted before the user asks for more. */
export const TIMELINE_WINDOW_SIZE = 150;
/** Rows revealed per "show earlier" click. */
export const TIMELINE_WINDOW_STEP = 150;

/** First rendered index for a tail-anchored window. Pure — unit tested. */
export function timelineWindowStart(
  total: number,
  revealedOlder: number,
  windowSize: number = TIMELINE_WINDOW_SIZE,
): number {
  if (windowSize <= 0) return 0;
  return Math.max(0, total - windowSize - Math.max(0, revealedOlder));
}

export interface TimelineEdgeIds {
  length: number;
  firstId: string | null;
  lastId: string | null;
}

/**
 * How the messages array changed between renders. Pure — unit tested.
 * - 'reset':   conversation switched / cleared → forget local reveals.
 * - 'prepend': older page arrived at the top → grow window so the rows the
 *              user just requested do not vanish into the hidden range.
 * - 'keep':    append / in-place streaming update → window stays tail-anchored.
 */
export function classifyTimelineDelta(
  prev: TimelineEdgeIds | null,
  next: TimelineEdgeIds,
): 'reset' | 'prepend' | 'keep' {
  if (!prev) return 'keep';
  if (next.length === 0) return 'reset';
  if (prev.length === 0) return 'reset';
  if (next.length > prev.length && next.lastId === prev.lastId && next.firstId !== prev.firstId) {
    return 'prepend';
  }
  if (next.firstId !== prev.firstId && next.lastId !== prev.lastId) return 'reset';
  return 'keep';
}

/** Shared empty run events so MessageRow memo is not busted by `?? []` each render. */
const EMPTY_RUN_EVENTS: RunEvent[] = [];
/** Idle tool strip — avoid allocating `[]` on every finished MessageRow render. */
const EMPTY_TOOL_ACTIVITY: ReturnType<typeof deriveToolActivityFromEvents> = [];

function ChangeSummaryCard({
  locale,
  events,
  fileChanges,
  onRollbackChanges,
}: {
  locale: string;
  events: RunEvent[];
  fileChanges: FileChange[];
  onRollbackChanges?: (changes: Array<{ path: string; runId?: string }>) => Promise<boolean>;
}) {
  const zh = locale.startsWith('zh');
  const summary = useMemo(
    () => summarizeConversationChanges(events, fileChanges),
    [events, fileChanges],
  );
  const [pinnedOpen, setPinnedOpen] = useState(false);
  const [hovered, setHovered] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const [rollingBack, setRollingBack] = useState(false);
  const [rolledBack, setRolledBack] = useState(false);
  if (summary.files.length === 0) return null;
  const expanded = pinnedOpen || hovered;
  const restore = async () => {
    if (!onRollbackChanges || rollingBack) return;
    setRollingBack(true);
    try {
      const ok = await onRollbackChanges(summary.files.map(({ path, runId }) => ({ path, runId })));
      if (ok) setRolledBack(true);
    } finally {
      setRollingBack(false);
      setConfirming(false);
    }
  };

  return (
    <section className="overflow-hidden rounded-2xl border border-[var(--border)] bg-[var(--surface)]" data-change-summary onMouseEnter={() => setHovered(true)} onMouseLeave={() => setHovered(false)}>
      <div role="button" tabIndex={0} aria-expanded={expanded} onClick={() => setPinnedOpen(open => !open)} onKeyDown={(event) => { if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); setPinnedOpen(open => !open); } }} className="flex w-full items-center gap-3 px-4 py-3 text-left hover:bg-[var(--surface-hover)]">
        <div className="grid h-10 w-10 shrink-0 place-items-center rounded-xl bg-[var(--surface-hover)] text-[var(--text-secondary)]"><FileDiff size={20} /></div>
        <div className="min-w-0 flex-1">
          <div className="text-sm font-semibold text-[var(--text)]">{zh ? `已编辑 ${summary.files.length} 个文件` : `Edited ${summary.files.length} files`}</div>
          <div className="mt-0.5 text-sm font-medium tabular-nums"><span className="text-emerald-500">+{summary.additions}</span><span className="ml-2 text-red-500">−{summary.deletions}</span></div>
        </div>
        {onRollbackChanges && !rolledBack && (
          confirming ? (
            <div className="flex shrink-0 items-center gap-1">
              <button type="button" onClick={(event) => { event.stopPropagation(); setConfirming(false); }} disabled={rollingBack} className="rounded-lg px-2 py-1.5 text-xs text-[var(--text-secondary)] hover:bg-[var(--surface-hover)]">{zh ? '取消' : 'Cancel'}</button>
              <button type="button" onClick={(event) => { event.stopPropagation(); void restore(); }} disabled={rollingBack} className="rounded-lg border border-[var(--danger)]/40 px-2 py-1.5 text-xs font-medium text-[var(--danger)] hover:bg-red-500/10 disabled:opacity-50">{rollingBack ? (zh ? '撤销中…' : 'Undoing…') : (zh ? '确认撤销' : 'Confirm undo')}</button>
            </div>
          ) : (
            <button type="button" onClick={(event) => { event.stopPropagation(); setConfirming(true); }} className="flex shrink-0 items-center gap-1 rounded-lg px-2 py-1.5 text-sm font-medium text-[var(--text-secondary)] hover:bg-[var(--surface-hover)]"><Undo2 size={15} />{zh ? '撤销' : 'Undo'}</button>
          )
        )}
        {rolledBack && <span className="shrink-0 text-xs text-[var(--text-disabled)]">{zh ? '已撤销' : 'Undone'}</span>}
      </div>
      {expanded && <div className="border-t border-[var(--border-subtle)] px-4 py-1">
        {summary.files.map((file) => (
          <div key={file.path} className="flex items-center gap-3 py-2 text-sm">
            <span className="min-w-0 flex-1 truncate text-[var(--text-secondary)]">{file.path}</span>
            <span className="shrink-0 tabular-nums text-emerald-500">+{file.additions}</span>
            <span className="shrink-0 tabular-nums text-red-500">−{file.deletions}</span>
          </div>
        ))}
      </div>}
    </section>
  );
}

const MessageRow = memo(function MessageRow({
  message,
  locale,
  zh,
  now,
  isLastRetryable,
  onRetry,
  copiedId,
  onCopy,
  runEvents,
}: {
  message: Message;
  locale: string;
  zh: boolean;
  /** Wall clock; only meaningful while this message is live. */
  now: number;
  isLastRetryable: boolean;
  onRetry?: () => void;
  copiedId: string | null;
  onCopy: (id: string, text: string) => void;
  runEvents: RunEvent[];
}) {
  const user = message.role === 'user';
  const messageLive = message.status === 'streaming' || message.status === 'running';
  const start = message.startedAt
    ? Date.parse(message.startedAt)
    : message.createdAt
      ? Date.parse(message.createdAt)
      : Number.NaN;
  // Footer total time: wall clock only while the message is active.
  const end = message.finishedAt
    ? Date.parse(message.finishedAt)
    : messageLive
      ? now || start
      : Number.isFinite(start)
        ? start
        : now;
  const duration = Number.isFinite(start) ? formatRunElapsed(end - start) : null;
  const tokens = (message.inputTokens ?? 0) + (message.outputTokens ?? 0);

  // Prefer dedicated reasoning clock; fall back to run/message start so the
  // "思考过程 · Ns" label never stalls at 0.7s waiting for the first delta.
  const reasoningStart = (() => {
    if (message.reasoningStartedAt) {
      const t = Date.parse(message.reasoningStartedAt);
      if (Number.isFinite(t)) return t;
    }
    if (Number.isFinite(start)) return start;
    return Number.NaN;
  })();
  const reasoningFinished = message.reasoningFinishedAt
    ? Date.parse(message.reasoningFinishedAt)
    : Number.NaN;

  const liveThinking = useMemo(
    () => (messageLive ? extractLiveThinking(message.contentBlocks) : null),
    [message.contentBlocks, messageLive],
  );
  const thinking = useMemo(
    () => liveThinking ?? extractThinking(message.contentBlocks),
    [liveThinking, message.contentBlocks],
  );

  const reasoningFinishedKnown = Number.isFinite(reasoningFinished);
  // Only finished reasoning without finishedAt needs the parent clock.
  const bodyDurationClock = !messageLive && !reasoningFinishedKnown ? now : reasoningFinished;
  // Live reasoning is stripped by filterTimelineBodyBlocks and rendered in
  // ThinkingActivity (own 100ms clock). Do not rebuild body blocks every 100ms.
  const bodyBlocks = useMemo(() => {
    const prepared = message.contentBlocks.map((block) => {
      if (block.type !== 'reasoning') return block;
      const blockLive = messageLive && Boolean(block.live ?? true);
      const withLocale = {
        ...block,
        locale: locale.startsWith('zh') ? 'zh' : 'en',
        live: blockLive,
      };
      if (blockLive || !Number.isFinite(reasoningStart)) return withLocale;
      const endMs = Number.isFinite(bodyDurationClock) ? bodyDurationClock : 0;
      return {
        ...withLocale,
        durationMs: Math.max(0, endMs - reasoningStart),
      };
    });
    return filterTimelineBodyBlocks(prepared);
  }, [
    message.contentBlocks,
    locale,
    messageLive,
    reasoningStart,
    bodyDurationClock,
  ]);

  const toolActivity = useMemo(() => {
    if (!messageLive) return EMPTY_TOOL_ACTIVITY;
    return deriveToolActivityFromEvents(runEvents);
  }, [messageLive, runEvents]);

  const hasReasoning = bodyBlocks.some((block) => block.type === 'reasoning');

  return (
    <article className={user ? 'ml-auto max-w-[78%]' : 'mr-auto w-full max-w-[760px]'}>
      <div
        className={
          user
            ? 'rounded-2xl rounded-br-md bg-[var(--surface-hover)] px-4 py-2.5 text-left text-[var(--text)]'
            : 'text-left text-black dark:text-white'
        }
      >
        {!user && (thinking || toolActivity.length > 0) && (
          <ThinkingActivity
            locale={locale}
            thinking={thinking}
            tools={toolActivity}
            thinkingStartedAtMs={Number.isFinite(reasoningStart) ? reasoningStart : null}
            thinkingFinishedAtMs={
              liveThinking
                ? null
                : Number.isFinite(reasoningFinished)
                  ? reasoningFinished
                  : null
            }
            nowMs={now}
          />
        )}
        {renderBlocks(bodyBlocks)}
        {!user &&
          (message.status === 'streaming' || message.status === 'running') &&
          bodyBlocks.length === 0 &&
          !liveThinking &&
          toolActivity.length === 0 && (
            <div className="flex items-center gap-2 text-sm text-[var(--text-secondary)]">
              <span className="h-2 w-2 animate-pulse rounded-full bg-[var(--primary)]" />
              {zh ? '正在思考' : 'Thinking'}
              {duration ? ` · ${duration}` : ''}
            </div>
          )}
      </div>
      <div
        className={`mt-1 flex min-h-6 items-center gap-1 text-[11px] text-[var(--text-disabled)] ${
          user ? 'justify-end' : 'justify-start'
        }`}
      >
        <span className="tabular-nums">
          {new Date(message.createdAt).toLocaleTimeString([], {
            hour: '2-digit',
            minute: '2-digit',
          })}
        </span>
        {!user && duration && !hasReasoning && (
          <span className="tabular-nums">· {duration}</span>
        )}
        {!user && tokens > 0 && <span className="tabular-nums">· {tokens} tokens</span>}
        <button
          type="button"
          onClick={() => onCopy(message.id, messagePlainText(message.contentBlocks))}
          title={zh ? '复制' : 'Copy'}
          className="ml-1 rounded p-1 hover:bg-[var(--surface-hover)] hover:text-[var(--text-secondary)]"
        >
          {copiedId === message.id ? <Check size={12} /> : <Copy size={12} />}
        </button>
        {isLastRetryable && onRetry && (
          <button
            type="button"
            onClick={onRetry}
            title={zh ? '重试' : 'Retry'}
            className="rounded p-1 hover:bg-[var(--surface-hover)] hover:text-[var(--text-secondary)]"
          >
            <RefreshCw size={12} />
          </button>
        )}
      </div>
    </article>
  );
}, (prev, next) => {
  // Finished rows ignore parent wall-clock ticks (subagent storms re-render every 100ms).
  const prevLive = prev.message.status === 'streaming' || prev.message.status === 'running';
  const nextLive = next.message.status === 'streaming' || next.message.status === 'running';
  if (prevLive || nextLive) {
    if (prev.now !== next.now) return false;
  }
  return (
    prev.message === next.message &&
    prev.locale === next.locale &&
    prev.zh === next.zh &&
    prev.isLastRetryable === next.isLastRetryable &&
    prev.onRetry === next.onRetry &&
    prev.copiedId === next.copiedId &&
    prev.onCopy === next.onCopy &&
    prev.runEvents === next.runEvents
  );
});

export default function ConversationTimeline({
  messages,
  loading,
  locale,
  onRetry,
  eventsByRun = {},
  changeEvents = EMPTY_RUN_EVENTS,
  fileChanges = [],
  onRollbackChanges,
  hasMoreOlder = false,
  loadingOlder = false,
  onLoadOlder,
}: ConversationTimelineProps) {
  const zh = locale.startsWith('zh');
  const scrollRef = useRef<HTMLDivElement>(null);
  const [following, setFollowing] = useState(true);
  const [copiedId, setCopiedId] = useState<string | null>(null);
  // Wall-clock for live elapsed labels. Must tick faster than formatElapsed's
  // 0.1s precision or "思考过程 · 0.7s" freezes between whole-second jumps.
  const [now, setNow] = useState(() => Date.now());
  const hasActive = messages.some(
    (message) => message.status === 'streaming' || message.status === 'running',
  );

  // Coarse render window: extra older rows the user revealed locally.
  const [revealedOlder, setRevealedOlder] = useState(0);
  const edgesRef = useRef<TimelineEdgeIds | null>(null);
  useEffect(() => {
    const next: TimelineEdgeIds = {
      length: messages.length,
      firstId: messages[0]?.id ?? null,
      lastId: messages[messages.length - 1]?.id ?? null,
    };
    const prev = edgesRef.current;
    edgesRef.current = next;
    const delta = classifyTimelineDelta(prev, next);
    if (delta === 'reset') {
      setRevealedOlder((current) => (current === 0 ? current : 0));
    } else if (delta === 'prepend' && prev) {
      // Host page landed on top — keep it visible instead of window-hiding it.
      setRevealedOlder((current) => current + (next.length - prev.length));
    }
  }, [messages]);

  const windowStart = timelineWindowStart(messages.length, revealedOlder);
  const visibleMessages = windowStart > 0 ? messages.slice(windowStart) : messages;
  const hiddenOlderCount = windowStart;

  const loadOlder = async () => {
    const el = scrollRef.current;
    const before = el ? { height: el.scrollHeight, top: el.scrollTop } : null;
    await onLoadOlder?.();
    if (!el || !before) return;
    requestAnimationFrame(() => {
      el.scrollTop = before.top + (el.scrollHeight - before.height);
    });
  };

  // Same scroll-position compensation as loadOlder, but for the local window.
  const revealOlder = () => {
    const el = scrollRef.current;
    const before = el ? { height: el.scrollHeight, top: el.scrollTop } : null;
    setRevealedOlder((current) => current + TIMELINE_WINDOW_STEP);
    if (!el || !before) return;
    requestAnimationFrame(() => {
      el.scrollTop = before.top + (el.scrollHeight - before.height);
    });
  };

  useEffect(() => {
    if (!hasActive) return;
    setNow(Date.now());
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [hasActive]);

  useEffect(() => {
    if (!following) return;
    const el = scrollRef.current;
    if (!el) return;
    el.scrollTop = el.scrollHeight;
  }, [following, messages]);

  const lastRetryableId = useMemo(
    () =>
      [...messages]
        .reverse()
        .find(
          (message) =>
            message.role === 'assistant' &&
            (message.status === 'failed' || message.status === 'interrupted'),
        )?.id,
    [messages],
  );
  const latest = messages[messages.length - 1];
  const showChangeSummary = Boolean(
    latest?.role === 'assistant' &&
      !hasActive &&
      (latest.status === 'complete' || latest.status === 'completed' || latest.status === 'done'),
  );

  const onCopy = (id: string, text: string) => {
    void navigator.clipboard.writeText(text);
    setCopiedId(id);
    window.setTimeout(() => setCopiedId(null), 1200);
  };

  if (loading) {
    return (
      <div className="grid h-full place-items-center text-sm text-[var(--text-disabled)]">
        {zh ? '加载中…' : 'Loading…'}
      </div>
    );
  }
  if (messages.length === 0) {
    return (
      <div className="grid h-full place-items-center text-center">
        <div>
          <div className="text-sm text-[var(--text-secondary)]">
            {zh ? '开始新的对话' : 'Start a new conversation'}
          </div>
          <div className="mt-1 text-xs text-[var(--text-disabled)]">
            {zh ? '在下方描述你想完成的任务' : 'Describe the task below'}
          </div>
        </div>
      </div>
    );
  }

  return (
    <div
      ref={scrollRef}
      onScroll={(event) => {
        const element = event.currentTarget;
        const nearBottom =
          element.scrollHeight - element.scrollTop - element.clientHeight < NEAR_BOTTOM_PX;
        setFollowing(nearBottom);
      }}
      className="relative h-full overflow-y-auto"
    >
      <div className="mx-auto flex w-full max-w-[860px] flex-col gap-7 px-5 py-7">
        {hiddenOlderCount > 0 ? (
          <button
            type="button"
            onClick={revealOlder}
            className="self-center rounded border px-3 py-1 text-xs text-[var(--text-secondary)]"
            data-testid="timeline-show-earlier"
          >
            {/* i18n-pending: i18n files frozen this round; follow file-local zh/en pattern. */}
            {zh
              ? `显示更早 ${Math.min(TIMELINE_WINDOW_STEP, hiddenOlderCount)} 条（还有 ${hiddenOlderCount} 条未显示）`
              : `Show ${Math.min(TIMELINE_WINDOW_STEP, hiddenOlderCount)} earlier (${hiddenOlderCount} hidden)`}
          </button>
        ) : hasMoreOlder && onLoadOlder ? (
          <button type="button" onClick={() => void loadOlder()} disabled={loadingOlder} className="self-center rounded border px-3 py-1 text-xs text-[var(--text-secondary)]">
            {loadingOlder ? (zh ? '加载中…' : 'Loading…') : (zh ? '加载更早消息' : 'Load older messages')}
          </button>
        ) : null}
        {visibleMessages.map((message) => (
          <MessageRow
            key={message.id}
            message={message}
            locale={locale}
            zh={zh}
            now={message.status === 'streaming' || message.status === 'running' ? now : 0}
            isLastRetryable={message.id === lastRetryableId}
            onRetry={onRetry}
            copiedId={copiedId}
            onCopy={onCopy}
            runEvents={
              message.runId ? eventsByRun[message.runId] ?? EMPTY_RUN_EVENTS : EMPTY_RUN_EVENTS
            }
          />
        ))}
        {showChangeSummary && (
          <ChangeSummaryCard
            locale={locale}
            events={changeEvents}
            fileChanges={fileChanges}
            onRollbackChanges={onRollbackChanges}
          />
        )}
      </div>
      {!following && (
        <button
          type="button"
          onClick={() => {
            setFollowing(true);
            scrollRef.current?.scrollTo({
              top: scrollRef.current.scrollHeight,
              behavior: 'smooth',
            });
          }}
          className="sticky bottom-4 left-1/2 grid h-8 w-8 -translate-x-1/2 place-items-center rounded-full border border-[var(--border)] bg-[var(--surface)] shadow"
        >
          <ArrowDown size={15} />
        </button>
      )}
    </div>
  );
}
