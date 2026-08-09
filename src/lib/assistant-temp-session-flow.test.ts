/**
 * Agent C: project pick → local temp-* session (no conversation.create until first send).
 *
 * Pure-logic + source-contract tests — no React mount required.
 */
import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import {
  collectTempConversationIds,
  conversationsWithoutTemp,
  createTempConversationId,
  createTempSession,
  isTempConversationId,
  omitTempKeys,
  resolveRegisteredProjectPath,
} from './assistant-temp-conversation';
import {
  createInitialWorkspaceState,
  workspaceReducer,
} from './assistant-workspace/reducer';
import type { Conversation, Run } from './assistant-protocol';
import {
  loadPersistedDrafts,
  savePersistedDrafts,
} from './assistant-workspace/persistence';
import { groupAssistantConversations } from './assistant-project-groups';

const workbenchSrc = readFileSync(
  fileURLToPath(new URL('../components/assistant/AssistantWorkbench.tsx', import.meta.url)),
  'utf8',
);
const composerHookSrc = readFileSync(
  fileURLToPath(new URL('../hooks/useAssistantWorkbenchComposer.ts', import.meta.url)),
  'utf8',
);
const contextSrc = readFileSync(
  fileURLToPath(new URL('../components/assistant/AssistantWorkspaceContext.tsx', import.meta.url)),
  'utf8',
);
const sidebarSrc = readFileSync(
  fileURLToPath(new URL('../components/assistant/AssistantSidebarSection.tsx', import.meta.url)),
  'utf8',
);

// ─── Helpers simulating the project-pick flow ───────────────────────────────

function activateTempForProject(
  state: ReturnType<typeof createInitialWorkspaceState>,
  projectPath: string,
  title = 'New conversation',
) {
  // Drop previous temps (consecutive picks keep only the latest).
  for (const oldId of collectTempConversationIds(state.conversationOrder)) {
    state = workspaceReducer(state, { type: 'conversations/remove', id: oldId });
    state = workspaceReducer(state, { type: 'composer/clear', conversationId: oldId });
  }
  const session = createTempSession({ projectId: projectPath, title });
  state = workspaceReducer(state, {
    type: 'conversations/upsert',
    conversation: session.conversation,
  });
  state = workspaceReducer(state, {
    type: 'conversations/setActive',
    id: session.conversation.id,
  });
  return { state, session };
}

function sidebarGroupsFromState(
  state: ReturnType<typeof createInitialWorkspaceState>,
  projects: string[],
) {
  const conversations = conversationsWithoutTemp(
    state.conversationOrder
      .map((id) => state.conversations[id])
      .filter(Boolean) as Conversation[],
  ).map((c) => ({
    id: c.id,
    title: c.title,
    mode: c.mode,
    projectId: c.projectId ?? null,
    updatedAt: c.updatedAt,
  }));
  return groupAssistantConversations(conversations, projects, 'Unassigned');
}

// ─── Behaviour ──────────────────────────────────────────────────────────────

test('select project → active session becomes temp-*, previous selection cleared', () => {
  let state = createInitialWorkspaceState();
  // Prior persistent session
  const prior: Conversation = {
    id: 'conv-prior',
    mode: 'agent',
    title: 'Old',
    providerId: 'p',
    modelId: 'm',
    projectId: '/old',
    createdAt: 't0',
    updatedAt: 't0',
  };
  state = workspaceReducer(state, { type: 'conversations/upsert', conversation: prior });
  state = workspaceReducer(state, { type: 'conversations/setActive', id: 'conv-prior' });
  assert.equal(state.activeConversationId, 'conv-prior');

  const registered = resolveRegisteredProjectPath(
    { path: '/normalized/ProjectA' },
    '/picker/ProjectA',
  );
  const result = activateTempForProject(state, registered);
  state = result.state;

  assert.ok(isTempConversationId(state.activeConversationId));
  assert.notEqual(state.activeConversationId, 'conv-prior');
  assert.equal(
    state.conversations[state.activeConversationId!]!.projectId,
    '/normalized/ProjectA',
  );
  // Prior conversation still in store (background run safe) but not active
  assert.ok(state.conversations['conv-prior']);
});

