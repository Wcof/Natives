import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { createInitialWorkspaceState } from './state';
import {
  selectAllPendingInteractions,
  selectArtifacts,
  selectChildRuns,
  selectEventsForRunTree,
  selectFileChanges,
  selectFileChangesForRunTree,
  selectPendingInteractions,
  selectRunEvents,
} from './selectors';

describe('assistant workspace selectors', () => {
  it('keeps empty effect dependencies referentially stable', () => {
    const state = createInitialWorkspaceState();
    assert.strictEqual(selectRunEvents(state, null), selectRunEvents(state, null));
    assert.strictEqual(selectFileChanges(state, null), selectFileChanges(state, null));
    assert.strictEqual(selectArtifacts(state, null), selectArtifacts(state, null));
    assert.strictEqual(selectChildRuns(state, null), selectChildRuns(state, null));
    assert.strictEqual(selectEventsForRunTree(state, null), selectEventsForRunTree(state, null));
    assert.strictEqual(
      selectFileChangesForRunTree(state, null),
      selectFileChangesForRunTree(state, null),
    );
  });

  it('问题3：未绑定会话的 pending interaction 不投影到任意会话', () => {
    let state = createInitialWorkspaceState();
    state = {
      ...state,
      interactionOrder: ['i1', 'i2'],
      interactions: {
        i1: {
          id: 'i1',
          kind: 'permission',
          runId: 'r1',
          conversationId: null as unknown as string,
          createdAt: '2026-08-12T00:00:00.000Z',
          toolCallId: 'tc1',
          toolName: 'bash',
          reason: 'run',
          input: {},
        },
        i2: {
          id: 'i2',
          kind: 'permission',
          runId: 'r2',
          conversationId: 'c-new',
          createdAt: '2026-08-12T00:00:00.000Z',
          toolCallId: 'tc2',
          toolName: 'bash',
          reason: 'run',
          input: {},
        },
      },
    };
    // 新项目无会话上下文：不投影未绑定 interaction。
    assert.equal(selectPendingInteractions(state).length, 0);
    assert.equal(selectPendingInteractions(state, 'c-any').length, 0, '未绑定 interaction 不得匹配任意会话');
    // 只有显式绑定到该会话的 interaction 投影。
    assert.equal(selectPendingInteractions(state, 'c-new').length, 1);
    // 全局徽章仍统计全部（含未绑定）。
    assert.equal(selectAllPendingInteractions(state).length, 2);
  });
});
