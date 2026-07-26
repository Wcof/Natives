'use client';

import { useCallback, useRef } from 'react';
import { isActiveRunStatus } from '@/lib/assistant-protocol';
import { useAssistantDispatch, useAssistantGateway, useAssistantStore } from './context';
import { cancelRun, respondPermission, retryRun, subscribeRun } from './controller';
import { cancelUnwantedSubscriptions, replaceRunSubscription } from './subscription-coordination';

/** Quiet-resubscribe backoff: grows per empty poll, capped so a live run stays responsive. */
const RESUB_STEP_MS = 250;
const RESUB_MAX_MS = 2000;

/**
 * Run lifecycle for any surface that talks to the engine.
 *
 * `controller.ts` already exposes the individual calls as free functions. What
 * was missing — and what every consumer would otherwise have to rebuild — is the
 * subscription loop around them: per-run abort signals, quiet resubscribe with
 * backoff, and the rule that an empty poll must NOT be reported as a lost
 * connection. That last one is not a detail; treating quiet polls as failures
 * previously produced spurious "reconnecting" banners during perfectly healthy
 * runs, and any second implementation would rediscover that the hard way.
 *
 * Consumers: AssistantWorkbench and the creator workbench's CreationSession.
 */
export function useAssistantRun() {
  const state = useAssistantStore();
  const dispatch = useAssistantDispatch();
  const gateway = useAssistantGateway();

  // Read-through ref so the long-lived subscription loop always sees current
  // state without re-creating itself on every store update.
  const stateRef = useRef(state);
  stateRef.current = state;

  const subSignalsRef = useRef<Record<string, { aborted: boolean }>>({});
  const resubAttemptsRef = useRef<Record<string, number>>({});

  const startSubscription = useCallback(
    async (runId: string, afterSequence: number) => {
      // At most one live loop per run; other runs keep polling untouched.
      const signal = replaceRunSubscription(subSignalsRef.current, runId);

      try {
        await subscribeRun(gateway, dispatch, () => stateRef.current, runId, afterSequence, signal);
      } catch {
        // Real transport errors flip connection state inside the controller.
        // Anything else is handled by the quiet-resubscribe path below.
      }
      if (signal.aborted) return;

      const run = stateRef.current.runs[runId];
      if (!run || !isActiveRunStatus(run.status)) {
        delete resubAttemptsRef.current[runId];
        delete subSignalsRef.current[runId];
        return;
      }

      // The iterator ending without a terminal event is normal long-poll
      // behaviour, not a disconnect. Resubscribe quietly with backoff and leave
      // the global connection state alone.
      const nextSeq = stateRef.current.lastSequenceByRun[runId] ?? afterSequence;
      // Receiving events resets the delay; only silence stretches it.
      if (nextSeq > afterSequence) resubAttemptsRef.current[runId] = 0;
      const attempt = (resubAttemptsRef.current[runId] ?? 0) + 1;
      resubAttemptsRef.current[runId] = attempt;

      window.setTimeout(
        () => {
          if (!signal.aborted && subSignalsRef.current[runId] === signal) {
            void startSubscription(runId, nextSeq);
          }
        },
        Math.min(RESUB_STEP_MS * attempt, RESUB_MAX_MS),
      );
    },
    [gateway, dispatch],
  );

  /** Idempotent: safe to call on every render or event without stacking loops. */
  const ensureRunSubscription = useCallback(
    (runId: string | null | undefined) => {
      if (!runId) return;
      const run = stateRef.current.runs[runId];
      if (!run || !isActiveRunStatus(run.status)) return;
      const existing = subSignalsRef.current[runId];
      if (existing && !existing.aborted) return;
      void startSubscription(runId, stateRef.current.lastSequenceByRun[runId] ?? 0);
    },
    [startSubscription],
  );

  /**
   * Keep loops only for `wanted`, dropping the rest.
   *
   * Without this, every historical active run kept polling after a session
   * switch — multi-subscription thrash and wasted gateway traffic.
   */
  const retainSubscriptions = useCallback(
    (wanted: Set<string>) => {
      const cancelled = cancelUnwantedSubscriptions(subSignalsRef.current, wanted);
      for (const runId of cancelled) delete resubAttemptsRef.current[runId];
      for (const runId of wanted) ensureRunSubscription(runId);
    },
    [ensureRunSubscription],
  );

  /** Stop every loop this hook owns — for unmount. */
  const abortAllSubscriptions = useCallback(() => {
    cancelUnwantedSubscriptions(subSignalsRef.current, new Set());
    subSignalsRef.current = {};
    resubAttemptsRef.current = {};
  }, []);

  const stop = useCallback(
    async (runId: string) => {
      await cancelRun(gateway, dispatch, runId);
    },
    [gateway, dispatch],
  );

  const retry = useCallback(
    async (runId: string) => {
      const newRunId = await retryRun(gateway, dispatch, runId);
      // A retry produces a new run id; without subscribing here its events would
      // never reach the store.
      if (newRunId) ensureRunSubscription(newRunId);
      return newRunId;
    },
    [gateway, dispatch, ensureRunSubscription],
  );

  const respond = useCallback(
    async (requestId: string, approved: boolean, scope: string, runId?: string) => {
      await respondPermission(gateway, dispatch, requestId, approved, scope, runId);
    },
    [gateway, dispatch],
  );

  return {
    startSubscription,
    ensureRunSubscription,
    retainSubscriptions,
    abortAllSubscriptions,
    stop,
    retry,
    respondPermission: respond,
  };
}