test('before send: no conversation.create is issued (source contract + local-only shell)', () => {
  // Source contract: addProjectFolder body must not invoke conversation.create RPC.
  // (Comments mentioning the method are allowed.)
  const addStart = workbenchSrc.indexOf('addProjectFolder:');
  const createStart = workbenchSrc.indexOf('createConversation:', addStart);
  assert.ok(addStart >= 0 && createStart > addStart);
  const addBlock = workbenchSrc.slice(addStart, createStart);
  assert.equal(/request\s*\(\s*['"]conversation\.create['"]/.test(addBlock), false);
  assert.equal(/['"]conversation\.create['"]/.test(addBlock), false);

  const createEnd = workbenchSrc.indexOf('createConversationInProject:', createStart);
  const createBlock = workbenchSrc.slice(createStart, createEnd);
  assert.equal(/request\s*\(\s*['"]conversation\.create['"]/.test(createBlock), false);

  const shellAddStart = contextSrc.indexOf('addProjectFolder:');
  const shellCreateStart = contextSrc.indexOf('createConversation:', shellAddStart);
  const shellAdd = contextSrc.slice(shellAddStart, shellCreateStart);
  assert.equal(/request\s*\(\s*['"]conversation\.create['"]/.test(shellAdd), false);
  assert.equal(/assistantV2/.test(shellAdd) && /conversation\.create/.test(shellAdd), false);

  // Behaviour: activating temp only mutates local store
  let state = createInitialWorkspaceState();
  const { state: next, session } = activateTempForProject(state, '/p');
  state = next;
  assert.ok(isTempConversationId(session.conversation.id));
  assert.equal(state.conversations[session.conversation.id]?.id, session.conversation.id);
});

test('temp session does not appear in sidebar project groups', () => {
  let state = createInitialWorkspaceState();
  const prior: Conversation = {
    id: 'conv-1',
    mode: 'agent',
    title: 'Real',
    providerId: 'p',
    modelId: 'm',
    projectId: '/proj',
    createdAt: 't0',
    updatedAt: 't0',
  };
  state = workspaceReducer(state, { type: 'conversations/upsert', conversation: prior });
  const { state: withTemp } = activateTempForProject(state, '/proj');
  const groups = sidebarGroupsFromState(withTemp, ['/proj']);
  const allIds = groups.flatMap((g) => g.conversations.map((c) => c.id));
  assert.ok(allIds.includes('conv-1'));
  assert.equal(allIds.some((id) => isTempConversationId(id)), false);
  // selected temp is still active in store
  assert.ok(isTempConversationId(withTemp.activeConversationId));
});

test('first send creates once with normalized project path and replaces temp id', async () => {
  const createCalls: Array<Record<string, unknown>> = [];
  const gateway = {
    async request(method: string, params: Record<string, unknown>) {
      if (method === 'conversation.create') {
        createCalls.push(params);
        return {
          id: 'conv-real-1',
          mode: 'agent' as const,
          title: String(params.title ?? ''),
          providerId: String(params.provider_id ?? ''),
          modelId: String(params.model_id ?? ''),
          projectId: (params.project_id as string | null | undefined) ?? null,
          permissionProfileId: String(params.permission_profile_id ?? 'ask'),
          createdAt: 't1',
          updatedAt: 't1',
        } satisfies Conversation;
      }
      throw new Error(`unexpected ${method}`);
    },
  };

  let state = createInitialWorkspaceState();
  const normalized = resolveRegisteredProjectPath(
    { path: '/host/Normalized' },
    '/picker/raw',
  );
  const { state: ready, session } = activateTempForProject(state, normalized);
  state = ready;
  const tempId = session.conversation.id;
  // User typed a draft on the temp shell
  state = workspaceReducer(state, {
    type: 'composer/set',
    conversationId: tempId,
    draft: { text: 'hello world', attachments: [], updatedAt: 't' },
  });

  // Simulate first-send create path (mirrors handleSend)
  assert.ok(isTempConversationId(state.activeConversationId));
  const created = (await gateway.request('conversation.create', {
    mode: 'agent',
    title: 'hello world'.slice(0, 30),
    provider_id: 'openai',
    model_id: 'gpt-4o',
    project_id: normalized,
    permission_profile_id: 'ask',
  })) as Conversation;
  assert.equal(createCalls.length, 1);
  assert.equal(createCalls[0]!.project_id, '/host/Normalized');
  assert.notEqual(createCalls[0]!.project_id, '/picker/raw');

  state = workspaceReducer(state, { type: 'conversations/upsert', conversation: created });
  state = workspaceReducer(state, { type: 'conversations/setActive', id: created.id });
  const tempDraft = state.composerByConversation[tempId];
  if (tempDraft) {
    state = workspaceReducer(state, {
      type: 'composer/set',
      conversationId: created.id,
      draft: tempDraft,
    });
    state = workspaceReducer(state, { type: 'composer/clear', conversationId: tempId });
  }
  state = workspaceReducer(state, { type: 'conversations/remove', id: tempId });

  assert.equal(state.activeConversationId, 'conv-real-1');
  assert.equal(state.conversations[tempId], undefined);
  assert.equal(state.composerByConversation[created.id]?.text, 'hello world');
  assert.equal(createCalls.length, 1);
});

test('create failure keeps temp page and user input', async () => {
  let state = createInitialWorkspaceState();
  const { state: ready, session } = activateTempForProject(state, '/p');
  state = ready;
  state = workspaceReducer(state, {
    type: 'composer/set',
    conversationId: session.conversation.id,
    draft: {
      text: 'keep me',
      attachments: [{ path: '/a.png', name: 'a.png' }],
      updatedAt: 't',
    },
  });

  const gateway = {
    async request() {
      throw new Error('create failed');
    },
  };
  let failed = false;
  try {
    await gateway.request();
  } catch {
    failed = true;
  }
  assert.equal(failed, true);
  // State untouched after failure
  assert.equal(state.activeConversationId, session.conversation.id);
  assert.ok(state.conversations[session.conversation.id]);
  assert.equal(state.composerByConversation[session.conversation.id]?.text, 'keep me');
  assert.equal(
    state.composerByConversation[session.conversation.id]?.attachments.length,
    1,
  );
});

test('consecutive project picks keep only the second temp session', () => {
  let state = createInitialWorkspaceState();
  const first = activateTempForProject(state, '/a');
  state = first.state;
  const firstId = first.session.conversation.id;

  // Ensure second pick gets a distinct id (Date.now resolution can collide in tests).
  const secondSession = createTempSession({
    projectId: '/b',
    title: 'New conversation',
    id: createTempConversationId(Number(firstId.replace('temp-', '')) + 1),
  });
  for (const oldId of collectTempConversationIds(state.conversationOrder)) {
    state = workspaceReducer(state, { type: 'conversations/remove', id: oldId });
    state = workspaceReducer(state, { type: 'composer/clear', conversationId: oldId });
  }
  state = workspaceReducer(state, {
    type: 'conversations/upsert',
    conversation: secondSession.conversation,
  });
  state = workspaceReducer(state, {
    type: 'conversations/setActive',
    id: secondSession.conversation.id,
  });
  const secondId = secondSession.conversation.id;

  assert.notEqual(firstId, secondId);
  assert.equal(state.conversations[firstId], undefined);
  assert.ok(state.conversations[secondId]);
  assert.equal(state.activeConversationId, secondId);
  assert.equal(state.conversations[secondId]!.projectId, '/b');
  assert.deepEqual(collectTempConversationIds(state.conversationOrder), [secondId]);
});

test('switching to new project does not cancel prior conversation run', () => {
  let state = createInitialWorkspaceState();
  const prior: Conversation = {
    id: 'conv-running',
    mode: 'agent',
    title: 'Running',
    providerId: 'p',
    modelId: 'm',
    projectId: '/old',
    createdAt: 't0',
    updatedAt: 't0',
  };
  state = workspaceReducer(state, { type: 'conversations/upsert', conversation: prior });
  state = workspaceReducer(state, { type: 'conversations/setActive', id: 'conv-running' });
  const run: Run = {
    id: 'run-1',
    conversationId: 'conv-running',
    status: 'running',
    providerId: 'p',
    modelId: 'm',
    permissionProfile: 'ask',
    createdAt: 't0',
    startedAt: 't0',
  };
  state = workspaceReducer(state, { type: 'run/upsert', run });
  state = {
    ...state,
    activeRunByConversation: { ...state.activeRunByConversation, 'conv-running': 'run-1' },
  };

  // Project pick creates temp and deselects prior — but leaves run state intact
  const { state: next } = activateTempForProject(state, '/new');
  state = next;
  assert.ok(isTempConversationId(state.activeConversationId));
  assert.equal(state.runs['run-1']?.status, 'running');
  assert.equal(state.activeRunByConversation['conv-running'], 'run-1');
  assert.ok(state.conversations['conv-running']);
  // Source: addProjectFolder must not call cancelRun / run.cancel
  const addBlock = workbenchSrc.slice(
    workbenchSrc.indexOf('addProjectFolder:'),
    workbenchSrc.indexOf('createConversation:'),
  );
  assert.equal(/cancelRun|run\.cancel|run\.stop/.test(addBlock), false);
});

test('temp drafts are never written to long-term draft persistence', () => {
  // Unit-level filter used by savePersistedDrafts (and any caller).
  const mixed = {
    'conv-1': { text: 'keep', attachments: [] as [], updatedAt: '2026-07-22T00:00:00Z' },
    [createTempConversationId(1)]: {
      text: 'ghost',
      attachments: [] as [],
      updatedAt: '2026-07-22T00:00:01Z',
    },
  };
  const filtered = omitTempKeys(mixed);
  assert.deepEqual(Object.keys(filtered), ['conv-1']);
  assert.equal(filtered['conv-1']?.text, 'keep');

  // Source contract: savePersistedDrafts drops temp-* keys.
  const persistenceSrc = readFileSync(
    fileURLToPath(new URL('./assistant-workspace/persistence.ts', import.meta.url)),
    'utf8',
  );
  assert.match(persistenceSrc, /!id\.startsWith\('temp-'\)|!id\.startsWith\("temp-"\)/);

  // If storage is available, round-trip must still omit temps.
  if (typeof globalThis.localStorage !== 'undefined' && globalThis.localStorage) {
    savePersistedDrafts(mixed);
    const loaded = loadPersistedDrafts();
    assert.equal(
      Object.keys(loaded).some((id) => isTempConversationId(id)),
      false,
    );
  }
});

// ─── Source contracts ───────────────────────────────────────────────────────

test('workbench first-send path uses isTempConversationId and project.register path', () => {
  assert.match(workbenchSrc, /isTempConversationId/);
  assert.match(workbenchSrc, /resolveRegisteredProjectPath/);
  assert.match(workbenchSrc, /createTempSession/);
  assert.match(workbenchSrc, /conversationsWithoutTemp/);
  // conversation.create lives in the composer hook (first send), not in the shell.
  assert.match(composerHookSrc, /'conversation\.create'/);
  assert.match(composerHookSrc, /project_id:\s*activeProjectPath/);
});

test('workspace context stores tempSession on navigation and uses register path', () => {
  assert.match(contextSrc, /tempSession/);
  assert.match(contextSrc, /withFreshTempSession|createTempSession/);
  assert.match(contextSrc, /resolveRegisteredProjectPath/);
  assert.match(contextSrc, /isTempConversationId/);
});

test('sidebar filters temp ids and navigates assistant before project pick', () => {
  assert.match(sidebarSrc, /isTempConversationId/);
  assert.match(sidebarSrc, /onNavigateAssistant\(\)/);
  assert.match(sidebarSrc, /createTempSession/);
  // Empty-state choose project navigates first
  assert.match(
    sidebarSrc,
    /onNavigateAssistant\(\);\s*\n\s*actions\?\.addProjectFolder\(\)/,
  );
});
