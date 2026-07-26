import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { FixtureAssistantAdapter } from '../assistant-gateway/fixture-adapter';
import { createDefaultGateway } from '../assistant-gateway';
import { goldenTextStream, goldenTool } from '../assistant-fixtures/golden';
import { createInitialWorkspaceState, workspaceReducer } from './reducer';
import {
  selectConversationMessages,
  selectActiveRun,
  selectIsRunActive,
} from './selectors';
import {
  connectWorkspace,
  loadConversations,
  openConversation,
  sendOrQueue,
  subscribeRun,
} from './controller';
import {
  loadPersistedDrafts,
  loadPersistedQuestionHistory,
  loadPersistedView,
  savePersistedDrafts,
  savePersistedQuestionHistory,
  savePersistedView,
} from './persistence';
import { createInitialWorkspaceState as initialFromState } from './state';

const workbenchSrc = readFileSync(
  fileURLToPath(new URL('../../components/assistant/AssistantWorkbench.tsx', import.meta.url)),
  'utf8',
);

test('createDefaultGateway(preferFixture) returns FixtureAssistantAdapter surface', () => {
  const g = createDefaultGateway(true);
  assert.equal(typeof g.connect, 'function');
  assert.equal(typeof g.subscribe, 'function');
  assert.equal(typeof g.getSnapshot, 'function');
  assert.equal(typeof g.request, 'function');
});

test('createDefaultGateway(false) fails closed without assistantV2', async () => {
  const g = createDefaultGateway(false);
  await assert.rejects(() => g.connect(), /assistantV2 not available/);
});

