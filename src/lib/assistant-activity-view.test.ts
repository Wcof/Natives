import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import type { Artifact, FileChange, RunEvent } from '@/lib/assistant-protocol';
import {
  aggregateArtifactFiles,
  extractTodosFromEvents,
  mapSubagentUiStatus,
  normalizeArtifactPath,
  summarizeTodoStatus,
  type ActivityTodo,
} from './assistant-activity-view';

test('task list auto-load uses stable query inputs and reserves external refresh for the user action', () => {
  // R4-04: controller logic lives in the activity-inspector hook; the shell
  // only renders panels.
  const inspector = readFileSync(
    resolve(process.cwd(), 'src/components/assistant/activity-inspector/useActivityInspector.ts'),
    'utf8',
  );

  assert.match(inspector, /const runId = run\?\.id \?\? null;/);
  assert.match(inspector, /\}, \[useTaskList, gateway, runId, conversationId\]\);/);
  assert.match(inspector, /\}, \[effectiveTab, useTaskList, refreshTasks\]\);/);
  assert.equal(inspector.includes('onRefreshTasks?.();\n    if (!useTaskList'), false);
  assert.match(inspector, /const handleRefreshTasks = useCallback\(/);
});

function ev(
  sequence: number,
  type: string,
  payload: Record<string, unknown>,
  runId = 'run-1',
): RunEvent {
  return {
    runId,
    sequence,
    timestamp: `2026-07-23T00:00:${String(sequence).padStart(2, '0')}Z`,
    type,
    payload,
  };
}

test('normalizeArtifactPath collapses separators and trailing slash', () => {
  assert.equal(normalizeArtifactPath('  src//a/b/  '), 'src/a/b');
  assert.equal(normalizeArtifactPath('C:\\foo\\bar'), 'C:/foo/bar');
  assert.equal(normalizeArtifactPath('/'), '/');
});

test('extractTodosFromEvents: latest todo_write wins', () => {
  const events: RunEvent[] = [
    ev(1, 'tool_call_requested', {
      id: 't1',
      name: 'todo_write',
      input: {
        todos: [
          { id: 'a', content: 'Old A', status: 'pending' },
          { id: 'b', content: 'Old B', status: 'pending' },
        ],
      },
    }),
    ev(2, 'tool_call_completed', {
      id: 't1',
      name: 'todo_write',
      output: {
        ok: true,
        todos: [
          { id: 'a', content: 'Old A', status: 'completed' },
          { id: 'b', content: 'Old B', status: 'in_progress' },
        ],
      },
    }),
    ev(3, 'tool_call_requested', {
      id: 't2',
      name: 'TodoWrite',
      input: {
        todos: [
          { id: 'a', content: 'A done', status: 'completed' },
          { id: 'b', content: 'B running', status: 'in_progress' },
          { id: 'c', content: 'C pending', status: 'pending' },
        ],
      },
    }),
    ev(4, 'tool_call_completed', { id: 'x', name: 'read_file', output: 'ok' }),
  ];

  const todos = extractTodosFromEvents(events);
  assert.equal(todos.length, 3);
  assert.equal(todos[0]!.content, 'A done');
  assert.equal(todos[0]!.status, 'completed');
  assert.equal(todos[1]!.status, 'in_progress');
  assert.equal(todos[2]!.status, 'pending');
});

test('extractTodosFromEvents: empty when no todo_write', () => {
  assert.deepEqual(
    extractTodosFromEvents([
      ev(1, 'text_delta', { text: 'hi' }),
      ev(2, 'tool_call_completed', { id: '1', name: 'read_file', output: 'x' }),
    ]),
    [],
  );
});

test('summarizeTodoStatus: priority in_progress > completed > pending', () => {
  const mix: ActivityTodo[] = [
    { id: '1', content: 'a', status: 'completed' },
    { id: '2', content: 'b', status: 'pending' },
  ];
  assert.equal(summarizeTodoStatus(mix), 'pending');
  assert.equal(
    summarizeTodoStatus([
      { id: '1', content: 'a', status: 'completed' },
      { id: '2', content: 'b', status: 'in_progress' },
    ]),
    'in_progress',
  );
  assert.equal(
    summarizeTodoStatus([
      { id: '1', content: 'a', status: 'completed' },
      { id: '2', content: 'b', status: 'completed' },
    ]),
    'completed',
  );
  assert.equal(summarizeTodoStatus([]), 'pending');
});

