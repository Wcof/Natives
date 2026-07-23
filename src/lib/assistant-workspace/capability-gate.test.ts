import assert from 'node:assert/strict';
import test from 'node:test';
import type { DaemonCapabilities } from '@/lib/assistant-protocol';
import {
  buildDiagnosticsText,
  canCancelTask,
  canInterject,
  canListTaskDepth,
  canListTasks,
  canRewind,
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
  assert.equal(hasMethod(c, 'run.rewind'), false);
});

test('canRewind requires run.rewind or run.rewindPreview', () => {
  assert.equal(canRewind(null), false);
  assert.equal(canRewind(caps(['run.start'])), false);
  assert.equal(canRewind(caps(['run.rewind'])), true);
  assert.equal(canRewind(caps(['run.rewindPreview'])), true);
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
