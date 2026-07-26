/**
 * sidebar-dir-tree-state — 侧栏懒加载目录树的纯状态逻辑
 *
 * 对照 fanbox 的 navDirLi/toggleNavSub（References/fanbox/public/app.js:1880-1921）：
 * 目录行带 ▸/▾ 箭头，点箭头懒加载子目录（只列文件夹、不列文件、排除隐藏），
 * 点行本身仍是跳转。本模块只承载可单测的纯函数：
 * - 节点缓存（已加载子目录 + 加载时间戳）：折叠不清缓存，再展开不重复请求
 * - 30 秒过期：目录内容可能变化，展开时超过 STALE 阈值则重新拉取
 * - 箭头形态推导：loading / 展开 / 折叠 / 无箭头（空目录或 listDir 报错静默降级）
 *
 * React 组件（SidebarDirTree.tsx）只做渲染与 IO，状态迁移全部经由这里。
 */

/** 树节点里展示子目录所需的最小字段（从 FileEntry 裁剪，避免持有整个 wire 对象） */
export interface DirChild {
  name: string;
  path: string;
}

/** 单个目录节点的加载状态 */
export interface DirNodeState {
  status: 'loading' | 'loaded' | 'error';
  /** 已知子目录（loading 复用上次结果，做 stale-while-revalidate 展示） */
  children: DirChild[];
  /** 最近一次加载完成的时间戳（ms）；未完成过为 0 */
  loadedAt: number;
}

/** path → 节点状态；根与所有已展开过的子目录共用一张表 */
export type DirTreeNodes = Readonly<Record<string, DirNodeState>>;

/** 目录内容可能变化：距上次加载超过该毫秒数，再展开时重新拉取 */
export const DIR_TREE_STALE_MS = 30_000;

/** 箭头展示形态 */
export type ArrowKind = 'collapsed' | 'expanded' | 'loading' | 'none';

/** 从 listDir 结果里筛出子目录（只列文件夹、排除隐藏），裁剪为 DirChild */
export function dirChildrenOf(
  entries: ReadonlyArray<{ name: string; path: string; isDir: boolean; hidden: boolean }>,
): DirChild[] {
  return entries
    .filter((e) => e.isDir && !e.hidden)
    .map((e) => ({ name: e.name, path: e.path }));
}

/**
 * 展开该路径时是否需要发起 listDir。
 * - 从未加载过 → 需要
 * - 正在加载 → 不需要（去重：单次展开只发一次）
 * - 上次出错 → 允许重试（不过出错节点箭头已隐藏，实际到不了这里）
 * - 已加载但超过 30 秒 → 需要（内容可能已变化）
 */
export function shouldFetchOnExpand(
  nodes: DirTreeNodes,
  path: string,
  now: number,
): boolean {
  const node = nodes[path];
  if (!node) return true;
  if (node.status === 'loading') return false;
  if (node.status === 'error') return true;
  return now - node.loadedAt > DIR_TREE_STALE_MS;
}

/** 标记进入加载态；保留上次 children 以便展示旧内容而非闪空 */
export function markLoading(nodes: DirTreeNodes, path: string): DirTreeNodes {
  const prev = nodes[path];
  return {
    ...nodes,
    [path]: {
      status: 'loading',
      children: prev?.children ?? [],
      loadedAt: prev?.loadedAt ?? 0,
    },
  };
}

/** 加载成功：写入子目录并记录时间戳 */
export function markLoaded(
  nodes: DirTreeNodes,
  path: string,
  children: DirChild[],
  now: number,
): DirTreeNodes {
  return { ...nodes, [path]: { status: 'loaded', children, loadedAt: now } };
}

/** 加载失败：记为 error（对应箭头静默降级为无箭头） */
export function markError(nodes: DirTreeNodes, path: string): DirTreeNodes {
  return { ...nodes, [path]: { status: 'error', children: [], loadedAt: 0 } };
}

/** 展开集合的切换（展开集合独立于节点缓存：折叠只收起、不清缓存） */
export function toggleExpanded(
  expanded: ReadonlySet<string>,
  path: string,
): Set<string> {
  const next = new Set(expanded);
  if (next.has(path)) next.delete(path);
  else next.add(path);
  return next;
}

/**
 * 推导某目录行的箭头形态：
 * - listDir 报错 → 无箭头（静默降级）
 * - 已加载且无子文件夹 → 无箭头（空目录禁用态）
 * - 展开中且正在加载 → loading（spinner）
 * - 其余按展开集合显示 ▸/▾
 */
export function arrowKindFor(
  nodes: DirTreeNodes,
  expanded: ReadonlySet<string>,
  path: string,
): ArrowKind {
  const node = nodes[path];
  if (node?.status === 'error') return 'none';
  if (node?.status === 'loaded' && node.children.length === 0) return 'none';
  const isOpen = expanded.has(path);
  if (isOpen && node?.status === 'loading') return 'loading';
  return isOpen ? 'expanded' : 'collapsed';
}
