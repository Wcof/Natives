'use client';

import { memo, useEffect, useMemo, useRef, useState } from 'react';
import { ArrowDown, Check, Copy, RefreshCw } from 'lucide-react';
import { formatElapsed, messagePlainText } from '@/lib/assistant-message-view';
import type { RunEvent } from '@/lib/assistant-protocol';
import {
  deriveToolActivityFromEvents,
  extractLiveThinking,
  filterTimelineBodyBlocks,
  selectActiveToolActivity,
} from '@/lib/assistant-timeline';
import { renderBlocks, type ContentBlock } from './blocks';
import ThinkingActivity from './ThinkingActivity';

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
}

const NEAR_BOTTOM_PX = 80;

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
  now: number;
  isLastRetryable: boolean;
  onRetry?: () => void;
  copiedId: string | null;
  onCopy: (id: string, text: string) => void;
  runEvents: RunEvent[];
}) {
  const user = message.role === 'user';
  const start = message.startedAt
    ? Date.parse(message.startedAt)
    : message.createdAt
      ? Date.parse(message.createdAt)
      : Number.NaN;
  // Footer total time: always track wall clock while the message is active.
  const end = message.finishedAt ? Date.parse(message.finishedAt) : now || start;
  const duration = Number.isFinite(start) ? formatElapsed(end - start) : null;
  const tokens = (message.inputTokens ?? 0) + (message.outputTokens ?? 0);
  const messageLive = message.status === 'streaming' || message.status === 'running';

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

  const bodyBlocks = useMemo(() => {
    const prepared = message.contentBlocks.map((block) => {
      if (block.type !== 'reasoning') return block;
      const blockLive = messageLive && Boolean(block.live ?? true);
      const withLocale = {
        ...block,
        locale: locale.startsWith('zh') ? 'zh' : 'en',
        live: blockLive,
      };
      if (!Number.isFinite(reasoningStart)) return withLocale;
      const endMs = blockLive
        ? now
        : Number.isFinite(reasoningFinished)
          ? reasoningFinished
          : now;
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
    reasoningFinished,
    now,
  ]);

  const toolActivity = useMemo(() => {
    if (!messageLive) return [];
    const all = deriveToolActivityFromEvents(runEvents);
    return selectActiveToolActivity(all);
  }, [runEvents, messageLive]);

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
        {!user && (liveThinking || toolActivity.length > 0) && (
          <ThinkingActivity
            locale={locale}
            thinking={liveThinking}
            tools={toolActivity}
            thinkingStartedAtMs={Number.isFinite(reasoningStart) ? reasoningStart : null}
            thinkingFinishedAtMs={
              liveThinking
                ? null
                : Number.isFinite(reasoningFinished)
                  ? reasoningFinished
                  : null
            }
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
});

export default function ConversationTimeline({
  messages,
  loading,
  locale,
  onRetry,
  eventsByRun = {},
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

  useEffect(() => {
    if (!hasActive) return;
    setNow(Date.now());
    // 100ms ≈ one update per 0.1s display step; rAF would re-render every frame.
    const timer = window.setInterval(() => setNow(Date.now()), 100);
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
        {messages.map((message) => (
          <MessageRow
            key={message.id}
            message={message}
            locale={locale}
            zh={zh}
            now={now}
            isLastRetryable={message.id === lastRetryableId}
            onRetry={onRetry}
            copiedId={copiedId}
            onCopy={onCopy}
            runEvents={message.runId ? eventsByRun[message.runId] ?? [] : []}
          />
        ))}
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
