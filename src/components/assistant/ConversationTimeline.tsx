'use client';

import { memo, useEffect, useMemo, useRef, useState } from 'react';
import { ArrowDown, Check, Copy, RefreshCw } from 'lucide-react';
import { formatElapsed, messagePlainText } from '@/lib/assistant-message-view';
import { renderBlocks, type ContentBlock } from './blocks';

export interface Message {
  id: string;
  role: 'system' | 'user' | 'assistant';
  contentBlocks: ContentBlock[];
  status: string;
  createdAt: string;
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
}

const NEAR_BOTTOM_PX = 80;

const MessageRow = memo(function MessageRow({
  message,
  locale,
  zh,
  now,
  hasActive,
  isLastRetryable,
  onRetry,
  copiedId,
  onCopy,
}: {
  message: Message;
  locale: string;
  zh: boolean;
  now: number;
  hasActive: boolean;
  isLastRetryable: boolean;
  onRetry?: () => void;
  copiedId: string | null;
  onCopy: (id: string, text: string) => void;
}) {
  const user = message.role === 'user';
  const start = message.startedAt ? Date.parse(message.startedAt) : Number.NaN;
  const end = message.finishedAt ? Date.parse(message.finishedAt) : now || start;
  const duration = Number.isFinite(start) ? formatElapsed(end - start) : null;
  const tokens = (message.inputTokens ?? 0) + (message.outputTokens ?? 0);
  const reasoningStart = message.reasoningStartedAt
    ? Date.parse(message.reasoningStartedAt)
    : Number.NaN;
  const reasoningEnd = message.reasoningFinishedAt
    ? Date.parse(message.reasoningFinishedAt)
    : hasActive
      ? now
      : Number.NaN;
  const messageLive = message.status === 'streaming' || message.status === 'running';
  // Keep real event / store order — do not force reasoning to the top.
  const contentBlocks = message.contentBlocks.map((block) => {
    if (block.type !== 'reasoning') return block;
    const withLocale = {
      ...block,
      locale: locale.startsWith('zh') ? 'zh' : 'en',
      live: messageLive && Boolean(block.live ?? true),
    };
    if (!Number.isFinite(reasoningStart)) return withLocale;
    return {
      ...withLocale,
      durationMs: Math.max(
        0,
        (Number.isFinite(reasoningEnd) ? reasoningEnd : now) - reasoningStart,
      ),
    };
  });
  const hasReasoning = contentBlocks.some((block) => block.type === 'reasoning');

  return (
    <article
      className={user ? 'ml-auto max-w-[78%]' : 'mr-auto w-full max-w-[760px]'}
    >
      <div
        className={
          user
            ? 'rounded-2xl rounded-br-md bg-[var(--surface-hover)] px-4 py-2.5 text-left text-[var(--text)]'
            : 'text-left text-black dark:text-white'
        }
      >
        {renderBlocks(contentBlocks)}
        {!user &&
          (message.status === 'streaming' || message.status === 'running') &&
          contentBlocks.length === 0 && (
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
}: ConversationTimelineProps) {
  const zh = locale.startsWith('zh');
  const scrollRef = useRef<HTMLDivElement>(null);
  const [following, setFollowing] = useState(true);
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [now, setNow] = useState(0);
  const hasActive = messages.some(
    (message) => message.status === 'streaming' || message.status === 'running',
  );
  // Elapsed timer: 1s cadence, isolated from stream chunk rate.
  useEffect(() => {
    if (!hasActive) return;
    setNow(Date.now());
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [hasActive]);

  // Stick to bottom without smooth animation on every stream chunk.
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
            hasActive={hasActive}
            isLastRetryable={message.id === lastRetryableId}
            onRetry={onRetry}
            copiedId={copiedId}
            onCopy={onCopy}
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
