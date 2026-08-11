'use client';

import { useCallback, useState } from 'react';
import { useAssistantDispatch, useAssistantGateway, useAssistantStore } from '@/lib/assistant-workspace';
import type { ContextUsage, FileChange, PlanApprovalInteraction, Run, RunEvent } from '@/lib/assistant-protocol';
import type { Locale } from '@/i18n';
import { t } from '@/i18n';
import ConversationTimeline, { type Message } from '@/components/assistant/conversation/ConversationTimeline';
import GoalStatusBar from '../GoalStatusBar';

export interface WorkbenchTimelinePaneProps {
  locale: Locale;
  timelineMessages: Message[];
  /** Visible surface events/files: root includes child agents; child is self-only. */
  events: RunEvent[];
  fileChanges: FileChange[];
  allowRewind: boolean;
  onRollbackChanges: ((changes: Array<{ path: string; runId?: string }>) => Promise<boolean>) | undefined;
  loadingMessages: boolean;
  onRetry: () => void;
  /** W8: fork the current conversation at the selected persisted user message. */
  onForkMessage: (messageId: string) => void;
  hasMoreOlder: boolean;
  loadingOlder: boolean;
  onLoadOlder: () => void;
  /** Pending plan_approval interaction rendered as an approve/reject banner. */
  planApproval: PlanApprovalInteraction | undefined;
  isGoalMode: boolean;
  goalTitle: string;
  goalInstruction: string | null;
  activeRun: Run | null;
  contextUsage: ContextUsage | null;
  goalCanResume: boolean;
  onPause: () => void;
  onResume: () => void;
  onDeleteGoal: () => void;
}

/**
 * Conversation content pane: the message timeline, the pending plan-approval
 * banner, and the goal-mode status bar.
 *
 * Reads only the event stream from the store; everything else (messages, plan
 * approval, goal chrome) arrives as props from the shell.
 */
