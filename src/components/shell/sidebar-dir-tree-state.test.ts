import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import {
  DIR_TREE_STALE_MS,
  arrowKindFor,
  dirChildrenOf,
  markError,
  markLoaded,
  markLoading,
  shouldFetchOnExpand,
  toggleExpanded,
  type DirTreeNodes,
} from './sidebar-dir-tree-state';

const entry = (name: string, isDir: boolean, hidden = false) => ({
  name,
  path: `/root/${name}`,
  isDir,
  hidden,
});

describe('dirChildrenOf', () => {
  it('只列文件夹、不列文件、排除隐藏', () => {
    const out = dirChildrenOf([
      entry('src', true),
      entry('readme.md', false),
      entry('.git', true, true),
      entry('docs', true),
    ]);
    assert.deepEqual(out, [
      { name: 'src', path: '/root/src' },
      { name: 'docs', path: '/root/docs' },
    ]);
  });

  it('空列表返回空数组', () => {
    assert.deepEqual(dirChildrenOf([]), []);
  });
});

describe('shouldFetchOnExpand', () => {
  it('从未加载过 → 需要请求', () => {
    assert.equal(shouldFetchOnExpand({}, '/a', 1000), true);
  });

  it('正在加载 → 去重不重复请求（单次展开只发一次）', () => {
    const nodes = markLoading({}, '/a');
    assert.equal(shouldFetchOnExpand(nodes, '/a', 1000), false);
  });

  it('30 秒内已加载 → 折叠再展开不重复请求', () => {
    const nodes = markLoaded({}, '/a', [], 1000);
    assert.equal(shouldFetchOnExpand(nodes, '/a', 1000 + DIR_TREE_STALE_MS), false);
  });

  it('超过 30 秒 → 重新拉取', () => {
    const nodes = markLoaded({}, '/a', [], 1000);
    assert.equal(shouldFetchOnExpand(nodes, '/a', 1000 + DIR_TREE_STALE_MS + 1), true);
  });

  it('上次出错 → 允许重试', () => {
    const nodes = markError({}, '/a');
    assert.equal(shouldFetchOnExpand(nodes, '/a', 1000), true);
  });
});

describe('markLoading / markLoaded / markError', () => {
  it('markLoading 保留上次 children（stale-while-revalidate）', () => {
    const kids = [{ name: 'src', path: '/a/src' }];
    let nodes: DirTreeNodes = markLoaded({}, '/a', kids, 1000);
    nodes = markLoading(nodes, '/a');
    const node = nodes['/a'];
    assert.ok(node);
    assert.equal(node.status, 'loading');
    assert.deepEqual(node.children, kids);
    assert.equal(node.loadedAt, 1000);
  });

  it('首次 markLoading 无历史 children', () => {
    const nodes = markLoading({}, '/a');
    assert.deepEqual(nodes['/a'], { status: 'loading', children: [], loadedAt: 0 });
  });

  it('markLoaded 覆盖为新结果并记录时间戳', () => {
    let nodes: DirTreeNodes = markLoading({}, '/a');
    nodes = markLoaded(nodes, '/a', [{ name: 'b', path: '/a/b' }], 2000);
    assert.deepEqual(nodes['/a'], {
      status: 'loaded',
      children: [{ name: 'b', path: '/a/b' }],
      loadedAt: 2000,
    });
  });

  it('markError 清空 children 并复位时间戳', () => {
    let nodes: DirTreeNodes = markLoaded({}, '/a', [{ name: 'b', path: '/a/b' }], 2000);
    nodes = markError(nodes, '/a');
    assert.deepEqual(nodes['/a'], { status: 'error', children: [], loadedAt: 0 });
  });

  it('状态迁移不改动其它路径（不可变更新）', () => {
    const base: DirTreeNodes = markLoaded({}, '/a', [], 1000);
    const next = markLoading(base, '/b');
    assert.equal(next['/a'], base['/a']);
    assert.equal(base['/b'], undefined);
  });
});

describe('toggleExpanded', () => {
  it('展开/折叠往返', () => {
    let s: ReadonlySet<string> = new Set<string>();
    s = toggleExpanded(s, '/a');
    assert.equal(s.has('/a'), true);
    s = toggleExpanded(s, '/a');
    assert.equal(s.has('/a'), false);
  });

  it('返回新 Set，不修改入参', () => {
    const before = new Set(['/a']);
    const after = toggleExpanded(before, '/b');
    assert.deepEqual([...before], ['/a']);
    assert.equal(after.has('/b'), true);
  });
});

describe('arrowKindFor', () => {
  const open = new Set(['/a']);
  const closed = new Set<string>();

  it('未加载 + 未展开 → collapsed（▸）', () => {
    assert.equal(arrowKindFor({}, closed, '/a'), 'collapsed');
  });

  it('已加载有子目录 + 展开 → expanded（▾）', () => {
    const nodes = markLoaded({}, '/a', [{ name: 'b', path: '/a/b' }], 1000);
    assert.equal(arrowKindFor(nodes, open, '/a'), 'expanded');
  });

  it('展开中且正在加载 → loading（spinner）', () => {
    const nodes = markLoading({}, '/a');
    assert.equal(arrowKindFor(nodes, open, '/a'), 'loading');
  });

  it('空目录（无子文件夹）→ 无箭头/禁用态', () => {
    const nodes = markLoaded({}, '/a', [], 1000);
    assert.equal(arrowKindFor(nodes, open, '/a'), 'none');
    assert.equal(arrowKindFor(nodes, closed, '/a'), 'none');
  });

  it('listDir 报错 → 静默降级为无箭头', () => {
    const nodes = markError({}, '/a');
    assert.equal(arrowKindFor(nodes, closed, '/a'), 'none');
  });

  it('折叠但正在加载（后台刷新）→ 仍显示 collapsed', () => {
    const nodes = markLoading({}, '/a');
    assert.equal(arrowKindFor(nodes, closed, '/a'), 'collapsed');
  });
});
