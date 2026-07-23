/**
 * Agent E integration helpers: artifact envelope + surfaceConversationId.
 */
import assert from 'node:assert/strict';
import test from 'node:test';
import {
  mapWireArtifactList,
  unwrapArtifactListPayload,
} from '../assistant-protocol/wire';
import {
  selectSurfaceConversationId,
  selectArtifactsForRunTree,
} from './selectors';
import { createInitialWorkspaceState } from './state';
import { workspaceReducer } from './reducer';
import type { Artifact, Run } from '../assistant-protocol';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

test('unwrapArtifactListPayload accepts bare array and { artifacts } envelope', () => {
  const bare = unwrapArtifactListPayload([
    { id: 'a1', run_id: 'r1', path: '/tmp/a.txt', kind: 'file', size: 1 },
  ]);
  assert.equal(bare.length, 1);
  assert.equal(bare[0]!.id, 'a1');

  const env = unwrapArtifactListPayload({
    artifacts: [
      { id: 'a2', runId: 'r1', path: '/tmp/b.txt', kind: 'file', size: 2 },
    ],
  });
  assert.equal(env.length, 1);
  assert.equal(env[0]!.id, 'a2');

  assert.deepEqual(unwrapArtifactListPayload(null), []);
  assert.deepEqual(unwrapArtifactListPayload({}), []);
});

test('mapWireArtifactList maps both envelopes', () => {
  const list = mapWireArtifactList({
    artifacts: [{ id: 'x', run_id: 'r', path: 'p', kind: 'file', size: 3 }],
  });
  assert.equal(list.length, 1);
  assert.equal(list[0]!.id, 'x');
  assert.equal(list[0]!.runId, 'r');
  assert.equal(list[0]!.path, 'p');
});

test('selectSurfaceConversationId prefers child over root', () => {
  assert.equal(selectSurfaceConversationId('root', 'child'), 'child');
  assert.equal(selectSurfaceConversationId('root', null), 'root');
  assert.equal(selectSurfaceConversationId('root', '  '), 'root');
  assert.equal(selectSurfaceConversationId(null, 'child'), 'child');
  assert.equal(selectSurfaceConversationId(null, null), null);
});

test('selectArtifactsForRunTree merges parent + child run artifacts', () => {
  let state = createInitialWorkspaceState();
  const run: Run = {
    id: 'parent',
    conversationId: 'c1',
    status: 'running',
    providerId: 'p',
    modelId: 'm',
    permissionProfile: 'ask',
    startedAt: 't',
    retryCount: 0,
    lastEventSequence: 0,
  };
  state = workspaceReducer(state, { type: 'run/upsert', run });
  const arts: Artifact[] = [
    { id: 'a1', runId: 'parent', path: '/a', kind: 'file', size: 1 },
    { id: 'a2', runId: 'child', path: '/b', kind: 'file', size: 2 },
  ];
  state = {
    ...state,
    childRunsByParent: { parent: ['child'] },
    artifactsByRun: {
      parent: [arts[0]!],
      child: [arts[1]!],
    },
  };
  const merged = selectArtifactsForRunTree(state, 'parent');
  assert.equal(merged.length, 2);
  assert.ok(merged.some((a) => a.id === 'a1'));
  assert.ok(merged.some((a) => a.id === 'a2'));
});

test('i18n keys for A/B/C/subagent assignment exist in zh and en', () => {
  const zh = readFileSync(fileURLToPath(new URL('../../i18n/zh.ts', import.meta.url)), 'utf8');
  const en = readFileSync(fileURLToPath(new URL('../../i18n/en.ts', import.meta.url)), 'utf8');
  const keys = [
    'copyConversationId',
    'conversationIdCopied',
    'copyConversationIdFailed',
    'permission:',
    'activity:',
    'subagentAssignment:',
    'modeDefault',
    'switchKeyTitle',
    'mainTask',
    'backToMain',
    'missingDefaultBinding',
    'needValidKey',
    'tasks',
  ];
  for (const k of keys) {
    assert.ok(zh.includes(k), `zh missing ${k}`);
    assert.ok(en.includes(k), `en missing ${k}`);
  }
});

test('workbench returns Promise from permission handlers and uses multi-run subSignals', () => {
  const workbench = readFileSync(
    fileURLToPath(new URL('../../components/assistant/AssistantWorkbench.tsx', import.meta.url)),
    'utf8',
  );
  assert.match(workbench, /return handlePermission\(/);
  assert.match(workbench, /subSignalsRef/);
  assert.match(workbench, /selectedRootConversationId/);
  assert.match(workbench, /selectedChildConversationId/);
  assert.match(workbench, /subagent\.touch/);
  assert.match(workbench, /conversation_id: rootConversationId/);
  assert.match(workbench, /subagent\.switchRoute/);
  assert.match(workbench, /interaction\.respond/);
  assert.match(workbench, /SubagentAssignmentModal/);
  assert.match(workbench, /restarted_run_id/);
  assert.match(workbench, /rootEvents/);
  assert.match(workbench, /selectedChildEvents/);
  assert.match(workbench, /mainTodos/);
  // Must not soft-abort all runs with a single shared signal.
  assert.equal(workbench.includes('subAbortRef'), false);
  // Heartbeat must not loop over every subagent.
  assert.equal(/for \(const s of targets\)/.test(workbench), false);
});

test('reducer maps subagent_assignment interaction_requested', () => {
  let state = createInitialWorkspaceState();
  const run: Run = {
    id: 'r1',
    conversationId: 'c1',
    status: 'running',
    providerId: 'p',
    modelId: 'm',
    permissionProfile: 'ask',
    startedAt: 't',
    retryCount: 0,
    lastEventSequence: 0,
  };
  state = workspaceReducer(state, { type: 'run/upsert', run });
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: {
      runId: 'r1',
      sequence: 1,
      timestamp: 't',
      type: 'interaction_requested',
      payload: {
        interaction_id: 'ia1',
        kind: 'subagent_assignment',
        conversation_id: 'c1',
        reason: 'assign',
        batch_id: 'batch-1',
        parent_conversation_id: 'c1',
        parent_run_id: 'r1',
        default_binding: {
          provider_id: 'openai',
          key_id: 'k1',
          model_id: 'gpt-4o',
        },
        tasks: [
          { call_id: 't1', name: 'Research', prompt: 'Look up docs' },
          { call_id: 't2', name: 'Implement', prompt: 'Write code' },
          { call_id: 't3', name: 'Review', prompt: 'Check diffs' },
        ],
      },
    },
  });
  const interaction = state.interactions['ia1'];
  assert.ok(interaction);
  assert.equal(interaction.kind, 'subagent_assignment');
  if (interaction.kind === 'subagent_assignment') {
    assert.equal(interaction.conversationId, 'c1');
    assert.equal(interaction.batchId, 'batch-1');
    assert.equal(interaction.defaultBinding?.providerId, 'openai');
    assert.equal(interaction.defaultBinding?.keyId, 'k1');
    assert.equal(interaction.defaultBinding?.modelId, 'gpt-4o');
    assert.equal(interaction.tasks?.length, 3);
    assert.equal(interaction.tasks?.[0]?.callId, 't1');
    assert.equal(interaction.tasks?.[0]?.name, 'Research');
    assert.equal(interaction.tasks?.[2]?.name, 'Review');
  }
});
