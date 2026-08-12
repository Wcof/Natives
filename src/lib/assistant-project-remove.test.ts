// W1-A #1（审计收口）：项目软删生产链红灯测试。
// 真实调用生产函数 removeProjectFromHost（唯一 authority），Host fake 返回
// 新的 hidden set，Daemon 仍返回旧会话；断言项目/会话立即消失、重挂载仍隐藏、
// 重新添加恢复、删除不存在失败、active 不停留在隐藏会话。
import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { removeProjectFromHost } from './assistant-project-remove';
import { groupAssistantConversations } from '@/lib/assistant-project-groups';

interface HostState {
  visible: Array<{ id: string; path: string }>;
  hidden: string[];
  removeCalls: Array<string | null>;
}

function makeHost(initial: { visible: string[]; hidden: string[] }): HostState & {
  api: {
    list(): Promise<Array<{ id: string; path: string }> | null>;
    listHidden(): Promise<string[] | null>;
    remove(id: string): Promise<void>;
  };
  softRemove: (path: string) => void;
  reAdd: (path: string) => void;
} {
  const state: HostState = { visible: [], hidden: [], removeCalls: [] };
  state.visible = initial.visible.map((p, i) => ({ id: `id-${i}`, path: p }));
  state.hidden = [...initial.hidden];
  return {
    ...state,
    api: {
      list: async () => state.visible.map((p) => ({ ...p })),
      listHidden: async () => [...state.hidden],
      remove: async (id: string | null) => {
        state.removeCalls.push(id);
        const idx = state.visible.findIndex((p) => p.id === id || p.path === id);
        if (idx === -1) throw new Error('Project not found');
        state.hidden.push(state.visible[idx]!.path);
        state.visible.splice(idx, 1);
      },
    },
    softRemove(path: string) {
      const idx = state.visible.findIndex((p) => p.path === path);
      if (idx !== -1) {
        state.hidden.push(path);
        state.visible.splice(idx, 1);
      }
    },
    reAdd(path: string) {
      state.visible.push({ id: `id-${state.visible.length}`, path });
      // 原地修改同一数组引用，避免外部已持有旧引用。
      state.hidden.splice(0, state.hidden.length, ...state.hidden.filter((p) => p !== path));
    },
  };
}

describe('W1-A #1 项目软删生产链（removeProjectFromHost）', () => {
  it('删除成功：项目/会话立即消失，active 切到下一个可见项目', async () => {
    const host = makeHost({ visible: ['/a', '/b'], hidden: [] });
    let registered: Array<{ id: string; path: string }> = host.visible.map((p) => ({ ...p }));
    let hiddenPaths: string[] = [];
    let activePath: string | null = '/a';
    let navGroups: Array<{ path: string | null }> = [
      { path: '/a' },
      { path: '/b' },
      { path: null },
    ];

    const ok = await removeProjectFromHost({
      api: host.api,
      path: '/a',
      activeProjectPath: '/a',
      setRegisteredProjects: (p) => {
        registered = p as Array<{ id: string; path: string }>;
      },
      setHiddenProjectPaths: (p) => {
        hiddenPaths = p;
      },
      setActiveProjectPath: (p) => {
        activePath = p;
      },
      publishNavigation: (updater) => {
        navGroups = (
          typeof updater === 'function'
            ? updater({ groups: navGroups, activeProjectPath: activePath })
            : updater
        ).groups as Array<{ path: string | null }>;
      },
      writeActiveProject: async () => undefined,
    });

    assert.equal(ok, true, '删除成功');
    assert.deepEqual(hiddenPaths, ['/a'], 'hidden set 立即包含已删项目');
    assert.equal(activePath, '/b', 'active 不停留在隐藏会话，切到下一个可见项目');
    assert.ok(!navGroups.some((g) => g.path === '/a'), '导航投影立即移除被删项目');

    // Daemon 旧会话仍返回带 /a 的 project_id → 分组不得复活项目
    const groups = groupAssistantConversations(
      [
        { id: 's1', projectId: '/a', updatedAt: '2026-08-12T00:00:00Z', title: 'old-session' },
        { id: 's2', projectId: '/b', updatedAt: '2026-08-12T00:00:01Z', title: 'b' },
      ],
      registered.map((p) => p.path),
      'Unassigned',
      hiddenPaths,
    );
    assert.ok(!groups.some((g) => g.path === '/a'), 'daemon 旧会话不会反向复活已删项目');
    assert.ok(groups.some((g) => g.path === '/b'), '其他项目仍可见');
  });

  it('重挂载仍隐藏：hidden 集合保留，软删项目不复活', () => {
    const host = makeHost({ visible: ['/a'], hidden: ['/gone'] });
    const groups = groupAssistantConversations(
      [{ id: 's1', projectId: '/gone', updatedAt: '2026-08-12T00:00:00Z', title: 'old' }],
      ['/a'],
      'Unassigned',
      host.hidden,
    );
    assert.ok(!groups.some((g) => g.path === '/gone'), '重挂载后隐藏集合仍生效');
  });

  it('重新添加恢复：register 后 hidden 移除、项目重新出现', async () => {
    const host = makeHost({ visible: ['/b'], hidden: ['/a'] });
    host.reAdd('/a');
    // reAdd 替换了 host.hidden 数组引用，必须重新读取。
    const hiddenPaths = host.hidden;
    const groups = groupAssistantConversations(
      [{ id: 's1', projectId: '/a', updatedAt: '2026-08-12T00:00:00Z', title: 'restored' }],
      ['/a', '/b'],
      'Unassigned',
      hiddenPaths,
    );
    assert.deepEqual(hiddenPaths, [], '重新添加后 hidden 移除该路径');
    assert.ok(groups.some((g) => g.path === '/a'), '重新添加后项目恢复显示');
  });

  it('删除不存在项目：Host not-found 抛错，不假成功', async () => {
    const host = makeHost({ visible: ['/b'], hidden: [] });
    const errors: string[] = [];
    const ok = await removeProjectFromHost({
      api: host.api,
      path: '/missing',
      activeProjectPath: null,
      setRegisteredProjects: () => undefined,
      setHiddenProjectPaths: () => undefined,
      setActiveProjectPath: () => undefined,
      publishNavigation: () => undefined,
      writeActiveProject: async () => undefined,
      onError: (m) => errors.push(m),
    });
    assert.equal(ok, false, '删除不存在必须失败');
    assert.ok(errors.some((e) => e.includes('not found')), `错误信息透传: ${errors.join(', ')}`);
  });

  it('hidden 读取失败 fail-closed：不当作空集合', async () => {
    const host = makeHost({ visible: ['/a'], hidden: [] });
    const errors: string[] = [];
    const ok = await removeProjectFromHost({
      api: {
        list: async () => host.visible.map((p) => ({ ...p })),
        listHidden: async () => null,
        remove: async (id) => {
          host.softRemove(String(id));
        },
      },
      path: '/a',
      activeProjectPath: '/a',
      setRegisteredProjects: () => undefined,
      setHiddenProjectPaths: () => undefined,
      setActiveProjectPath: () => undefined,
      publishNavigation: () => undefined,
      writeActiveProject: async () => undefined,
      onError: (m) => errors.push(m),
    });
    assert.equal(ok, false, 'hidden 读取失败必须 fail-closed');
    assert.ok(errors.length > 0, '失败信息已上报');
  });
});