test('workbench source is composition-only (no v2call / streamChat / invent background status)', () => {
  assert.match(workbenchSrc, /AssistantStoreProvider/);
  assert.match(workbenchSrc, /data-gateway="1"/);
  assert.match(workbenchSrc, /sendOrQueue/);
  assert.match(workbenchSrc, /CommandPalette/);
  assert.equal(workbenchSrc.includes('function v2call'), false);
  assert.equal(/\bstreamChat\s*[:(]/.test(workbenchSrc), false);
  assert.equal(workbenchSrc.includes('browser-fallback'), false);
  assert.equal(workbenchSrc.includes("id: 'openai'"), false);
  assert.equal(workbenchSrc.includes("status: 'background_watching'"), false);
  assert.equal(workbenchSrc.includes('createAssistantStreamState'), false);
  assert.equal(workbenchSrc.includes('reduceAssistantStreamEvent'), false);
});

test('controller+gateway fixture path: connect → list → send → subscribe → timeline non-empty', async () => {
  const adapter = new FixtureAssistantAdapter(goldenTextStream);
  const actions: import('./state').WorkspaceAction[] = [];
  const dispatch = (a: import('./state').WorkspaceAction) => {
    actions.push(a);
    state = workspaceReducer(state, a);
  };
  let state = createInitialWorkspaceState();

  await connectWorkspace(adapter, dispatch);
  assert.equal(state.connection, 'connected');

  await loadConversations(adapter, dispatch);
  assert.ok(state.conversationOrder.includes('conv-1'));

  await openConversation(adapter, dispatch, 'conv-1');
  assert.equal(state.activeConversationId, 'conv-1');
  // Golden fixture conversation carries projectId — sendOrQueue must use it
  // without window.nativesAPI / readActiveProject.
  assert.ok(state.conversations['conv-1']?.projectId);

  const result = await sendOrQueue(adapter, dispatch, state, {
    conversationId: 'conv-1',
    content: 'hello workbench',
    providerId: 'openai',
    modelId: 'gpt-4o',
    // Omit projectPath to exercise conversation.projectId resolution path
  });
  assert.equal(result.queued, false);
  assert.ok(result.runId);

  await subscribeRun(adapter, dispatch, () => state, result.runId!, 0);
  assert.equal(state.runs[result.runId!]!.status, 'completed');
  const msgs = selectConversationMessages(state, 'conv-1');
  assert.ok(msgs.length >= 1);
  const text = msgs
    .flatMap((m) => m.contentBlocks)
    .filter((b) => b.type === 'text')
    .map((b) => b.text)
    .join('');
  assert.match(text, /Hello world/);
  assert.equal(selectIsRunActive(state, 'conv-1'), false);
});

test('sendOrQueue uses conversation.projectId when no window db (fixture main path)', async () => {
  const adapter = new FixtureAssistantAdapter(goldenTextStream);
  await adapter.connect();
  let state = createInitialWorkspaceState();
  const dispatch = (a: import('./state').WorkspaceAction) => {
    state = workspaceReducer(state, a);
  };
  await loadConversations(adapter, dispatch);
  // No projectPath param — must resolve from conversation.projectId
  const result = await sendOrQueue(adapter, dispatch, state, {
    conversationId: 'conv-1',
    content: 'path-resolve',
    providerId: 'openai',
    modelId: 'gpt-4o',
  });
  assert.ok(result.runId);
});

test('tool fixture produces single in-place tool block via shipped reducer', async () => {
  const adapter = new FixtureAssistantAdapter(goldenTool);
  await adapter.connect();
  const run = (await adapter.request('run.start', {
    conversation_id: 'conv-1',
    content: 'read',
  })) as { id: string };
  let state = createInitialWorkspaceState();
  state = workspaceReducer(state, {
    type: 'run/upsert',
    run: {
      id: run.id,
      conversationId: 'conv-1',
      status: 'running',
      providerId: 'openai',
      modelId: 'gpt-4o',
      permissionProfile: 'ask',
    },
  });
  const events = [];
  for await (const e of adapter.subscribe(run.id, 0)) events.push(e);
  state = workspaceReducer(state, { type: 'event/applyBatch', events });
  // Live path may still hold tools; after terminal promote, tools stay in events only.
  const liveTools = (state.liveByRun[run.id]?.blocks ?? []).filter((b) => b.type === 'tool_call');
  const msgTools = Object.values(state.messages)
    .flatMap((m) => m.contentBlocks)
    .filter((b) => b.type === 'tool_call');
  const eventTools = (state.eventsByRun[run.id] ?? []).filter((e) =>
    String(e.type).includes('tool_call') || String(e.type).includes('tool_'),
  );
  if (liveTools.length > 0) {
    assert.equal(liveTools.length, 1);
    assert.equal(liveTools[0]!.toolStatus, 'completed');
  } else {
    assert.equal(msgTools.length, 0, 'completed tools must not enter answer body');
    assert.ok(eventTools.length >= 1, 'tool lifecycle remains in eventsByRun');
  }
});

test('persistence helpers round-trip view and drafts (non-execution only)', () => {
  // jsdom-less node: polyfill localStorage
  const store = new Map<string, string>();
  const ls = {
    getItem: (k: string) => store.get(k) ?? null,
    setItem: (k: string, v: string) => {
      store.set(k, v);
    },
    removeItem: (k: string) => {
      store.delete(k);
    },
  };
  // @ts-expect-error test polyfill
  globalThis.window = { localStorage: ls };

  const view = {
    ...initialFromState().view,
    leftWidth: 280,
    rightWidth: 360,
    inspectorTab: 'artifacts' as const,
    rightCollapsed: true,
  };
  savePersistedView(view);
  const loaded = loadPersistedView();
  assert.equal(loaded?.leftWidth, 280);
  assert.equal(loaded?.rightWidth, 360);
  assert.equal(loaded?.inspectorTab, 'artifacts');
  assert.equal(loaded?.rightCollapsed, true);

  savePersistedDrafts({
    c1: { text: 'draft-hello', attachments: [], updatedAt: '2026-07-17T00:00:00Z' },
  });
  const drafts = loadPersistedDrafts();
  assert.equal(drafts.c1?.text, 'draft-hello');

  for (let index = 0; index < 101; index++) {
    savePersistedQuestionHistory('/project-a', `question-${index}`);
  }
  assert.deepEqual(loadPersistedQuestionHistory('/project-a'), Array.from({ length: 100 }, (_, index) => `question-${index + 1}`));
  assert.deepEqual(loadPersistedQuestionHistory('/project-b'), []);

  // cleanup
  // @ts-expect-error cleanup
  delete globalThis.window;
});

test('active run selector does not invent completion without terminal event', () => {
  let state = createInitialWorkspaceState();
  state = workspaceReducer(state, {
    type: 'run/upsert',
    run: {
      id: 'r1',
      conversationId: 'c1',
      status: 'running',
      providerId: 'openai',
      modelId: 'gpt-4o',
      permissionProfile: 'ask',
    },
  });
  state = workspaceReducer(state, {
    type: 'event/apply',
    event: {
      runId: 'r1',
      sequence: 1,
      timestamp: 't',
      type: 'text_delta',
      payload: { text: 'x' },
    },
  });
  assert.equal(selectActiveRun(state, 'c1')?.status, 'running');
  assert.notEqual(selectActiveRun(state, 'c1')?.status, 'completed');
});
