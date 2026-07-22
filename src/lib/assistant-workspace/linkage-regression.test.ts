/**
 * Regression harness for assistant shell ↔ workbench linkage bugs reported 2026-07-21:
 * - first paint must not depend on workbench mount for project folders
 * - send failure must not leave a permanent live "thinking" bubble
 * - prompt queue / goal chrome must not activate for ordinary agent turns
 * - conversation.delete not-found still removes local row
 */
import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import {
  createInitialWorkspaceState,
  workspaceReducer,
} from './reducer';
import {
  selectActiveRun,
  selectConversationMessages,
  selectIsRunActive,
  selectPromptQueue,
} from './selectors';
import { sendOrQueue, subscribeRun } from './controller';
import type { AssistantGateway } from '@/lib/assistant-gateway';
import type { Run, RunEvent } from '@/lib/assistant-protocol';
import { groupAssistantConversations } from '@/lib/assistant-project-groups';

const providerSrc = readFileSync(
  fileURLToPath(new URL('../../components/assistant/AssistantWorkspaceContext.tsx', import.meta.url)),
  'utf8',
);
const workbenchSrc = readFileSync(
  fileURLToPath(new URL('../../components/assistant/AssistantWorkbench.tsx', import.meta.url)),
  'utf8',
);
const sidebarSrc = readFileSync(
  fileURLToPath(new URL('../../components/assistant/AssistantSidebarSection.tsx', import.meta.url)),
  'utf8',
);
const hostServiceSrc = readFileSync(
  fileURLToPath(new URL('../../../src-tauri/src/assistant_service.rs', import.meta.url)),
  'utf8',
);

test('provider seeds projects+sessions before workbench: refreshNavigationFromHost + shell actions always on', () => {
  assert.match(providerSrc, /refreshNavigationFromHost/);
  assert.match(providerSrc, /conversation\.list/);
  assert.match(providerSrc, /project\.list/);
  assert.match(providerSrc, /shellActions/);
  assert.match(providerSrc, /workbenchActions/);
  // Must not clear shell actions when workbench unmounts.
  assert.match(providerSrc, /actions: shellActions/);
});

test('host conversation.* is never dual-routed to UDS daemon', () => {
  assert.match(hostServiceSrc, /if method\.starts_with\("conversation\."\)/);
  assert.match(hostServiceSrc, /return false;/);
  // Old dual-store routing must not return.
  assert.equal(
    /method\.starts_with\("conversation\."\) && daemon_authority::authority_mode_label\(\) == "uds"/.test(
      hostServiceSrc,
    ),
    false,
  );
});

test('remove project confirm button is hard-coded zh/en, not raw i18n key', () => {
  assert.match(sidebarSrc, /zh \? '移除' : 'Remove'/);
  assert.equal(sidebarSrc.includes("t(locale, 'common.remove')"), false);
});

test('goal chrome only when conversation.mode === goal; ordinary send uses agent mode', () => {
  assert.match(workbenchSrc, /activeConversation\?\.mode === 'goal'/);
  assert.match(workbenchSrc, /mode: 'agent'/);
  // Prompt queue hidden for goal mode (not used as goal chrome substitute).
  assert.match(workbenchSrc, /items=\{isGoalMode \? \[\] : promptQueue\}/);
  // Ordinary chat/agent: no RunStatusBar — progress is timeline + input stop only.
  assert.equal(workbenchSrc.includes("import RunStatusBar"), false);
  assert.equal(workbenchSrc.includes('<RunStatusBar'), false);
  assert.match(workbenchSrc, /GoalStatusBar/);
});

test('groupAssistantConversations seeds empty registered projects on first paint', () => {
  const groups = groupAssistantConversations([], ['/Users/me/alpha', '/Users/me/beta'], '未关联项目');
  assert.equal(groups.length, 2);
  assert.deepEqual(
    groups.map((g) => g.path).sort(),
    ['/Users/me/alpha', '/Users/me/beta'],
  );
  assert.ok(groups.every((g) => g.conversations.length === 0));
});

