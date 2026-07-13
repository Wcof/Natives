import assert from 'node:assert/strict';
import test from 'node:test';

import { classifyAssistantSurface, groupAssistantConversations, projectCreationState } from './assistant-project-groups';

test('groups conversations by their real project directory and keeps unassigned data explicit', () => {
  assert.deepEqual(
    groupAssistantConversations([
      { id: 'b', projectId: '/work/beta', updatedAt: '2026-07-12T10:00:00Z', title: 'B' },
      { id: 'a', projectId: '/work/alpha', updatedAt: '2026-07-12T11:00:00Z', title: 'A' },
      { id: 'u', projectId: '', updatedAt: '2026-07-12T12:00:00Z', title: 'U' },
    ]),
    [
      {
        id: '/work/alpha',
        path: '/work/alpha',
        label: 'alpha',
        conversations: [{ id: 'a', projectId: '/work/alpha', updatedAt: '2026-07-12T11:00:00Z', title: 'A' }],
      },
      {
        id: '/work/beta',
        path: '/work/beta',
        label: 'beta',
        conversations: [{ id: 'b', projectId: '/work/beta', updatedAt: '2026-07-12T10:00:00Z', title: 'B' }],
      },
      {
        id: '__unassigned__',
        path: null,
        label: 'Unassigned',
        conversations: [{ id: 'u', projectId: '', updatedAt: '2026-07-12T12:00:00Z', title: 'U' }],
      },
    ],
  );
});

test('registered projects appear even without conversations', () => {
  const result = groupAssistantConversations(
    [],
    ['/work/alpha', '/work/beta'],
  );
  assert.equal(result.length, 2);
  assert.equal(result[0]!.path, '/work/alpha');
  assert.equal(result[0]!.conversations.length, 0);
  assert.equal(result[1]!.path, '/work/beta');
  assert.equal(result[1]!.conversations.length, 0);
});

test('registered projects with conversations merge correctly', () => {
  const result = groupAssistantConversations(
    [{ id: 'c1', projectId: '/work/alpha', updatedAt: '2026-07-12T10:00:00Z', title: 'Conv1' }],
    ['/work/alpha', '/work/beta'],
  );
  assert.equal(result.length, 2);
  const alpha = result.find(g => g.path === '/work/alpha')!;
  assert.equal(alpha.conversations.length, 1);
  assert.equal(alpha.conversations[0]!.id, 'c1');
  const beta = result.find(g => g.path === '/work/beta')!;
  assert.equal(beta.conversations.length, 0);
});

test('sorts conversations inside a project by latest activity', () => {
  const [group] = groupAssistantConversations([
    { id: 'older', projectId: '/work/app', updatedAt: '2026-07-12T10:00:00Z', title: 'Older' },
    { id: 'newer', projectId: '/work/app', updatedAt: '2026-07-12T11:00:00Z', title: 'Newer' },
  ]);
  assert.deepEqual(group?.conversations.map(conversation => conversation.id), ['newer', 'older']);
});

test('no project is not an error — engine and provider ready gives ready', () => {
  assert.equal(projectCreationState({ engine: 'ready', providerReadiness: 'ready' }), 'ready');
});

test('engine unavailable still blocks creation', () => {
  assert.equal(projectCreationState({ engine: 'unavailable', providerReadiness: 'ready' }), 'engine_unavailable');
});

test('no provider still blocks creation even without project check', () => {
  assert.equal(projectCreationState({ engine: 'ready', providerReadiness: 'no_provider' }), 'provider_needed');
});

test('no model still blocks creation even without project check', () => {
  assert.equal(projectCreationState({ engine: 'ready', providerReadiness: 'no_model' }), 'model_needed');
});

test('keeps page, engine, and provider setup states distinct', () => {
  assert.equal(classifyAssistantSurface({ bridge: false }), 'renderer_only');
  assert.equal(classifyAssistantSurface({ bridge: true, engine: 'connecting' }), 'connecting_engine');
  assert.equal(classifyAssistantSurface({ bridge: true, engine: 'failed' }), 'engine_unavailable');
  assert.equal(classifyAssistantSurface({ bridge: true, engine: 'ready', provider: 'no_provider' }), 'provider_needed');
  assert.equal(classifyAssistantSurface({ bridge: true, engine: 'ready', provider: 'no_model' }), 'model_needed');
  assert.equal(classifyAssistantSurface({ bridge: true, engine: 'ready', provider: 'ready' }), 'ready');
});

test('unassigned group always sorted last', () => {
  const result = groupAssistantConversations([
    { id: 'u1', projectId: '', updatedAt: '2026-07-12T12:00:00Z', title: 'Unassigned' },
    { id: 'p1', projectId: '/work/proj', updatedAt: '2026-07-12T10:00:00Z', title: 'Project' },
  ]);
  assert.equal(result.length, 2);
  // Unassigned should be last
  assert.equal(result[1]!.path, null);
});

test('projectId null and empty both treated as unassigned', () => {
  const result = groupAssistantConversations([
    { id: 'null-proj', projectId: null as unknown as string, updatedAt: '2026-07-12T12:00:00Z', title: 'Null' },
    { id: 'empty-proj', projectId: '', updatedAt: '2026-07-12T11:00:00Z', title: 'Empty' },
  ]);
  const unassignedGroup = result.find(g => g.path === null)!;
  assert.equal(unassignedGroup.conversations.length, 2);
});