test('aggregateArtifactFiles: created / modified buckets', () => {
  const fileChanges: FileChange[] = [
    { path: 'src/new.ts', changeType: 'created', runId: 'r1' },
    { path: 'src/edit.ts', changeType: 'modified', runId: 'r1' },
  ];
  const { created, modified } = aggregateArtifactFiles({ fileChanges });
  assert.deepEqual(
    created.map((c) => c.path),
    ['src/new.ts'],
  );
  assert.deepEqual(
    modified.map((m) => m.path),
    ['src/edit.ts'],
  );
});

test('aggregateArtifactFiles: created wins over later modified (no duplicate)', () => {
  const fileEvents = [
    { path: '/proj/a.ts', changeType: 'created' as const, at: 't1', runId: 'r1' },
    { path: '/proj/a.ts', changeType: 'modified' as const, at: 't2', runId: 'r1' },
    { path: '/proj//a.ts', changeType: 'modified' as const, at: 't3', runId: 'r2' },
  ];
  const { created, modified } = aggregateArtifactFiles({ fileEvents });
  assert.equal(created.length, 1);
  assert.equal(modified.length, 0);
  assert.equal(created[0]!.path, '/proj/a.ts');
  assert.equal(created[0]!.at, 't3');
  assert.equal(created[0]!.runId, 'r2');
});

test('aggregateArtifactFiles: duplicate events keep latest metadata', () => {
  const events: RunEvent[] = [
    ev(1, 'file_changed', { path: 'x.md', change_type: 'modified' }, 'r1'),
    ev(2, 'file_changed', { path: 'x.md', change_type: 'modified' }, 'r1'),
  ];
  const { created, modified } = aggregateArtifactFiles({ events });
  assert.equal(created.length, 0);
  assert.equal(modified.length, 1);
  assert.equal(modified[0]!.at, '2026-07-23T00:00:02Z');
});

test('aggregateArtifactFiles: child-run file events merge by path', () => {
  const events: RunEvent[] = [
    ev(1, 'file_changed', { path: 'child/out.ts', change_type: 'created' }, 'child-run'),
    ev(2, 'file_changed', { path: 'parent/out.ts', change_type: 'modified' }, 'parent-run'),
  ];
  const fileChanges: FileChange[] = [
    { path: 'parent/out.ts', changeType: 'modified', runId: 'parent-run' },
  ];
  const { created, modified } = aggregateArtifactFiles({ events, fileChanges });
  assert.equal(created.map((c) => c.path).join(','), 'child/out.ts');
  assert.equal(modified.map((m) => m.path).join(','), 'parent/out.ts');
});

test('aggregateArtifactFiles: path-bearing artifact defaults to created', () => {
  const artifacts: Artifact[] = [
    {
      id: 'a1',
      runId: 'r1',
      path: 'reports/summary.html',
      kind: 'report',
      size: 10,
      createdAt: 't9',
    },
  ];
  const { created, modified } = aggregateArtifactFiles({ artifacts });
  assert.equal(modified.length, 0);
  assert.equal(created.length, 1);
  assert.equal(created[0]!.path, 'reports/summary.html');
  assert.equal(created[0]!.changeType, 'created');
});

test('mapSubagentUiStatus labels', () => {
  assert.equal(mapSubagentUiStatus('pending_assignment').key, 'pending_assignment');
  assert.equal(mapSubagentUiStatus('queued').key, 'in_progress');
  assert.equal(mapSubagentUiStatus('open').key, 'in_progress');
  assert.equal(mapSubagentUiStatus('running').key, 'in_progress');
  assert.equal(mapSubagentUiStatus('waiting_permission').key, 'in_progress');
  assert.equal(mapSubagentUiStatus('completed').key, 'completed');
  assert.equal(mapSubagentUiStatus('idle').key, 'completed');
  assert.equal(mapSubagentUiStatus('failed').key, 'closed');
  assert.equal(mapSubagentUiStatus('cancelled').key, 'closed');
  assert.equal(mapSubagentUiStatus('interrupted').key, 'closed');
  assert.equal(mapSubagentUiStatus('closed').key, 'closed');
});
