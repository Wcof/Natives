import assert from 'node:assert/strict';
import test from 'node:test';
import type { DaemonCapabilities } from '@/lib/assistant-protocol';
import {
  buildDiagnosticsText,
  canBrowseMcpHub,
  canCancelTask,
  canInterject,
  canListTaskDepth,
  canListTasks,
  canManageCapabilities,
  canRewind,
  canSelectRunCapabilities,
  canShowContextUsage,
  hasMethod,
  HOST_METHODS_UI,
  needsEngineRecovery,
} from './capability-gate';

function caps(methods: string[]): DaemonCapabilities {
  return {
    protocolVersion: '2.0.0',
    methods,
    providers: [],
    tools: false,
    hooks: false,
    subagents: false,
    mcp: false,
    extensions: false,
    scheduler: false,
    eventReplay: false,
    credentialBroker: false,
  };
}

test('hasMethod is false for null/empty capabilities', () => {
  assert.equal(hasMethod(null, 'run.start'), false);
  assert.equal(hasMethod(undefined, 'run.start'), false);
  assert.equal(hasMethod(caps([]), 'run.start'), false);
});

test('hasMethod matches advertised methods only', () => {
  const c = caps(['run.start', 'run.cancel']);
  assert.equal(hasMethod(c, 'run.start'), true);
  assert.equal(hasMethod(c, 'run.getActivity'), false);
});

test('canRewind requires both non-deprecated workspace restore methods', () => {
  assert.equal(canRewind(null), false);
  assert.equal(canRewind(caps(['run.start'])), false);
  assert.equal(canRewind(caps(['run.start', 'run.getActivity'])), false);
  assert.equal(canRewind(caps(['workspace.restore'])), false);
  assert.equal(canRewind(caps(['workspace.restore', 'workspace.restorePreview'])), true);
});

test('canShowContextUsage requires conversation.getContextUsage', () => {
  assert.equal(canShowContextUsage(null), false);
  assert.equal(canShowContextUsage(caps(['run.start'])), false);
  assert.equal(canShowContextUsage(caps(['conversation.getContextUsage'])), true);
});

test('canListTasks accepts task.list or run.listChildren', () => {
  assert.equal(canListTasks(null), false);
  assert.equal(canListTasks(caps(['run.start'])), false);
  assert.equal(canListTasks(caps(['task.list'])), true);
  assert.equal(canListTasks(caps(['run.listChildren'])), true);
});

test('canListTaskDepth requires task.list (not mere run.listChildren)', () => {
  assert.equal(canListTaskDepth(null), false);
  assert.equal(canListTaskDepth(caps(['run.listChildren'])), false);
  assert.equal(canListTaskDepth(caps(['task.list'])), true);
});

test('canCancelTask requires task.cancel', () => {
  assert.equal(canCancelTask(null), false);
  assert.equal(canCancelTask(caps(['task.list'])), false);
  assert.equal(canCancelTask(caps(['task.cancel'])), true);
});

test('canInterject requires promptQueue.interject', () => {
  assert.equal(canInterject(null), false);
  assert.equal(canInterject(caps(['promptQueue.enqueue'])), false);
  assert.equal(canInterject(caps(['promptQueue.interject'])), true);
});

test('canManageCapabilities requires capability.skill.list', () => {
  assert.equal(canManageCapabilities(null), false);
  assert.equal(canManageCapabilities(undefined), false);
  assert.equal(canManageCapabilities(caps([])), false);
  assert.equal(canManageCapabilities(caps(['skill.list', 'mcp.list'])), false);
  assert.equal(canManageCapabilities(caps(['capability.mcp.list'])), false);
  assert.equal(canManageCapabilities(caps(['capability.skill.list'])), true);
});

test('canBrowseMcpHub requires capability.mcp.hub.search (may lag base family)', () => {
  assert.equal(canBrowseMcpHub(null), false);
  assert.equal(canBrowseMcpHub(caps(['capability.skill.list', 'capability.mcp.list'])), false);
  assert.equal(canBrowseMcpHub(caps(['capability.mcp.hub.search'])), true);
});

test('canSelectRunCapabilities requires conversation.updateCapabilities', () => {
  assert.equal(canSelectRunCapabilities(null), false);
  assert.equal(canSelectRunCapabilities(caps(['capability.skill.list'])), false);
  assert.equal(canSelectRunCapabilities(caps(['conversation.getCapabilities'])), false);
  assert.equal(canSelectRunCapabilities(caps(['conversation.updateCapabilities'])), true);
});

test('needsEngineRecovery blocks fatal/incompatible and offline without caps', () => {
  assert.equal(needsEngineRecovery('fatal', caps(['run.start'])), true);
  assert.equal(needsEngineRecovery('incompatible', null), true);
  assert.equal(needsEngineRecovery('offline', null), true);
  assert.equal(needsEngineRecovery('disconnected', null), false);
  assert.equal(needsEngineRecovery('offline', caps(['run.start'])), false);
  assert.equal(needsEngineRecovery('connected', null), false);
  assert.equal(needsEngineRecovery('connected', caps(['run.start'])), false);
  assert.equal(needsEngineRecovery('reconnecting', null), false);
  assert.equal(needsEngineRecovery('connecting', null), false);
});

test('buildDiagnosticsText includes connection fields', () => {
  const text = buildDiagnosticsText({
    connection: 'fatal',
    connectionError: 'boom',
    protocolVersion: '2.0.0',
    methodsCount: 3,
    reconnectAttempts: 2,
  });
  assert.match(text, /connection=fatal/);
  assert.match(text, /error=boom/);
  assert.match(text, /methods=3/);
});

test('HOST_METHODS_UI lists core host RPCs for contract checks', () => {
  assert.ok(HOST_METHODS_UI.includes('promptQueue.enqueue'));
  assert.ok(HOST_METHODS_UI.includes('permission.respond'));
  assert.ok(HOST_METHODS_UI.includes('interaction.respond'));
});
