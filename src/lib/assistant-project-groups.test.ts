import assert from 'node:assert/strict';
import test from 'node:test';

import { classifyAssistantSurface, groupAssistantConversations, orderGroupsWithPins, projectCreationState } from './assistant-project-groups';

test('groups conversations by their registered project directory and hides no-project sessions', () => {
  const groups = groupAssistantConversations([
    { id: 'b', projectId: '/work/beta', updatedAt: '2026-07-12T10:00:00Z', title: 'B' },
    { id: 'a', projectId: '/work/alpha', updatedAt: '2026-07-12T11:00:00Z', title: 'A' },
    { id: 'u', projectId: '', updatedAt: '2026-07-12T12:00:00Z', title: 'U' },
  ], ['/work/alpha', '/work/beta']);
  assert.deepEqual(groups.map((g) => g.path), ['/work/alpha', '/work/beta']);
  assert.deepEqual(groups[0]!.conversations.map((c) => c.id), ['a']);
  assert.deepEqual(groups[1]!.conversations.map((c) => c.id), ['b']);
  assert.ok(!groups.some((g) => g.path === null), 'no unassigned bucket');
});

test('unregistered legacy UUID project_id is hidden, never a fake project node', () => {
  // A daemon session may carry a legacy project_id (a UUID) that never matches a
  // registered project. It must be hidden entirely — neither re-invented as a
  // project node nor surfaced in an unassigned bucket.
  const groups = groupAssistantConversations(
    [{ id: 'orphan', projectId: '79782adb-bbea-4e3e-8542-3a9002df1f2c', updatedAt: '2026-07-12T12:00:00Z', title: 'Orphan' }],
    ['/work/real'],
  );
  assert.equal(groups.length, 1, 'only the registered project remains');
  assert.equal(groups[0]!.path, '/work/real');
  assert.ok(!groups.some((g) => g.path === '79782adb-bbea-4e3e-8542-3a9002df1f2c'), 'UUID must not become a project path');
  assert.ok(!groups.some((g) => g.path === null), 'no unassigned bucket');
});

test('unregistered path project_id is hidden', () => {
  const groups = groupAssistantConversations(
    [{ id: 'orphan', projectId: '/project/test', updatedAt: '2026-07-12T12:00:00Z', title: 'Orphan' }],
    ['/work/real'],
  );
  assert.equal(groups.length, 1, 'only the registered project remains');
  assert.ok(!groups.some((g) => g.path === '/project/test'), 'unregistered path must not become a project path');
  assert.ok(!groups.some((g) => g.path === null), 'no unassigned bucket');
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
  ], ['/work/app']);
  assert.deepEqual(group?.conversations.map(conversation => conversation.id), ['newer', 'older']);
});

test('conversations whose registered project directory is gone are hidden', () => {
  const groups = groupAssistantConversations(
    [{ id: 'lost', projectId: '/Volumes/Offline/project', updatedAt: '2026-07-12T12:00:00Z', title: 'Lost' }],
    [{ path: '/Volumes/Offline/project', exists: false }],
  );
  assert.equal(groups.length, 0, 'physically-deleted project session is hidden entirely');
});

test('soft-deleted (hidden) project never reappears from daemon project_id', () => {
  // Product decision 1: project_remove only hides the project; daemon sessions
  // keep their project_id. The grouping must not re-invent the project node
  // (extras) nor list those sessions — they stay hidden until re-added.
  const groups = groupAssistantConversations(
    [
      { id: 'hidden-session', projectId: '/work/hidden', updatedAt: '2026-07-12T12:00:00Z', title: 'Hidden' },
      { id: 'visible-session', projectId: '/work/visible', updatedAt: '2026-07-12T11:00:00Z', title: 'Visible' },
    ],
    [{ path: '/work/visible', exists: true }],
    ['/work/hidden'],
  );
  assert.equal(groups.length, 1, 'hidden project must not create a group');
  assert.equal(groups[0]!.path, '/work/visible');
  assert.deepEqual(groups[0]!.conversations.map((conversation) => conversation.id), ['visible-session']);
});

test('hidden project sessions never fall into unassigned', () => {
  const groups = groupAssistantConversations(
    [{ id: 'hidden-session', projectId: '/work/hidden', updatedAt: '2026-07-12T12:00:00Z', title: 'Hidden' }],
    [],
    ['/work/hidden'],
  );
  assert.equal(groups.length, 0, 'hidden session must be filtered out entirely');
});

test('hidden project matches a daemon path with trailing slashes', () => {
  const groups = groupAssistantConversations(
    [{ id: 'legacy-session', projectId: '/work/hidden/', updatedAt: '2026-07-12T12:00:00Z', title: 'Legacy hidden' }],
    [],
    ['/work/hidden'],
  );
  assert.equal(groups.length, 0, 'equivalent path forms must not revive a hidden project');
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

test('projectId null and empty both hidden (no unassigned bucket)', () => {
  const result = groupAssistantConversations([
    { id: 'null-proj', projectId: null as unknown as string, updatedAt: '2026-07-12T12:00:00Z', title: 'Null' },
    { id: 'empty-proj', projectId: '', updatedAt: '2026-07-12T11:00:00Z', title: 'Empty' },
  ], ['/work/proj']);
  assert.equal(result.length, 1, 'only the registered project remains');
  assert.equal(result[0]!.path, '/work/proj');
  assert.equal(result[0]!.conversations.length, 0, 'no-project sessions are hidden');
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
  ], ['/work/app']);
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