export function WorkbenchTimelinePane({
  locale,
  timelineMessages,
  events,
  fileChanges,
  allowRewind,
  onRollbackChanges,
  loadingMessages,
  onRetry,
  onForkMessage,
  hasMoreOlder,
  loadingOlder,
  onLoadOlder,
  planApproval,
  isGoalMode,
  goalTitle,
  goalInstruction,
  activeRun,
  contextUsage,
  goalCanResume,
  onPause,
  onResume,
  onDeleteGoal,
}: WorkbenchTimelinePaneProps) {
  const state = useAssistantStore();
  const dispatch = useAssistantDispatch();
  const gateway = useAssistantGateway();
  const [searchOpen, setSearchOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState('');
  const [searchResults, setSearchResults] = useState<Array<{ message_id: string; role: string; snippet: string }>>([]);
  const [searchIndex, setSearchIndex] = useState(0);

  // W8: bounded full-conversation search via conversation.searchMessages.
  const runSearch = useCallback(
    async (q: string) => {
      if (!q.trim()) {
        setSearchResults([]);
        setSearchIndex(0);
        return;
      }
      const conversationId = state.activeRunByConversation
        ? Object.entries(state.activeRunByConversation).find(([, runId]) => runId) ?? null
        : null;
      // Use the timeline's active conversation when available via navigation
      // fallback; the daemon query is conversation-scoped and bounded.
      const resp = await gateway.request('conversation.searchMessages', {
        conversation_id: conversationId?.[0] ?? '',
        q,
        limit: 50,
      }).catch(() => ({ results: [] as Array<{ message_id: string; role: string; snippet: string }> }));
      const results = Array.isArray((resp as { results?: unknown[] })?.results)
        ? ((resp as { results: unknown[] }).results as Array<{ message_id: string; role: string; snippet: string }>)
        : [];
      setSearchResults(results);
      setSearchIndex(0);
      if (results.length > 0) {
        document.getElementById(`msg-${results[0]!.message_id}`)?.scrollIntoView({ block: 'center' });
      }
    },
    [gateway, state.activeRunByConversation],
  );

  const goNext = useCallback(() => {
    if (searchResults.length === 0) return;
    const next = (searchIndex + 1) % searchResults.length;
    setSearchIndex(next);
    document.getElementById(`msg-${searchResults[next]!.message_id}`)?.scrollIntoView({ block: 'center' });
  }, [searchIndex, searchResults]);

  const goPrev = useCallback(() => {
    if (searchResults.length === 0) return;
    const prev = (searchIndex - 1 + searchResults.length) % searchResults.length;
    setSearchIndex(prev);
    document.getElementById(`msg-${searchResults[prev]!.message_id}`)?.scrollIntoView({ block: 'center' });
  }, [searchIndex, searchResults]);

  return (
    <>
      <div className="flex items-center gap-2 border-b border-[var(--border)] px-3 py-1">
        <button
          type="button"
          onClick={() => {
            setSearchOpen(!searchOpen);
            if (!searchOpen) setSearchQuery('');
          }}
          aria-expanded={searchOpen}
          className="rounded px-2 py-1 text-[11px] text-[var(--text-disabled)] hover:bg-[var(--surface-hover)] hover:text-[var(--text-secondary)]"
        >
          {t(locale, 'assistant.searchMessages')}
        </button>
        {searchOpen ? (
          <input
            autoFocus
            type="search"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') void runSearch(searchQuery);
              if (e.key === 'Escape') setSearchOpen(false);
            }}
            placeholder={t(locale, 'assistant.searchPlaceholder')}
            aria-label={t(locale, 'assistant.searchMessages')}
            className="min-w-0 flex-1 rounded border border-[var(--border)] bg-[var(--surface)] px-2 py-1 text-xs"
          />
        ) : null}
        {searchResults.length > 0 ? (
          <span className="shrink-0 text-[11px] tabular-nums text-[var(--text-secondary)]">
            {searchIndex + 1}/{searchResults.length}
          </span>
        ) : null}
        {searchResults.length > 0 ? (
          <button
            type="button"
            onClick={goPrev}
            aria-label={t(locale, 'assistant.searchPrev')}
            className="shrink-0 rounded p-1 text-[var(--text-disabled)] hover:text-[var(--text-secondary)]"
          >
            ↑
          </button>
        ) : null}
        {searchResults.length > 0 ? (
          <button
            type="button"
            onClick={goNext}
            aria-label={t(locale, 'assistant.searchNext')}
            className="shrink-0 rounded p-1 text-[var(--text-disabled)] hover:text-[var(--text-secondary)]"
          >
            ↓
          </button>
        ) : null}
      </div>
      <div className="min-h-0 flex-1">
        <ConversationTimeline
          messages={timelineMessages}
          eventsByRun={state.eventsByRun}
          changeEvents={events}
          fileChanges={fileChanges}
          onRollbackChanges={allowRewind ? onRollbackChanges : undefined}
          loading={loadingMessages}
          locale={locale}
          onRetry={onRetry}
          onFork={onForkMessage}
          hasMoreOlder={hasMoreOlder}
          loadingOlder={loadingOlder}
          onLoadOlder={onLoadOlder}
        />
      </div>

      {planApproval && planApproval.kind === 'plan_approval' && (
        <div className="mx-4 mb-2 max-h-48 overflow-y-auto rounded-lg border border-[var(--border)] bg-[var(--surface)] p-3 text-sm">
          <div className="font-medium">{planApproval.title}</div>
          <pre className="mt-2 whitespace-pre-wrap text-xs text-[var(--text-secondary)]">
            {planApproval.planMarkdown}
          </pre>
          <div className="mt-2 flex gap-2">
            <button
              type="button"
              className="rounded bg-[var(--primary)] px-3 py-1 text-[var(--accent-ink)]"
              onClick={() =>
                void gateway
                  .request('interaction.respond', {
                    id: planApproval.id,
                    approved: true,
                    run_id: planApproval.runId,
                  })
                  .then(() => dispatch({ type: 'interaction/remove', id: planApproval.id }))
              }
            >
              {t(locale, 'assistant.approvePlan')}
            </button>
            <button
              type="button"
              className="rounded border border-[var(--border)] px-3 py-1"
              onClick={() =>
                void gateway
                  .request('interaction.respond', {
                    id: planApproval.id,
                    approved: false,
                    run_id: planApproval.runId,
                  })
                  .then(() => dispatch({ type: 'interaction/remove', id: planApproval.id }))
              }
            >
              {t(locale, 'assistant.rejectPlan')}
            </button>
          </div>
        </div>
      )}

      {isGoalMode ? (
        <GoalStatusBar
          goalTitle={goalTitle}
          instruction={goalInstruction}
          run={activeRun}
          locale={locale}
          tokenLabel={contextUsage ? `${contextUsage.usedTokens} tokens` : undefined}
          canResume={goalCanResume}
          onPause={onPause}
          onResume={onResume}
          onDelete={onDeleteGoal}
        />
      ) : null}
    </>
  );
}

export default WorkbenchTimelinePane;
