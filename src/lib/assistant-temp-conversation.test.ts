import assert from 'node:assert/strict';
import test from 'node:test';
import {
  collectTempConversationIds,
  conversationsWithoutTemp,
  createTempConversationId,
  createTempConversationShell,
  createTempSession,
  emptyTempDraft,
  isTempConversationId,
  omitTempKeys,
  resolveRegisteredProjectPath,
  TEMP_CONVERSATION_ID_PREFIX,
} from './assistant-temp-conversation';

test('isTempConversationId recognizes only temp-* shells', () => {
  assert.equal(isTempConversationId('temp-1'), true);
  assert.equal(isTempConversationId(`${TEMP_CONVERSATION_ID_PREFIX}123`), true);
  assert.equal(isTempConversationId('conv-1'), false);
  assert.equal(isTempConversationId(null), false);
  assert.equal(isTempConversationId(undefined), false);
  assert.equal(isTempConversationId(''), false);
});

test('createTempConversationId always uses temp- prefix', () => {
  const id = createTempConversationId(42);
  assert.equal(id, 'temp-42');
  assert.ok(isTempConversationId(id));
});

test('createTempConversationShell is local-only and allows empty provider', () => {
  const shell = createTempConversationShell({
    projectId: '/Users/me/proj',
    title: 'New conversation',
    id: 'temp-9',
    now: '2026-07-22T00:00:00.000Z',
  });
  assert.equal(shell.id, 'temp-9');
  assert.equal(shell.projectId, '/Users/me/proj');
  assert.equal(shell.providerId, '');
  assert.equal(shell.modelId, '');
  assert.equal(shell.permissionProfileId, 'ask');
  assert.equal(shell.mode, 'agent');
  assert.equal(shell.createdAt, '2026-07-22T00:00:00.000Z');
});

test('createTempSession includes empty draft (not long-term)', () => {
  const session = createTempSession({
    projectId: '/p',
    title: 't',
    id: 'temp-1',
    now: '2026-07-22T00:00:00.000Z',
  });
  assert.equal(session.conversation.id, 'temp-1');
  assert.deepEqual(session.draft, emptyTempDraft('2026-07-22T00:00:00.000Z'));
});

test('collectTempConversationIds keeps only temp shells', () => {
  assert.deepEqual(
    collectTempConversationIds(['conv-a', 'temp-1', 'temp-2', 'x']),
    ['temp-1', 'temp-2'],
  );
});

test('omitTempKeys strips temp drafts before long-term persist', () => {
  const drafts = {
    'conv-1': { text: 'keep', attachments: [], updatedAt: 't' },
    'temp-9': { text: 'ghost', attachments: [], updatedAt: 't' },
  };
  assert.deepEqual(omitTempKeys(drafts), {
    'conv-1': { text: 'keep', attachments: [], updatedAt: 't' },
  });
});

test('conversationsWithoutTemp excludes temp shells from sidebar groups', () => {
  const list = [
    { id: 'temp-1', title: 't' },
    { id: 'real-1', title: 'r' },
  ];
  assert.deepEqual(conversationsWithoutTemp(list), [{ id: 'real-1', title: 'r' }]);
});

test('resolveRegisteredProjectPath prefers host-normalized path', () => {
  assert.equal(
    resolveRegisteredProjectPath({ path: '/normalized/Proj' }, '/picker/Proj'),
    '/normalized/Proj',
  );
  assert.equal(
    resolveRegisteredProjectPath({ path: '  /n  ' }, '/picker'),
    '/n',
  );
  assert.equal(resolveRegisteredProjectPath(null, '/picker/only'), '/picker/only');
  assert.equal(resolveRegisteredProjectPath({ path: '' }, '/picker/only'), '/picker/only');
});

test('re-picking a project keeps only the latest temp id', () => {
  // Simulate consecutive project picks: drop previous temps, keep the new shell.
  let ids = ['conv-old', 'temp-100'];
  const next = createTempConversationShell({
    projectId: '/b',
    title: 'New',
    id: createTempConversationId(200),
  });
  ids = ids.filter((id) => !isTempConversationId(id)).concat(next.id);
  assert.deepEqual(ids, ['conv-old', 'temp-200']);
  assert.equal(collectTempConversationIds(ids).length, 1);
  assert.equal(collectTempConversationIds(ids)[0], 'temp-200');
});
