import assert from 'node:assert/strict';
import test from 'node:test';
import {
  listExtensions,
  listMcp,
  listScheduler,
  listSkills,
  loadCapabilityAdminDashboard,
  searchMemory,
} from './capability-admin';
import type { AssistantGateway } from '@/lib/assistant-gateway';

function mockGateway(handlers: Record<string, unknown>): AssistantGateway {
  return {
    async connect() {},
    async disconnect() {},
    async getCapabilities() {
      return null;
    },
    async request<T>(method: string): Promise<T> {
      if (!(method in handlers)) throw new Error(`unexpected method ${method}`);
      return handlers[method] as T;
    },
    async *subscribe() {
      // no events
    },
    async getSnapshot() {
      return {
        conversation: {
          id: 'c',
          mode: 'agent' as const,
          title: 't',
          providerId: 'openai',
          modelId: 'm',
          projectId: '/p',
          createdAt: 't',
          updatedAt: 't',
        },
        messages: [],
        runs: [],
        activeRunId: null,
        eventsByRun: {},
        artifacts: [],
        interactions: [],
        capabilities: null,
      };
    },
  };
}

test('listMcp maps servers/tools/namespaced from mcp.list', async () => {
  const g = mockGateway({
    'mcp.list': {
      servers: [{ id: 's1' }],
      tools: [{ name: 't1' }],
      namespaced: [['mcp__s1__t1', 'd', {}]],
    },
  });
  const snap = await listMcp(g);
  assert.equal(snap.servers.length, 1);
  assert.equal(snap.tools.length, 1);
  assert.equal(snap.namespaced.length, 1);
});

test('listScheduler accepts array or jobs wrapper', async () => {
  const a = await listScheduler(mockGateway({ 'scheduler.list': [{ id: 'j1' }] }));
  assert.equal(a.jobs.length, 1);
  const b = await listScheduler(mockGateway({ 'scheduler.list': { jobs: [{ id: 'j2' }] } }));
  assert.equal(b.jobs.length, 1);
});

test('listExtensions and listSkills via gateway only', async () => {
  const g = mockGateway({
    'extension.list': { extensions: [{ id: 'e1', enabled: true }] },
    'skill.list': [{ name: 'sk' }],
  });
  assert.equal((await listExtensions(g)).extensions.length, 1);
  assert.equal((await listSkills(g)).skills.length, 1);
});

test('searchMemory returns results array', async () => {
  const g = mockGateway({
    'memory.search': { results: [{ id: 'm1', text: 'hi' }] },
  });
  const snap = await searchMemory(g, 'hi');
  assert.equal(snap.results.length, 1);
});

test('loadCapabilityAdminDashboard fans out Phase6 RPC methods', async () => {
  const methods: string[] = [];
  const g: AssistantGateway = {
    async connect() {},
    async disconnect() {},
    async getCapabilities() {
      return null;
    },
    async request<T>(method: string): Promise<T> {
      methods.push(method);
      if (method === 'mcp.list') return { servers: [], tools: [], namespaced: [] } as T;
      if (method === 'scheduler.list') return [] as T;
      if (method === 'extension.list') return [] as T;
      if (method === 'skill.list') return [] as T;
      throw new Error(method);
    },
    async *subscribe() {},
    async getSnapshot() {
      throw new Error('unused');
    },
  };
  const dash = await loadCapabilityAdminDashboard(g);
  assert.deepEqual(methods.sort(), [
    'extension.list',
    'mcp.list',
    'scheduler.list',
    'skill.list',
  ].sort());
  assert.ok(dash.mcp);
  assert.ok(dash.scheduler);
  assert.ok(dash.extensions);
  assert.ok(dash.skills);
});

test('capability admin never calls streamChat or window.nativesAPI', async () => {
  const fs = await import('node:fs');
  const path = await import('node:path');
  const srcPath = path.join(process.cwd(), 'src/lib/assistant-workspace/capability-admin.ts');
  const src = fs.readFileSync(srcPath, 'utf8');
  assert.equal(/\bstreamChat\s*[:(]/.test(src), false);
  assert.equal(/window\.nativesAPI/.test(src), false);
  assert.equal(/\.streamChat\s*\(/.test(src), false);
  assert.match(src, /mcp\.list/);
  assert.match(src, /scheduler\.list/);
  assert.match(src, /gateway\.request/);
});
