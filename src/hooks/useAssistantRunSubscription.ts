'use client';

import { useEffect, useRef } from 'react';
import { useAssistantGateway, useAssistantStore } from '@/lib/assistant-workspace';
import { useAssistantRun } from '@/lib/assistant-workspace/use-assistant-run';
import { isActiveRunStatus } from '@/lib/assistant-protocol';
import type { ChildRunSummary, Run, SubagentSession } from '@/lib/assistant-protocol';
import { isTempConversationId } from '@/lib/assistant-temp-conversation';

export interface UseAssistantRunSubscriptionOptions {
  /** Root run of the currently selected root conversation (null when idle). */
  rootRun: Run | null;
  rootConversationId: string | null;
  /** Child runs of the root run tree (only active-status ones get subscribed). */
  children: ChildRunSummary[];
  subagentSessions: SubagentSession[];
}

/**
 * Run subscription ownership for the workbench.
 *
 * Wraps the shared `useAssistantRun` loop and additionally owns the two
 * subscription-related effects that used to live inside AssistantWorkbench:
 *
 * - the multi-run keep-alive effect (root + active children + subagent child
 *   runs stay subscribed, stale runs are dropped to avoid polling thrash), and
 * - the 30s subagent heartbeat for the root conversation.
 *
 * This is a single-owner class of state: every subscription/abort the workbench
 * performs goes through this hook, so the shell never juggles subSignals itself.
 */
export function useAssistantRunSubscription({
  rootRun,
  rootConversationId,
  children,
  subagentSessions,
}: UseAssistantRunSubscriptionOptions) {
  const state = useAssistantStore();
  const gateway = useAssistantGateway();
  const {
    startSubscription,
    ensureRunSubscription,
    retainSubscriptions,
    abortAllSubscriptions,
  } = useAssistantRun();

  // Read-through ref so long-lived subscription decisions always see current
  // state without re-creating effects on every store update.
  const stateRef = useRef(state);
  stateRef.current = state;

  // Multi-run subscription: keep root + active child runs subscribed without
  // cancelling others. Depend on status signatures (not array/object identity)
  // so empty selectChildRuns / store map replacement on unrelated ticks cannot
  // thrash soft-resubscribe.
  const childrenSubKey = children
    .map((ch) => `${ch.id}:${ch.status}`)
    .sort()
    .join('|');
  const subagentSubKey = subagentSessions
    .map((s) => {
      const childRunId = state.activeRunByConversation[s.childConversationId] ?? '';
      const status = childRunId ? state.runs[childRunId]?.status ?? '' : '';
      return `${s.childConversationId}:${childRunId}:${status}`;
    })
    .sort()
    .join('|');
  useEffect(() => {
    const wanted = new Set<string>();
    if (rootRun && isActiveRunStatus(rootRun.status)) wanted.add(rootRun.id);
    // children / sessions read from latest render via closure; deps are signature keys.
    for (const ch of children) {
      if (isActiveRunStatus(String(ch.status))) wanted.add(ch.id);
    }
    for (const s of subagentSessions) {
      const childRunId = stateRef.current.activeRunByConversation[s.childConversationId];
      if (childRunId) {
        const run = stateRef.current.runs[childRunId];
        if (run && isActiveRunStatus(run.status)) wanted.add(childRunId);
      }
    }
    // Drop soft-resub loops for runs no longer in the wanted set (switch session /
    // terminal child / parent left). Without this, every historical active run kept
    // polling → multi-subscription thrash and wasted gateway traffic.
    retainSubscriptions(wanted);
  }, [
    rootRun?.id,
    rootRun?.status,
    childrenSubKey,
    subagentSubKey,
    ensureRunSubscription,
    retainSubscriptions,
  ]);

  // Heartbeat: touch only the root conversation every 30s while assistant page is visible.
  // Do NOT loop over every subagent — daemon scopes keepalive by parent conversation_id.
  useEffect(() => {
    if (!rootConversationId || isTempConversationId(rootConversationId)) return;
    let cancelled = false;
    const tick = () => {
      if (cancelled) return;
      if (typeof document !== 'undefined' && document.visibilityState === 'hidden') return;
      void gateway
        .request('subagent.touch', {
          conversation_id: rootConversationId,
        })
        .catch(() => undefined);
    };
    tick();
    const handle = window.setInterval(tick, 30_000);
    return () => {
      cancelled = true;
      window.clearInterval(handle);
    };
  }, [rootConversationId, gateway]);

  return {
    startSubscription,
    ensureRunSubscription,
    retainSubscriptions,
    abortAllSubscriptions,
  };
}
