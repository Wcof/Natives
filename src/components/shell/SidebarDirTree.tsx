'use client';

/**
 * SidebarDirTree — 侧栏目录行的懒加载子目录树
 *
 * 对照 fanbox navDirLi/toggleNavSub（References/fanbox/public/app.js:1880-1921）：
 * - 目录行行首 ▸/▾ 箭头；点箭头懒加载该目录的子目录（只列文件夹、排除隐藏），
 *   点行本身仍是跳转（由父级通过 renderRow 保留原有行为）
 * - 子目录行可继续展开（递归树），缩进逐级递进
 * - 已加载子树折叠后再展开不重复请求；超过 30 秒过期重拉（见 sidebar-dir-tree-state）
 * - 加载态显示 spinner；空目录/出错箭头静默消失
 *
 * IO 契约：只经 files-api（fsApi().listDir + hasNativeFiles 探测），
 * 跳转由父级统一走 `__files__:<path>` → ShellLayout → navigateToFiles。
 * 状态迁移全部在 sidebar-dir-tree-state.ts 的纯函数里（可单测）。
 */

import { useCallback, useEffect, useState } from 'react';
import type { MouseEvent as ReactMouseEvent, ReactNode } from 'react';
import { ChevronDown, ChevronRight, Folder, Loader2 } from 'lucide-react';
import { t, type Locale } from '@/i18n';
import { fsApi, hasNativeFiles } from '@/lib/files-api';
import type { FileEntry } from '@/types/file';
import {
  arrowKindFor,
  dirChildrenOf,
  markError,
  markLoaded,
  markLoading,
  shouldFetchOnExpand,
  toggleExpanded,
  type ArrowKind,
  type DirTreeNodes,
} from './sidebar-dir-tree-state';

export interface SidebarDirTreeProps {
  /** 根目录路径（可为 `~/xxx`，后端 expand_tilde 展开） */
  path: string;
  locale: Locale;
  /** 当前高亮的导航 id（与 `__files__:<path>` 比对，兼容 Sidebar 既有高亮逻辑） */
  activeNavigationId: string | null;
  /** 点子目录行跳转；父级负责 selectNavigation(`__files__:<p>`) → navigateToFiles */
  onNavigate: (path: string) => void;
  /**
   * 渲染根目录行本身（保持 Sidebar 既有行外观/行为不变），
   * arrow 为行首箭头节点；文件系统不可用（浏览器 dev）时为 null，整树静默隐藏。
   */
  renderRow: (arrow: ReactNode) => ReactNode;
}

/** 每级子目录的左缩进（px） */
const INDENT_PER_LEVEL = 14;

