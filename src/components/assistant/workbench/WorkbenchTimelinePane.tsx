'use client';

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

  return (
    <>
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
