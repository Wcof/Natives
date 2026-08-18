'use client';

/**
 * FileBrowser — 文件浏览器（F3-04 拆分后的组合层，ARCH-002）。
 *
 * 职责边界：本文件只做「组合」——
 * - 布局状态（viewMode / sortBy / sortDir / showHidden / search / gridSize）
 * - 各职责 hook 的装配与跨 hook 接线（导航清选择、选择委托操作等）
 * - 跨切面的 Header 事件路由 / 文件区键盘快捷键 / 状态广播
 * - 纯布局 JSX 渲染
 *
 * 业务状态按职责归下列 hook：
 * - useFileNavigation      路径 / 历史 / 最近模式 / 外部导航
 * - useFileEntries         目录加载 / fs_watch / 卡片点亮
 * - useFileFavorites       收藏状态
 * - useFileSelection       单选光标 / 多选 / 右键收敛选择
 * - useFileOperations      剪贴板 / 重命名 / 删除 / 新建 / 解压压缩等
 * - useFileDrop            拖拽落盘编排
 * - useFilePreviewSelection 打开/预览选择（preview 单轨 P2-03）
 */

import { useState, useEffect, useCallback, useRef, useMemo } from 'react';
import { SPACING, FONT_SIZE, BORDER_RADIUS } from '@/lib/design-tokens';
import { type FileEntry } from '@/types/file';
import { t, type Locale } from '@/i18n';
import FileContextMenu from './FileContextMenu';
import DiskUsagePanel from './DiskUsagePanel';
import FileNavShell from './FileNavShell';
import FileSearch from './FileSearch';
import FileArea from './browser/FileArea';
import FileStatusBar from './browser/FileStatusBar';
import FileEmptyState from './browser/FileEmptyState';
import FileModals from './browser/FileModals';
import { nextSortDir, nextSortForField, type FileSortBy } from './file-sort';
import ConfirmDialog from '@/components/ui/ConfirmDialog';
import { useRecentFiles } from '@/lib/recent-files-client';
import type { VirtualFileViewHandle } from '@/lib/preview/contracts';
import {
  FILE_EVENTS,
  dispatchFileEvent,
  onFileEvent,
  type HeaderFileAction,
  type HeaderFileState,
} from '@/lib/file-events';
import { useFileNavigation } from '@/hooks/useFileNavigation';
import { useFileFavorites } from '@/hooks/useFileFavorites';
import { useFileEntries } from '@/hooks/useFileEntries';
import { useFileSelection } from '@/hooks/useFileSelection';
import { useFileOperations } from '@/hooks/useFileOperations';
import { useFileDrop } from '@/hooks/useFileDrop';
import { useFilePreviewSelection } from '@/hooks/useFilePreviewSelection';

interface FileBrowserProps {
  onFileSelect?: (entry: FileEntry) => void;
}

