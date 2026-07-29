import assert from 'node:assert/strict';
import test from 'node:test';

import { classifyAssistantSurface, groupAssistantConversations, orderGroupsWithPins, projectCreationState } from './assistant-project-groups';

test('groups conversations by their real project directory and keeps unassigned data explicit', () => {
  const groups = groupAssistantConversations([
    { id: 'b', projectId: '/work/beta', updatedAt: '2026-07-12T10:00:00Z', title: 'B' },
    { id: 'a', projectId: '/work/alpha', updatedAt: '2026-07-12T11:00:00Z', title: 'A' },
    { id: 'u', projectId: '', updatedAt: '2026-07-12T12:00:00Z', title: 'U' },
  ]);
  assert.deepEqual(groups.map((g) => g.path), ['/work/alpha', '/work/beta', null]);
  assert.deepEqual(groups[0]!.conversations.map((c) => c.id), ['a']);
  assert.deepEqual(groups[1]!.conversations.map((c) => c.id), ['b']);
  assert.deepEqual(groups[2]!.conversations.map((c) => c.id), ['u']);
  assert.equal(groups[2]!.label, 'Unassigned');
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

test('hidden subagent child conversations stay out of the sidebar groups', () => {
  const result = groupAssistantConversations(
    [
      { id: 'root-a', projectId: '/work/app', updatedAt: '2026-07-12T10:00:00Z', title: 'Root A' },
      { id: 'child-a', projectId: '/work/app', updatedAt: '2026-07-12T11:00:00Z', title: 'Child A', parentConversationId: 'root-a' },
    ],
    ['/work/app'],
  );
  assert.deepEqual(result[0]?.conversations.map((c) => c.id), ['root-a']);
});

test('sorts conversations inside a project by latest activity', () => {
  const [group] = groupAssistantConversations([
    { id: 'older', projectId: '/work/app', updatedAt: '2026-07-12T10:00:00Z', title: 'Older' },
    { id: 'newer', projectId: '/work/app', updatedAt: '2026-07-12T11:00:00Z', title: 'Newer' },
  ]);
  assert.deepEqual(group?.conversations.map(conversation => conversation.id), ['newer', 'older']);
});

test('conversations whose registered project directory is gone move to unassigned', () => {
  const groups = groupAssistantConversations(
    [{ id: 'lost', projectId: '/Volumes/Offline/project', updatedAt: '2026-07-12T12:00:00Z', title: 'Lost' }],
    [{ path: '/Volumes/Offline/project', exists: false }],
  );
  assert.equal(groups.length, 1);
  assert.equal(groups[0]!.path, null);
  assert.deepEqual(groups[0]!.conversations.map((conversation) => conversation.id), ['lost']);
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


test('registered project order follows backend list, not latest session', () => {
  const result = groupAssistantConversations(
    [
      { id: 'old-in-alpha', projectId: '/work/alpha', updatedAt: '2026-07-12T09:00:00Z', title: 'Old' },
      { id: 'new-in-beta', projectId: '/work/beta', updatedAt: '2026-07-12T12:00:00Z', title: 'New' },
    ],
    [
      { path: '/work/alpha', lastOpenedAt: '2026-07-12T13:00:00Z' },
      { path: '/work/beta', lastOpenedAt: '2026-07-12T10:00:00Z' },
    ],
  );
  assert.equal(result[0]!.path, '/work/alpha');
  assert.equal(result[1]!.path, '/work/beta');
});

test('pinned conversations float within their project only', () => {
  const [group] = groupAssistantConversations([
    { id: 'older-pin', projectId: '/work/app', updatedAt: '2026-07-12T09:00:00Z', title: 'Older', pinned: true },
    { id: 'newer', projectId: '/work/app', updatedAt: '2026-07-12T11:00:00Z', title: 'Newer' },
  ]);
  assert.deepEqual(group?.conversations.map((c) => c.id), ['older-pin', 'newer']);
});

test('orderGroupsWithPins keeps relative order within buckets', () => {
  const groups = groupAssistantConversations(
    [],
    ['/a', '/b', '/c'],
  );
  const ordered = orderGroupsWithPins(groups, ['/c', '/a']);
  assert.deepEqual(ordered.map((g) => g.path), ['/a', '/c', '/b']);
});