test('sendOrQueue failure clears optimistic user message and does not leave active run', async () => {
  let state = createInitialWorkspaceState();
  const dispatch = (a: Parameters<typeof workspaceReducer>[1]) => {
    state = workspaceReducer(state, a);
  };
  state = workspaceReducer(state, {
    type: 'conversations/upsert',
    conversation: {
      id: 'c1',
      mode: 'agent',
      title: 't',
      providerId: 'p',
      modelId: 'm',
      projectId: '/proj',
      permissionProfileId: 'ask',
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
    },
  });
  state = workspaceReducer(state, { type: 'conversations/setActive', id: 'c1' });

  const gateway: AssistantGateway = {
    connect: async () => undefined,
    disconnect: async () => undefined,
    getCapabilities: async () => null,
    request: async (method) => {
      if (method === 'run.start') throw new Error('engine down');
      throw new Error(`unexpected ${method}`);
    },
    subscribe: async function* () {
      /* empty */
    },
    getSnapshot: async () => {
      throw new Error('no');
    },
  };

  await assert.rejects(
    () =>
      sendOrQueue(gateway, dispatch, state, {
        conversationId: 'c1',
        content: 'hello',
        providerId: 'p',
        modelId: 'm',
        projectPath: '/proj',
      }),
    /engine down/,
  );

  assert.equal(selectActiveRun(state, 'c1'), null);
  assert.equal(selectIsRunActive(state, 'c1'), false);
  const msgs = selectConversationMessages(state, 'c1');
  assert.equal(
    msgs.filter((m) => m.status === 'sending' || m.status === 'streaming').length,
    0,
  );
});

test('subscribeRun ending without terminal event stays quiet (no reconnect banner)', async () => {
  let state = createInitialWorkspaceState();
  const dispatch = (a: Parameters<typeof workspaceReducer>[1]) => {
    state = workspaceReducer(state, a);
  };
  const run: Run = {
    id: 'r1',
    conversationId: 'c1',
    status: 'preparing',
    providerId: 'p',
    modelId: 'm',
    permissionProfile: 'ask',
    startedAt: new Date().toISOString(),
    retryCount: 0,
    lastEventSequence: 0,
  };
  state = workspaceReducer(state, { type: 'run/upsert', run });
  // Simulate a connected workspace so banner should stay hidden.
  state = workspaceReducer(state, {
    type: 'connection/set',
    connection: 'connected',
    error: null,
  });

  const gateway: AssistantGateway = {
    connect: async () => undefined,
    disconnect: async () => undefined,
    getCapabilities: async () => null,
    request: async () => [],
    subscribe: async function* (): AsyncGenerator<RunEvent> {
      // empty stream — engine silence / poll boundary
    },
    getSnapshot: async () => {
      throw new Error('no');
    },
  };

  await subscribeRun(gateway, dispatch, () => state, 'r1', 0);
  // Must stay active; only daemon terminals may fail the run.
  assert.equal(state.runs.r1?.status, 'preparing');
  assert.notEqual(state.runs.r1?.errorCode, 'SUBSCRIBE_ENDED');
  // Quiet empty end must not flip global connection or recovering banners.
  assert.equal(state.connection, 'connected');
  assert.equal(state.recoveringRuns.r1, undefined);
  assert.equal(
    state.connectionError,
    null,
    'must not show "Subscribe ended without terminal event" banner',
  );
});

test('prompt queue empty for non-busy conversation (ordinary send does not invent queue chrome)', () => {
  let state = createInitialWorkspaceState();
  state = workspaceReducer(state, {
    type: 'conversations/upsert',
    conversation: {
      id: 'c1',
      mode: 'agent',
      title: 't',
      providerId: 'p',
      modelId: 'm',
      projectId: '/proj',
      permissionProfileId: 'ask',
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
    },
  });
  assert.deepEqual(selectPromptQueue(state, 'c1'), []);
});