export default function FileBrowser({ onFileSelect }: FileBrowserProps) {
  // ── 组合层 UI 反馈状态：toast + locale ──
  const [locale, setLocale] = useState<Locale>('zh');
  const [toast, setToast] = useState<string | null>(null);
  /** toast 计时器句柄：连续 toast 覆盖前先清理，卸载时清理防 setState */
  const toastTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const showToast = useCallback((msg: string) => {
    if (toastTimerRef.current) clearTimeout(toastTimerRef.current);
    setToast(msg);
    toastTimerRef.current = setTimeout(() => {
      setToast(null);
      toastTimerRef.current = null;
    }, 2200);
  }, []);

  // 卸载时清理 toast 计时器，避免卸载后 setState
  useEffect(() => {
    return () => {
      if (toastTimerRef.current) clearTimeout(toastTimerRef.current);
    };
  }, []);

  // 读取持久化 locale（浏览器 dev 模式无 IPC 时保持默认 zh）
  useEffect(() => {
    async function loadLocale() {
      try {
        const saved = await window.nativesAPI?.getLocale?.();
        if (saved) setLocale(saved === 'en' ? 'en' : 'zh');
      } catch {
        /* ignore */
      }
    }
    void loadLocale();
  }, []);

  // ── 组合层布局状态 ──
  const [viewMode, setViewMode] = useState<'grid' | 'list'>('grid');
  const [sortBy, setSortBy] = useState<'name' | 'mtime' | 'size'>('name');
  const [sortDir, setSortDir] = useState<'asc' | 'desc'>('asc');
  // Snapshot for event handlers that shouldn't re-subscribe on every sort change.
  const sortRef = useRef({ sortBy: 'name' as FileSortBy, sortDir: 'asc' as 'asc' | 'desc' });
  sortRef.current = { sortBy, sortDir };
  const [showHidden, setShowHidden] = useState(false);
  const [searchQuery, setSearchQuery] = useState('');
  const [gridSize, setGridSize] = useState<'sm' | 'md' | 'lg'>(() => {
    if (typeof window !== 'undefined') {
      return (localStorage.getItem('file-grid-size') as 'sm' | 'md' | 'lg') || 'md';
    }
    return 'md';
  });
  const [globalSearchOpen, setGlobalSearchOpen] = useState(false);
  const fileAreaRef = useRef<HTMLDivElement>(null);
  const gridContainerRef = useRef<HTMLDivElement>(null);
  /** T30 冻结接缝：虚拟视图 handle（scrollToIndex/getColumnCount），T31 持有并调用 */
  const viewHandleRef = useRef<VirtualFileViewHandle | null>(null);

  // ── 上下文菜单（组合层 overlay 状态；打开逻辑在 useFileSelection）──
  const [contextMenu, setContextMenu] = useState<{
    x: number;
    y: number;
    entry: FileEntry | null;
    mode: 'file' | 'dir' | 'blank';
  } | null>(null);
  const openContextMenu = useCallback((x: number, y: number, entry: FileEntry, mode: 'file' | 'dir') => {
    setContextMenu({ x, y, entry, mode });
  }, []);

  // ── 职责 hook 装配（跨 hook 周期用 ref 打破循环）──
  // 导航成功后清空选择：resetSelection 由 useFileSelection 提供，经 ref 注入
  // （避免 nav ↔ selection 的调用期循环依赖）
  const resetSelectionRef = useRef<() => void>(() => {});
  const nav = useFileNavigation({
    resetSelection: () => resetSelectionRef.current(),
    showToast,
    locale,
  });

  const recent = useRecentFiles();

  const entriesHook = useFileEntries({
    currentPath: nav.currentPath,
    sortBy,
    sortDir,
    showHidden,
    recentMode: nav.recentMode,
    recentOpenedMode: nav.recentOpenedMode,
    recentOpenedPaths: recent.paths,
    locale,
  });

  const fav = useFileFavorites({
    currentPath: nav.currentPath,
    entries: entriesHook.entries,
    showToast,
    locale,
  });

  const previewSel = useFilePreviewSelection({
    onFileSelect,
    navigateTo: nav.navigateTo,
    markSelfOpened: entriesHook.markSelfOpened,
  });

  // Search filter (client-side)
  const filteredEntries = useMemo(() => {
    if (!searchQuery) return entriesHook.entries;
    const q = searchQuery.toLowerCase();
    return entriesHook.entries.filter((e) => e.name.toLowerCase().includes(q));
  }, [entriesHook.entries, searchQuery]);

  // Grid column count calculation
  const getGridColumns = useCallback(() => {
    const container = gridContainerRef.current;
    if (!container) return 1;
    const containerWidth = container.clientWidth - SPACING.md * 2; // subtract padding
    const minCardWidth = gridSize === 'sm' ? 100 : gridSize === 'lg' ? 200 : 140;
    const gap = 10;
    return Math.max(1, Math.floor((containerWidth + gap) / (minCardWidth + gap)));
  }, [gridSize]);

  const sel = useFileSelection({
    entries: entriesHook.entries,
    filteredEntries,
    currentPath: nav.currentPath,
    searchQuery,
    viewHandleRef,
    onNavigate: nav.navigateTo,
    onOpenContextMenu: openContextMenu,
  });
  // 导航成功后清空选择（详见 resetSelectionRef 注释）
  resetSelectionRef.current = sel.resetSelection;

  const ops = useFileOperations({
    entries: entriesHook.entries,
    filteredEntries,
    selectedPaths: sel.selectedPaths,
    selectedIndex: sel.selectedIndex,
    setSelectedPaths: sel.setSelectedPaths,
    currentPath: nav.currentPath,
    locale,
    loadEntries: entriesHook.loadEntries,
    markSelfOpened: entriesHook.markSelfOpened,
    showToast,
  });

  const drop = useFileDrop({
    currentPath: nav.currentPath,
    loadEntries: entriesHook.loadEntries,
    showToast,
    locale,
  });
  const mutationBlocked = entriesHook.loading || entriesHook.error !== null;

  // A non-success directory state invalidates mutation surfaces opened for the previous path.
  useEffect(() => {
    if (!mutationBlocked) return;
    setContextMenu(null);
    ops.setRenameTarget(null);
    ops.setNewItemTarget(null);
    ops.setTrashTarget(null);
  }, [mutationBlocked, ops.setNewItemTarget, ops.setRenameTarget, ops.setTrashTarget]);

  // 对话框开着时不处理文件区键盘快捷键（与原行为一致：rename/new/trash/contextMenu 打开时跳过）
  const isDialogOpen = !!(ops.renameTarget || ops.newItemTarget || ops.trashTarget || contextMenu);

  // ── 排序 setter（FileNavShell / FileList）──
  const handleSort = (newSortBy: FileSortBy) => {
    const next = nextSortForField(sortBy, sortDir, newSortBy);
    setSortBy(next.sortBy);
    setSortDir(next.sortDir);
  };

  /** Explicit field + direction setter (used by FileNavShell menu). */
  const handleSortChange = useCallback((nextBy: FileSortBy, nextDir: 'asc' | 'desc') => {
    setSortBy(nextBy);
    setSortDir(nextDir);
  }, []);

  // Persist gridSize to localStorage
  useEffect(() => {
    localStorage.setItem('file-grid-size', gridSize);
  }, [gridSize]);

  // 当前目录的项目类型：以后端 list_dir_detailed 的 project 为唯一来源
  // （detect_project_badge 只在 Rust 实现一份，前端不再重复探测）
  const detectedProject = entriesHook.dirProject;

  // Persist active project path and badge to localStorage for other components (like Assistant) to read
  useEffect(() => {
    if (nav.currentPath && nav.currentPath !== '/') {
      localStorage.setItem('natives:active_project_path', nav.currentPath);
      if (detectedProject) {
        localStorage.setItem('natives:active_project_badge', detectedProject);
      } else {
        localStorage.removeItem('natives:active_project_badge');
      }
    } else {
      localStorage.removeItem('natives:active_project_path');
      localStorage.removeItem('natives:active_project_badge');
    }
  }, [nav.currentPath, detectedProject]);

  // ── 文件区键盘导航（仅当无输入聚焦且无对话框打开）──
  // 跨选择/操作/导航/收藏的组合层快捷键；与原 FileBrowser 行为完全一致。
  useEffect(() => {
    const handleFileKeyDown = (e: KeyboardEvent) => {
      // Only handle when no input is focused
      const target = e.target as HTMLElement;
      const isInputFocused = target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.isContentEditable;
      if (isInputFocused) return;
      // Don't handle if a dialog is open
      if (isDialogOpen) return;
      if (mutationBlocked) return;

      const list = filteredEntries;
      if (list.length === 0) return;

      switch (e.key) {
        case 'ArrowDown': {
          e.preventDefault();
          sel.setSelectedIndex((prev) => {
            if (viewMode === 'grid') {
              const cols = getGridColumns();
              return prev < 0 ? 0 : Math.min(prev + cols, list.length - 1);
            }
            return prev < 0 ? 0 : Math.min(prev + 1, list.length - 1);
          });
          break;
        }
        case 'ArrowUp': {
          e.preventDefault();
          sel.setSelectedIndex((prev) => {
            if (viewMode === 'grid') {
              const cols = getGridColumns();
              return prev < 0 ? 0 : Math.max(prev - cols, 0);
            }
            return prev < 0 ? 0 : Math.max(prev - 1, 0);
          });
          break;
        }
        case 'ArrowRight': {
          if (viewMode !== 'grid') break;
          e.preventDefault();
          sel.setSelectedIndex((prev) => (prev < 0 ? 0 : Math.min(prev + 1, list.length - 1)));
          break;
        }
        case 'ArrowLeft': {
          if (viewMode !== 'grid') break;
          e.preventDefault();
          sel.setSelectedIndex((prev) => (prev < 0 ? 0 : Math.max(prev - 1, 0)));
          break;
        }
        case 'Enter': {
          if (sel.selectedIndex < 0 || sel.selectedIndex >= list.length) break;
          e.preventDefault();
          const entry = list[sel.selectedIndex]!;
          if (e.metaKey || e.ctrlKey) {
            onFileSelect?.(entry);
          } else {
            previewSel.handleOpenEntry(entry);
          }
          break;
        }
        case 'F2': {
          if (sel.selectedIndex < 0 || sel.selectedIndex >= list.length) break;
          e.preventDefault();
          ops.handleRename(list[sel.selectedIndex]!);
          break;
        }
        case 'Delete':
        case 'Backspace': {
          // ⌘⌫ / ⌘Del → trash (fanbox). Bare Backspace goes up one directory.
          if (e.metaKey || e.ctrlKey) {
            e.preventDefault();
            if (sel.selectedPaths.size > 1) {
              void ops.handleBatchTrash();
              break;
            }
            if (sel.selectedIndex < 0 || sel.selectedIndex >= list.length) break;
            ops.handleTrash(list[sel.selectedIndex]!);
            break;
          }
          if (e.key === 'Backspace') {
            e.preventDefault();
            const parentPath = nav.currentPath.substring(0, nav.currentPath.lastIndexOf('/')) || '/';
            if (parentPath !== nav.currentPath) nav.navigateTo(parentPath);
          }
          break;
        }
        case 'd':
        case 'D': {
          // ⌘D → duplicate
          if (!(e.metaKey || e.ctrlKey)) break;
          if (sel.selectedIndex < 0 || sel.selectedIndex >= list.length) break;
          e.preventDefault();
          void ops.handleDuplicate(list[sel.selectedIndex]!);
          break;
        }
        case 'c':
        case 'C': {
          // ⌘C → copy to clipboard (in-app + system)
          if (!(e.metaKey || e.ctrlKey)) break;
          e.preventDefault();
          void ops.handleCopyEntry();
          break;
        }
        case 'x':
        case 'X': {
          // ⌘X → cut
          if (!(e.metaKey || e.ctrlKey)) break;
          e.preventDefault();
          void ops.handleCutEntry();
          break;
        }
        case 'v':
        case 'V': {
          // ⌘V → paste into current directory
          if (!(e.metaKey || e.ctrlKey)) break;
          e.preventDefault();
          void ops.handlePaste();
          break;
        }
        case 'a':
        case 'A': {
          // ⌘A → select all
          if (!(e.metaKey || e.ctrlKey)) break;
          e.preventDefault();
          sel.setSelectedPaths(new Set(list.map((x) => x.path)));
          if (list.length > 0) sel.setSelectedIndex(0);
          break;
        }
        case 'Escape': {
          if (sel.selectedPaths.size > 0 || sel.selectedIndex >= 0) {
            e.preventDefault();
            sel.setSelectedPaths(new Set());
            sel.setSelectedIndex(-1);
            sel.lastClickedIndexRef.current = -1;
          }
          break;
        }
        case 'Home': {
          e.preventDefault();
          if (list.length === 0) break;
          sel.setSelectedIndex(0);
          sel.setSelectedPaths(new Set([list[0]!.path]));
          break;
        }
        case 'End': {
          e.preventDefault();
          if (list.length === 0) break;
          const last = list.length - 1;
          sel.setSelectedIndex(last);
          sel.setSelectedPaths(new Set([list[last]!.path]));
          break;
        }
        case ' ': {
          if (sel.selectedIndex < 0 || sel.selectedIndex >= list.length) break;
          e.preventDefault();
          void fav.toggleFavorite(list[sel.selectedIndex]!.path);
          break;
        }
      }
    };
    window.addEventListener('keydown', handleFileKeyDown);
    return () => window.removeEventListener('keydown', handleFileKeyDown);
  }, [
    filteredEntries,
    mutationBlocked,
    sel.selectedIndex,
    sel.selectedPaths,
    sel.setSelectedIndex,
    sel.setSelectedPaths,
    viewMode,
    getGridColumns,
    onFileSelect,
    nav.currentPath,
    nav.navigateTo,
    isDialogOpen,
    fav.toggleFavorite,
    previewSel.handleOpenEntry,
    ops.handleRename,
    ops.handleTrash,
    ops.handleDuplicate,
    ops.handleCopyEntry,
    ops.handleCutEntry,
    ops.handlePaste,
    ops.handleBatchTrash,
  ]);

  // ── Header 动作下行（Header 工具条 → FileBrowser）──
  useEffect(() => {
    return onFileEvent(FILE_EVENTS.headerFileAction, (action: HeaderFileAction) => {
      if (!action) return;
      switch (action.type) {
        case 'viewMode':
          setViewMode(action.value);
          break;
        case 'sortBy': {
          // Same field → toggle direction (so "按名称排序" is never a no-op).
          // New field → natural default (name asc; mtime/size desc).
          const resolved = nextSortForField(
            sortRef.current.sortBy,
            sortRef.current.sortDir,
            action.value as FileSortBy,
          );
          setSortBy(resolved.sortBy);
          setSortDir(resolved.sortDir);
          break;
        }
        case 'sortDir':
          setSortDir(nextSortDir(sortRef.current.sortDir, action.value));
          break;
        case 'showHidden':
          setShowHidden((prev) => !prev);
          break;
        case 'search':
          setSearchQuery(action.value ?? '');
          break;
        case 'newFolder': {
          if (mutationBlocked) break;
          const dir = action.value ?? '';
          if (dir) ops.setNewItemTarget({ parentDir: dir, type: 'folder' });
          break;
        }
        case 'newFile': {
          if (mutationBlocked) break;
          const dir = action.value ?? '';
          if (dir) ops.setNewItemTarget({ parentDir: dir, type: 'file' });
          break;
        }
        case 'gridSize':
          setGridSize(action.value);
          break;
        case 'back':
          nav.goBack();
          break;
        case 'forward':
          nav.goForward();
          break;
        case 'up':
          nav.goUp();
          break;
        case 'refresh':
          void entriesHook.loadEntries();
          break;
        case 'toggleRecent':
          nav.toggleRecent();
          break;
        case 'toggleFavorite':
          void fav.toggleFavorite();
          break;
        case 'globalSearch':
          setGlobalSearchOpen(true);
          break;
        case 'goToPath':
          if (typeof action.value === 'string') void nav.resolveAndNavigate(action.value);
          break;
      }
    });
  }, [
    nav.goBack,
    nav.goForward,
    nav.goUp,
    nav.toggleRecent,
    nav.resolveAndNavigate,
    entriesHook.loadEntries,
    mutationBlocked,
    fav.toggleFavorite,
    ops.setNewItemTarget,
  ]);

  // ── Event bridge: broadcast file-browser state for Header ──
  useEffect(() => {
    const detail: HeaderFileState = {
      viewMode,
      sortBy,
      sortDir,
      showHidden,
      gridSize,
      segments: nav.segments.length > 0 ? nav.segments : ['/'],
      isFavorite: fav.isFavorite,
      breadcrumbPath: nav.currentPath,
      projectBadge: detectedProject,
      canGoBack: nav.canGoBack,
      canGoForward: nav.canGoForward,
      canGoUp: nav.canGoUp,
      recentMode: nav.recentMode,
      recentOpenedMode: nav.recentOpenedMode,
      searchQuery,
      loading: entriesHook.loading,
    };
    dispatchFileEvent(FILE_EVENTS.headerFileState, detail);
  }, [
    viewMode,
    sortBy,
    sortDir,
    showHidden,
    gridSize,
    nav.segments,
    fav.isFavorite,
    nav.currentPath,
    detectedProject,
    nav.canGoBack,
    nav.canGoForward,
    nav.canGoUp,
    nav.recentMode,
    nav.recentOpenedMode,
    searchQuery,
    entriesHook.loading,
    nav.historyTick,
  ]);

  // 浏览器模式：整页明确降级（文件管理需要桌面端 IPC）
  if (entriesHook.nativeMissing) {
    return (
      <div
        style={{
          display: 'flex',
          flexDirection: 'column',
          alignItems: 'center',
          justifyContent: 'center',
          height: '100%',
          gap: SPACING.sm,
          background: 'var(--surface)',
          color: 'var(--text-secondary)',
          fontSize: FONT_SIZE.md,
          textAlign: 'center',
          padding: SPACING.md,
        }}
      >
        <div style={{ fontSize: FONT_SIZE.lg, fontWeight: 600, color: 'var(--text)' }}>
          {t(locale, 'fileBrowser.desktopOnly')}
        </div>
        <div>{t(locale, 'fileBrowser.desktopOnlyHint')}</div>
      </div>
    );
  }

  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        height: '100%',
        background: 'var(--surface)',
        position: 'relative',
      }}
    >
      {/* Navigation chrome: back/forward/up, editable path, filter, global search */}
      <FileNavShell
        currentPath={nav.currentPath}
        canGoBack={nav.canGoBack}
        canGoForward={nav.canGoForward}
        canGoUp={nav.canGoUp}
        isFavorite={fav.isFavorite}
        recentMode={nav.recentMode}
        recentOpenedMode={nav.recentOpenedMode}
        searchQuery={searchQuery}
        sortBy={sortBy}
        sortDir={sortDir}
        loading={entriesHook.loading}
        onBack={nav.goBack}
        onForward={nav.goForward}
        onUp={nav.goUp}
        onRefresh={() => {
          void entriesHook.loadEntries();
        }}
        onToggleFavorite={() => {
          void fav.toggleFavorite();
        }}
        onToggleRecent={nav.toggleRecent}
        onToggleRecentOpened={nav.toggleRecentOpened}
        onSearchChange={setSearchQuery}
        onOpenGlobalSearch={() => setGlobalSearchOpen(true)}
        onPathSubmit={(path) => {
          void nav.resolveAndNavigate(path);
        }}
        onSortChange={handleSortChange}
      />

      {/* File area — drop zone covers entire height including empty space */}
      <FileArea
        locale={locale}
        viewMode={viewMode}
        loading={entriesHook.loading}
        error={entriesHook.error}
        onRetry={() => {
          void entriesHook.loadEntries();
        }}
        entries={filteredEntries}
        isDragging={drop.isDragging}
        dragHandlers={drop.dragHandlers}
        selectedIndex={sel.selectedIndex}
        selectedPaths={sel.selectedPaths}
        onSelect={(entry, ev) => sel.handleSelect(entry, ev)}
        onItemContextMenu={sel.handleContextMenu}
        onBlankClick={() => {
          sel.setSelectedPaths(new Set());
          sel.setSelectedIndex(-1);
          sel.lastClickedIndexRef.current = -1;
        }}
        onBlankContextMenu={(x, y) => {
          setContextMenu({ x, y, entry: null, mode: 'blank' as const });
        }}
        gridSize={gridSize}
        sortBy={sortBy}
        sortDir={sortDir}
        showDir={nav.recentMode || nav.recentOpenedMode}
        onSort={handleSort}
        onEditRequest={previewSel.handleOpenEntry}
        favorites={fav.favoritePaths}
        onFavoriteToggle={fav.handleFavoriteToggle}
        cutPaths={ops.clipBoard?.mode === 'cut' ? new Set(ops.clipBoard.paths) : undefined}
        onMoveDrop={ops.handleInternalMove}
        dragPaths={sel.selectedPaths.size > 0 ? Array.from(sel.selectedPaths) : undefined}
        flashPaths={entriesHook.flashPaths}
        areaRef={fileAreaRef}
        gridContainerRef={gridContainerRef}
        scrollContainerRef={fileAreaRef}
        onViewHandleReady={(h) => {
          viewHandleRef.current = h;
        }}
      />

      {!mutationBlocked && filteredEntries.length === 0 && (
        <FileEmptyState
          locale={locale}
          recentOpenedMode={nav.recentOpenedMode}
          onNewFile={() => ops.handleNewFile(nav.currentPath)}
          onNewFolder={() => ops.handleNewFolder(nav.currentPath)}
          onPaste={() => void ops.handlePaste()}
          canPaste={!!ops.clipBoard && ops.clipBoard.paths.length > 0}
        />
      )}

      {/* Status bar */}
      {!mutationBlocked && filteredEntries.length > 0 && (
        <FileStatusBar
          locale={locale}
          entries={filteredEntries}
          selectedIndex={sel.selectedIndex}
          selectedPaths={sel.selectedPaths}
          clipBoard={ops.clipBoard}
          onOpenDiskUsage={() => ops.setDiskUsageTarget(nav.currentPath)}
        />
      )}

      {/* Context menu */}
      {!mutationBlocked && contextMenu && (
        <FileContextMenu
          entry={contextMenu.entry ?? undefined}
          x={contextMenu.x}
          y={contextMenu.y}
          mode={contextMenu.mode}
          parentDir={nav.currentPath}
          onClose={() => setContextMenu(null)}
          onOpen={previewSel.handleOpenEntry}
          onOpenInTerminal={ops.handleOpenInTerminal}
          onRevealInFinder={ops.handleRevealInFinder}
          onOpenInEditor={ops.handleOpenInEditor}
          onPreview={previewSel.handlePreview}
          onEditImage={previewSel.handleEditRequest}
          onDiskUsage={ops.handleDiskUsage}
          onRename={ops.handleRename}
          onTrash={ops.handleTrash}
          onDuplicate={ops.handleDuplicate}
          onCopy={(entry) => {
            void ops.handleCopyEntry(entry);
          }}
          onCut={(entry) => {
            void ops.handleCutEntry(entry);
          }}
          onPaste={() => {
            void ops.handlePaste();
          }}
          canPaste={!!ops.clipBoard && ops.clipBoard.paths.length > 0}
          onCopyPath={ops.handleCopyPath}
          onCopyImage={ops.handleCopyImage}
          onExtract={(entry) => {
            void ops.handleExtract(entry);
          }}
          onCompress={(entry) => {
            void ops.handleCompress(entry);
          }}
          onOpenDefault={(entry) => {
            void ops.handleOpenWith(entry, 'default');
          }}
          onNewFile={ops.handleNewFile}
          onNewFolder={ops.handleNewFolder}
          onFavorite={(entry) => {
            void fav.toggleFavorite(entry.path);
          }}
          onUnfavorite={(entry) => {
            void fav.toggleFavorite(entry.path);
          }}
          isFavorite={contextMenu.entry ? fav.favoritePaths.includes(contextMenu.entry.path) : fav.isFavorite}
        />
      )}

      {/* Rename / new file / new folder dialogs */}
      {!mutationBlocked && (
        <FileModals
          locale={locale}
          renameTarget={ops.renameTarget}
          renameValue={ops.renameValue}
          onRenameChange={ops.setRenameValue}
          onRenameCancel={() => ops.setRenameTarget(null)}
          onRenameConfirm={() => void ops.handleRenameConfirm()}
          newItemTarget={ops.newItemTarget}
          newItemName={ops.newItemName}
          onNewItemChange={ops.setNewItemName}
          onNewItemCancel={() => ops.setNewItemTarget(null)}
          onNewItemConfirm={() => void ops.handleNewItemConfirm()}
        />
      )}

      {/* Global name/content search (fanbox cmdk-class, scoped + recursive) */}
      {globalSearchOpen && (
        <FileSearch
          rootPath={nav.currentPath}
          onClose={() => setGlobalSearchOpen(false)}
          onNavigate={async (path) => {
            setGlobalSearchOpen(false);
            await nav.resolveAndNavigate(path);
          }}
        />
      )}

      {/* Disk Usage Panel (overlay) */}
      {ops.diskUsageTarget && (
        <DiskUsagePanel
          dirPath={ops.diskUsageTarget}
          onClose={() => ops.setDiskUsageTarget(null)}
          onNavigate={(path) => {
            ops.setDiskUsageTarget(null);
            nav.navigateTo(path);
          }}
        />
      )}

      {/* Trash confirmation dialog */}
      <ConfirmDialog
        open={!mutationBlocked && !!ops.trashTarget}
        title={t(locale, 'fileBrowser.moveToTrash')}
        message={
          ops.trashTarget
            ? t(locale, 'fileBrowser.confirmMoveToTrash').replace('{name}', ops.trashTarget.name)
            : ''
        }
        confirmLabel={t(locale, 'fileBrowser.moveToTrash')}
        cancelLabel={t(locale, 'common.cancel')}
        danger
        onConfirm={ops.doTrash}
        onCancel={() => ops.setTrashTarget(null)}
      />

      {/* Toast */}
      {toast && (
        <div
          role="status"
          aria-live="polite"
          style={{
            position: 'fixed',
            bottom: 24,
            left: '50%',
            transform: 'translateX(-50%)',
            background: 'var(--surface)',
            border: '1px solid var(--border)',
            padding: `${SPACING.sm}px 18px`,
            borderRadius: BORDER_RADIUS.xl,
            fontSize: FONT_SIZE.sm,
            color: 'var(--text)',
            boxShadow: 'var(--shadow-popup)',
            zIndex: 200,
            animation: 'fadeIn 150ms ease',
          }}
        >
          {toast}
        </div>
      )}
    </div>
  );
}