/** 行首箭头（▸/▾/spinner）；kind === 'none' 渲染等宽占位保持对齐 */
function DirTreeArrow({
  kind,
  locale,
  onToggle,
}: {
  kind: ArrowKind;
  locale: Locale;
  onToggle: (e: ReactMouseEvent) => void;
}) {
  if (kind === 'none') {
    // 空目录/出错：无箭头（禁用态），保留占位维持图标对齐
    return <span aria-hidden="true" className="inline-flex w-[14px] shrink-0" />;
  }
  if (kind === 'loading') {
    return (
      <span
        aria-hidden="true"
        className="inline-flex w-[14px] shrink-0 items-center justify-center"
        title={t(locale, 'sidebar.dirTree.loading')}
      >
        <Loader2 size={11} className="animate-spin" />
      </span>
    );
  }
  const label = t(
    locale,
    kind === 'expanded' ? 'sidebar.dirTree.collapse' : 'sidebar.dirTree.expand',
  );
  return (
    // 行本身是 <button>（跳转），箭头用 span+role 避免嵌套 button
    <span
      role="button"
      tabIndex={-1}
      aria-label={label}
      title={label}
      onClick={onToggle}
      className="inline-flex w-[14px] shrink-0 items-center justify-center rounded-sm opacity-70 hover:opacity-100"
    >
      {kind === 'expanded' ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
    </span>
  );
}

/** 递归渲染某目录的子目录行（父目录须已在 nodes 中） */
function DirTreeChildren({
  parentPath,
  depth,
  nodes,
  expanded,
  locale,
  activeNavigationId,
  onToggle,
  onNavigate,
}: {
  parentPath: string;
  depth: number;
  nodes: DirTreeNodes;
  expanded: ReadonlySet<string>;
  locale: Locale;
  activeNavigationId: string | null;
  onToggle: (path: string, e: ReactMouseEvent) => void;
  onNavigate: (path: string) => void;
}) {
  const node = nodes[parentPath];
  if (!node || node.status === 'error') return null;
  // 首次加载中且尚无旧内容：占位省略号；有旧内容则先展示旧列表（箭头处已有 spinner）
  if (node.status === 'loading' && node.children.length === 0) {
    return (
      <div
        className="px-3 py-1 text-xs text-[var(--text-disabled)]"
        style={{ paddingLeft: 12 + (depth + 1) * INDENT_PER_LEVEL }}
      >
        …
      </div>
    );
  }
  return (
    <>
      {node.children.map((child) => {
        const navId = `__files__:${child.path}`;
        const isActive = activeNavigationId === navId;
        const isOpen = expanded.has(child.path);
        return (
          <div key={child.path}>
            <button
              type="button"
              onClick={() => onNavigate(child.path)}
              title={child.path}
              className={`flex w-full items-center gap-1.5 rounded-lg px-3 py-1 text-left transition-[color,background-color,border-color,opacity,transform] ${
                isActive
                  ? 'bg-[var(--accent)] text-[var(--accent-ink)] font-medium'
                  : 'text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--primary)]'
              }`}
              style={{ paddingLeft: 12 + (depth + 1) * INDENT_PER_LEVEL }}
            >
              <DirTreeArrow
                kind={arrowKindFor(nodes, expanded, child.path)}
                locale={locale}
                onToggle={(e) => onToggle(child.path, e)}
              />
              <Folder size={13} className="shrink-0" />
              <span className="truncate text-[0.8125rem]">{child.name}</span>
            </button>
            {isOpen && (
              <DirTreeChildren
                parentPath={child.path}
                depth={depth + 1}
                nodes={nodes}
                expanded={expanded}
                locale={locale}
                activeNavigationId={activeNavigationId}
                onToggle={onToggle}
                onNavigate={onNavigate}
              />
            )}
          </div>
        );
      })}
    </>
  );
}

export default function SidebarDirTree({
  path,
  locale,
  activeNavigationId,
  onNavigate,
  renderRow,
}: SidebarDirTreeProps) {
  const [nodes, setNodes] = useState<DirTreeNodes>({});
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(() => new Set());

  // 文件系统能力探测：SSR/浏览器 dev 为 false（整树静默隐藏箭头）。
  // 挂载后再切换，避免 SSR=false / CSR=true 的 hydration mismatch
  //（与 Sidebar 的 usesNativeTrafficLights 同一模式）。
  const [nativeReady, setNativeReady] = useState(false);
  useEffect(() => {

    setNativeReady(hasNativeFiles());
  }, []);

  const handleToggle = useCallback(
    (dirPath: string, e: ReactMouseEvent) => {
      // 点箭头只展开/折叠，不触发行跳转
      e.preventDefault();
      e.stopPropagation();

      // 点击发生在渲染之后，当前 render 的 expanded/nodes 即最新状态
      const wasOpen = expanded.has(dirPath);
      setExpanded(toggleExpanded(expanded, dirPath));
      if (wasOpen) return; // 折叠：仅收起，缓存保留

      // 展开：未加载/已过期（30s）才发请求；单次展开只发一次 listDir
      if (!shouldFetchOnExpand(nodes, dirPath, Date.now())) return;
      setNodes(markLoading(nodes, dirPath));
      void (async () => {
        try {
          const entries = ((await fsApi().listDir(dirPath, { showHidden: false })) ??
            []) as FileEntry[];
          setNodes((prev) => markLoaded(prev, dirPath, dirChildrenOf(entries), Date.now()));
        } catch {
          // listDir 报错：静默降级为无箭头，并收起该行
          setNodes((prev) => markError(prev, dirPath));
          setExpanded((prev) => {
            const next = new Set(prev);
            next.delete(dirPath);
            return next;
          });
        }
      })();
    },
    [expanded, nodes],
  );

  const rootOpen = nativeReady && expanded.has(path);

  return (
    <div>
      {renderRow(
        nativeReady ? (
          <DirTreeArrow
            kind={arrowKindFor(nodes, expanded, path)}
            locale={locale}
            onToggle={(e) => handleToggle(path, e)}
          />
        ) : null,
      )}
      {rootOpen && (
        <DirTreeChildren
          parentPath={path}
          depth={0}
          nodes={nodes}
          expanded={expanded}
          locale={locale}
          activeNavigationId={activeNavigationId}
          onToggle={handleToggle}
          onNavigate={onNavigate}
        />
      )}
    </div>
  );
}
