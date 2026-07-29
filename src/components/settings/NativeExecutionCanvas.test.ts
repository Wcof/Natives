import assert from 'node:assert/strict';
import test from 'node:test';
import {
  CANONICAL_STAGE_IDS,
  layoutStages,
  stageEvidenceCode,
  stageLabel,
  stagePresentation,
  visibleEdges,
  type CanvasStage,
  type CanvasTraceEntry,
} from './nativeExecutionCanvasModel';

const canonicalStages: CanvasStage[] = CANONICAL_STAGE_IDS.map((id, order) => ({
  id,
  order,
  hook_points: [],
}));

test('execution canvas gives every canonical daemon stage a human-readable label', () => {
  assert.equal(stageLabel('session', 'zh'), '收到请求');
  assert.equal(stageLabel('context', 'zh'), '准备身份与规则');
  assert.equal(stageLabel('provider', 'zh'), '调用 AI 模型');
  assert.equal(stageLabel('tool_gate', 'zh'), '选择下一步能力');
  assert.equal(stageLabel('permission', 'zh'), '确认操作权限');
  assert.equal(stageLabel('tool_execute', 'zh'), '执行工具');
  assert.equal(stageLabel('subagent', 'zh'), '派发子 Agent');
  assert.equal(stageLabel('compact', 'zh'), '整理上下文');
  assert.equal(stageLabel('stop', 'zh'), '判断是否继续');
  assert.equal(stageLabel('terminal', 'zh'), '返回结果');
  assert.equal(stageLabel('cross_stage', 'zh'), '发送全程通知');
  assert.equal(stageLabel('future_stage', 'zh'), 'future_stage');
  assert.equal(stagePresentation('future_stage', 'zh').group, 'other');
});

test('layout preserves every returned stage, including an unknown twelfth node', () => {
  const stages = [...canonicalStages, { id: 'future_stage', order: 11 }];
  const layout = layoutStages(stages, 'zh');
  assert.equal(layout.positions.size, stages.length);
  assert.deepEqual([...layout.positions.keys()].sort(), stages.map((stage) => stage.id).sort());
  assert.equal(new Set([...layout.positions.values()].map(({ x, y }) => `${x}:${y}`)).size, stages.length);
  assert.ok(layout.groups.some((group) => group.id === 'other'));
});

test('canvas only renders authoritative edges whose endpoints exist', () => {
  const edges = [
    { from: 'session', to: 'context', kind: 'flow' as const },
    { from: 'context', to: 'future_stage', kind: 'flow' as const },
    { from: 'missing', to: 'session', kind: 'flow' as const },
  ];
  assert.deepEqual(visibleEdges(canonicalStages, edges), [edges[0]]);
});

test('every current stage exposes at least one proven configuration destination', () => {
  for (const stage of canonicalStages) {
    assert.ok(stagePresentation(stage.id, 'zh').targets.length > 0, stage.id);
  }
  assert.deepEqual(stagePresentation('future_stage', 'zh').targets, []);
});

test('audit calls Hook evidence what it is instead of verifying a whole stage', () => {
  const stage: CanvasStage = {
    id: 'tool_gate',
    order: 3,
    hook_points: [{ event: 'PreToolUse', enabled_hook_count: 1, dispatched: true }],
  };
  const completed: CanvasTraceEntry[] = [{
    run_id: 'run-1',
    sequence: 2,
    timestamp: '2026-07-28T00:00:00Z',
    type: 'hook_invocation_completed',
    invocation_id: 'inv-1',
    hook_event: 'PreToolUse',
    status: 'completed',
  }];

  assert.equal(stageEvidenceCode({
    mode: 'audit',
    stage,
    promptBlockCount: 0,
    selectedRunId: 'run-1',
    traceEntries: completed,
    runSnapshot: { resolved: true },
  }), 'hook_evidence');

  assert.equal(stageEvidenceCode({
    mode: 'audit',
    stage: { id: 'provider', order: 2, hook_points: [] },
    promptBlockCount: 0,
    selectedRunId: 'run-1',
    traceEntries: completed,
    runSnapshot: { resolved: true },
  }), 'no_evidence');
});

test('audit distinguishes failed Hook evidence and a recorded prompt snapshot', () => {
  assert.equal(stageEvidenceCode({
    mode: 'audit',
    stage: { id: 'tool_execute', hook_points: [{ event: 'PostToolUse' }] },
    promptBlockCount: 0,
    selectedRunId: 'run-1',
    traceEntries: [{
      run_id: 'run-1',
      sequence: 4,
      timestamp: '2026-07-28T00:00:00Z',
      type: 'hook_invocation_completed',
      hook_event: 'PostToolUse',
      status: 'failed',
      error_category: 'handler_failure',
    }],
    runSnapshot: { resolved: true },
  }), 'failed');

  assert.equal(stageEvidenceCode({
    mode: 'audit',
    stage: { id: 'context', hook_points: [] },
    promptBlockCount: 0,
    selectedRunId: 'run-1',
    traceEntries: [],
    runSnapshot: { resolved: true, snapshot: { prompt_plan: { source_digests: ['digest'] } } },
  }), 'snapshot_recorded');
});
