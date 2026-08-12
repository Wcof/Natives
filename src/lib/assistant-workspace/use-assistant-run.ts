'use client';

import { useCallback, useRef } from 'react';
import { isActiveRunStatus } from '@/lib/assistant-protocol';
import { useAssistantDispatch, useAssistantGateway, useAssistantStore } from './context';
import { cancelRun, respondPermission, retryRun, subscribeRun, reconcileExhaustedRun } from './controller';
import { cancelUnwantedSubscriptions, replaceRunSubscription } from './subscription-coordination';

/** Quiet empty-poll续接间隔：正常长轮询空轮后重订阅一次（不设独立预算）。 */
const RESUB_STEP_MS = 250;

/**
 * Bounded reconnect (product decision 4, 审计收口 #4)：
 * Adapter（readPersistentStream）是唯一的重连预算 owner（MAX_WATCH_RECONNECTS、
 * 退避、双游标、watch handle、AbortController 全部在 Adapter 内）。本 Hook
 * 只启动/中止一个订阅：
 * - 订阅抛错（预算耗尽 / transport）→ 不再自行重订阅，调用
 *   reconcileExhaustedRun 做权威对账（run.getActivity → run.cancel 写回）；
 * - iterator 正常结束且无 terminal（长轮询空轮）→ 静默续接一次，保持 live，
 *   不重置任何重连预算。
 * 单 run 的 transport 状态绝不写全局 connection（全局 Banner 只表示 daemon
 * 级 starting/offline/fatal/incompatible）。
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

  const startSubscription = useCallback(
    async (runId: string, afterSequence: number) => {
      // At most one live loop per run; other runs keep polling untouched.
      const signal = replaceRunSubscription(subSignalsRef.current, runId);

      try {
        await subscribeRun(gateway, dispatch, () => stateRef.current, runId, afterSequence, signal);
      } catch {
        // 预算耗尽 / transport 错误：Adapter 已耗尽唯一预算，不再重订阅。
        // 权威对账（run.getActivity → run.cancel 恰好一次）由
        // reconcileExhaustedRun 完成，权威 Run 写回 store，清 recovering。
        if (!signal.aborted) {
          const run = stateRef.current.runs[runId];
          if (run && isActiveRunStatus(run.status)) {
            void reconcileExhaustedRun(gateway, dispatch, runId);
          }
        }
        delete subSignalsRef.current[runId];
        return;
      }
      if (signal.aborted) return;

      const run = stateRef.current.runs[runId];
      if (!run || !isActiveRunStatus(run.status)) {
        delete subSignalsRef.current[runId];
        return;
      }

      // 长轮询空轮正常结束（无 terminal、无错误）：静默续接一次保持 live。
      // 不重置 Adapter 的重连预算；重连预算耗尽只发生在 Adapter 内部抛错路径。
      const nextSeq = stateRef.current.lastSequenceByRun[runId] ?? afterSequence;
      window.setTimeout(
        () => {
          if (!signal.aborted && subSignalsRef.current[runId] === signal) {
            void startSubscription(runId, nextSeq);
          }
        },
        RESUB_STEP_MS,
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
      for (const runId of cancelled) delete subSignalsRef.current[runId];
      for (const runId of wanted) ensureRunSubscription(runId);
    },
    [ensureRunSubscription],
  );

  /** Stop every loop this hook owns — for unmount. */
  const abortAllSubscriptions = useCallback(() => {
    cancelUnwantedSubscriptions(subSignalsRef.current, new Set());
    subSignalsRef.current = {};
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
