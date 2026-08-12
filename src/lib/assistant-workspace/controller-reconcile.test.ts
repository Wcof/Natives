import assert from 'node:assert/strict';
import test from 'node:test';
import { cancelRun, reconcileExhaustedRun } from './controller';
import type { AssistantGateway } from '../assistant-gateway/gateway';
import type { AssistantMethod } from '../assistant-protocol';
import { createInitialWorkspaceState, workspaceReducer } from './reducer';

// 审计收口 #4（W1-B）：cancelRun / reconcileExhaustedRun 的权威 Run upsert。
// 从 controller.test.ts 拆出（原文件超 1000 行，架构检查 over_1000 阻断）。

test('审计收口 #4: cancelRun upserts the authoritative Run returned by run.cancel', async () => {
  const seen: Array<{ method: AssistantMethod; params?: unknown }> = [];
  const adapter: AssistantGateway = {
    connect: async () => undefined,
    disconnect: async () => undefined,
    request: async (method: AssistantMethod, _params?: unknown) => {
      seen.push({ method, params: _params });
      if (method === 'run.cancel') {
        return {
          id: 'run-c',
          conversationId: 'conv-1',
          status: 'cancelled',
          providerId: 'openai',
          modelId: 'gpt-4o',
          permissionProfile: 'ask',
          startedAt: '2026-08-12T00:00:00.000Z',
        };
      }
      return { ok: true };
    },
    subscribe: async function* () {},
  } as unknown as AssistantGateway;
  let state = createInitialWorkspaceState();
  const dispatch = (a: import('./state').WorkspaceAction) => {
    state = workspaceReducer(state, a);
  };
  state = workspaceReducer(state, {
    type: 'run/upsert',
    run: {
      id: 'run-c',
      conversationId: 'conv-1',
      status: 'running',
      providerId: 'openai',
      modelId: 'gpt-4o',
      permissionProfile: 'ask',
      startedAt: '2026-08-12T00:00:00.000Z',
    },
  });
  await cancelRun(adapter, dispatch, 'run-c');
  // 权威终态必须写回 store（订阅已停止，无终态 event 消费者）。
  assert.equal(state.runs['run-c']?.status, 'cancelled', 'run.cancel 返回的权威 Run 必须 upsert');
  assert.equal(seen.some((s) => s.method === 'run.cancel'), true);
});

test('审计收口 #4: reconcileExhaustedRun upserts authoritative snapshot and cancels exactly once', async () => {
  let getActivityCalls = 0;
  let cancelCalls = 0;
  const adapter: AssistantGateway = {
    connect: async () => undefined,
    disconnect: async () => undefined,
    request: async (method: AssistantMethod, _params?: unknown) => {
      if (method === 'run.getActivity') {
        getActivityCalls += 1;
        return {
          id: 'run-r',
          conversationId: 'conv-1',
          status: 'running',
          providerId: 'openai',
          modelId: 'gpt-4o',
          permissionProfile: 'ask',
          startedAt: '2026-08-12T00:00:00.000Z',
        };
      }
      if (method === 'run.cancel') {
        cancelCalls += 1;
        return {
          id: 'run-r',
          conversationId: 'conv-1',
          status: 'cancelled',
          providerId: 'openai',
          modelId: 'gpt-4o',
          permissionProfile: 'ask',
          startedAt: '2026-08-12T00:00:00.000Z',
        };
      }
      return { ok: true };
    },
    subscribe: async function* () {},
  } as unknown as AssistantGateway;
  let state = createInitialWorkspaceState();
  const dispatch = (a: import('./state').WorkspaceAction) => {
    state = workspaceReducer(state, a);
  };
  state = workspaceReducer(state, {
    type: 'run/upsert',
    run: {
      id: 'run-r',
      conversationId: 'conv-1',
      status: 'running',
      providerId: 'openai',
      modelId: 'gpt-4o',
      permissionProfile: 'ask',
      startedAt: '2026-08-12T00:00:00.000Z',
    },
  });
  state = workspaceReducer(state, { type: 'recovering/set', runId: 'run-r', recovering: true });

  await reconcileExhaustedRun(adapter, dispatch, 'run-r');
  // 权威快照写回：getActivity 的 running + cancel 的 cancelled。
  assert.equal(state.runs['run-r']?.status, 'cancelled', '权威终态进入 store');
  assert.equal(cancelCalls, 1, '同 run cancel 恰好一次');
  assert.equal(getActivityCalls, 1);
  assert.equal(state.recoveringRuns['run-r'], undefined, '对账后清 recovering');
  // active→cancel 后折叠栏显示 exhausted 状态（RUN_WATCH_EXHAUSTED_KEY），
  // 且该状态是 run 级（非全局 connection/error）。
  assert.equal(
    state.runErrors['run-r'],
    'assistant.runWatchExhausted',
    '折叠栏显示 exhausted key（run 级）',
  );
  assert.equal(state.connectionError, null, '全局 banner 不出现');
});

test('审计收口 #4: reconcileExhaustedRun already-terminal run clears recovering without cancel', async () => {
  let cancelCalls = 0;
  const adapter: AssistantGateway = {
    connect: async () => undefined,
    disconnect: async () => undefined,
    request: async (method: AssistantMethod, _params?: unknown) => {
      if (method === 'run.getActivity') {
        return {
          id: 'run-t',
          conversationId: 'conv-1',
          status: 'interrupted',
          providerId: 'openai',
          modelId: 'gpt-4o',
          permissionProfile: 'ask',
          startedAt: '2026-08-12T00:00:00.000Z',
        };
      }
      if (method === 'run.cancel') {
        cancelCalls += 1;
        return { ok: true };
      }
      return { ok: true };
    },
    subscribe: async function* () {},
  } as unknown as AssistantGateway;
  let state = createInitialWorkspaceState();
  const dispatch = (a: import('./state').WorkspaceAction) => {
    state = workspaceReducer(state, a);
  };
  state = workspaceReducer(state, {
    type: 'run/upsert',
    run: {
      id: 'run-t',
      conversationId: 'conv-1',
      status: 'interrupted',
      providerId: 'openai',
      modelId: 'gpt-4o',
      permissionProfile: 'ask',
      startedAt: '2026-08-12T00:00:00.000Z',
    },
  });
  state = workspaceReducer(state, { type: 'recovering/set', runId: 'run-t', recovering: true });
  await reconcileExhaustedRun(adapter, dispatch, 'run-t');
  assert.equal(cancelCalls, 0, '已终态不 cancel');
  assert.equal(state.recoveringRuns['run-t'], undefined, '已终态清 recovering');
});
