'use client';

import { startTransition, useState, useEffect, useCallback, useRef, useMemo } from 'react';
import { SPACING, FONT_SIZE, BORDER_RADIUS } from '@/lib/design-tokens';
import { type FileEntry, detectFileKind } from '@/types/file';
import { t, type Locale } from '@/i18n';
import FileGrid from './FileGrid';
import FileList from './FileList';
import FileContextMenu from './FileContextMenu';
import DiskUsagePanel from './DiskUsagePanel';
import FileNavShell from './FileNavShell';
import FileSearch from './FileSearch';
import { nextSortDir, nextSortForField, type FileSortBy } from './file-sort';
import Skeleton from '@/components/ui/Skeleton';
import ConfirmDialog from '@/components/ui/ConfirmDialog';
import Modal from '@/components/ui/Modal';
import { pushRecentFile, useRecentFiles, removeRecentFile } from '@/lib/recent-files-client';
import {
  type FavoriteItem,
  loadFavorites,
  toggleFileFavorite,
  saveFavorites,
  isFavoritePath,
  favoriteFilePaths,
} from '@/lib/favorites-client';
import { fmtSize } from '@/lib/format';
import { useFileDrop } from '@/lib/use-file-drop';

export type { FavoriteItem };

/** Tauri IPC 可用时用 nativesAPI.fs，否则抛出错误 */
function getFsApi() {
  const native = (window as any).nativesAPI?.fs;
  if (!native) throw new Error('[FileBrowser] fs API not available (Tauri IPC required)');
  return native;
}

interface FileBrowserProps {
  onFileSelect?: (entry: FileEntry) => void;
}

export default function FileBrowser({ onFileSelect }: FileBrowserProps) {
  const [currentPath, setCurrentPath] = useState('/');
  const [entries, setEntries] = useState<FileEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [viewMode, setViewMode] = useState<'grid' | 'list'>('grid');
  const [sortBy, setSortBy] = useState<'name' | 'mtime' | 'size'>('name');
  const [trashTarget, setTrashTarget] = useState<FileEntry | null>(null);
  const [sortDir, setSortDir] = useState<'asc' | 'desc'>('asc');
  // Snapshot for event handlers that shouldn't re-subscribe on every sort change.
  const sortRef = useRef({ sortBy: 'name' as FileSortBy, sortDir: 'asc' as 'asc' | 'desc' });
  sortRef.current = { sortBy, sortDir };
  const [showHidden, setShowHidden] = useState(false);
  const [searchQuery, setSearchQuery] = useState('');
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number; entry: FileEntry; mode: 'file' | 'dir' | 'blank' } | null>(null);
  const [renameTarget, setRenameTarget] = useState<FileEntry | null>(null);
  const [renameValue, setRenameValue] = useState('');
  const [newItemTarget, setNewItemTarget] = useState<{ parentDir: string; type: 'file' | 'folder' } | null>(null);
  const [newItemName, setNewItemName] = useState('');
  const [toast, setToast] = useState<string | null>(null);
  const [favorites, setFavorites] = useState<FavoriteItem[]>([]);
  const [diskUsageTarget, setDiskUsageTarget] = useState<string | null>(null);
  const [locale, setLocale] = useState<Locale>('zh');
  const [recentMode, setRecentMode] = useState(false);
  /** 「最近打开」视图（读取 LRU），与 recentMode（最近修改，后端扫描）互斥 */
  const [recentOpenedMode, setRecentOpenedMode] = useState(false);
  const { paths: recentOpenedPaths } = useRecentFiles();
  /** 供 loadEntries 读取的最新 LRU 快照，避免把 paths 放进依赖数组导致预览时重载 */
  const recentOpenedPathsRef = useRef<string[]>(recentOpenedPaths);
  recentOpenedPathsRef.current = recentOpenedPaths;
  const [selectedIndex, setSelectedIndex] = useState(-1);
  /** Multi-selection by path (shift/cmd click). Primary cursor remains selectedIndex. */
  const [selectedPaths, setSelectedPaths] = useState<Set<string>>(new Set());
  /** In-app clipboard for cut/copy → paste */
  const [clipBoard, setClipBoard] = useState<{ mode: 'copy' | 'cut'; paths: string[] } | null>(null);
  const [gridSize, setGridSize] = useState<'sm' | 'md' | 'lg'>(() => {
    if (typeof window !== 'undefined') {
      return (localStorage.getItem('file-grid-size') as 'sm' | 'md' | 'lg') || 'md';
    }
    return 'md';
  });

  // Navigation history for back/forward
  const historyRef = useRef<string[]>(['/']);
  const historyIndexRef = useRef(0);
  /** Bumped so canGoBack/canGoForward re-render after ref mutations. */
  const [historyTick, setHistoryTick] = useState(0);
  const [globalSearchOpen, setGlobalSearchOpen] = useState(false);
  const fileAreaRef = useRef<HTMLDivElement>(null);
  const gridContainerRef = useRef<HTMLDivElement>(null);
  const lastClickedIndexRef = useRef<number>(-1);
  /** 代次守卫：快速导航时丢弃过期的 loadEntries 响应，防止旧内容覆盖新目录 */
  const loadIdRef = useRef(0);
  /** toast 计时器句柄：连续 toast 覆盖前先清理，卸载时清理防 setState */
  const toastTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const navigateTo = useCallback((path: string) => {
    const normalized = path === '' ? '/' : path.replace(/\/+$/, '') || '/';
    // Truncate forward history and append (skip no-op)
    if (historyRef.current[historyIndexRef.current] === normalized) {
      setCurrentPath(normalized);
      setRecentMode(false);
      setRecentOpenedMode(false);
      setSelectedPaths(new Set());
      lastClickedIndexRef.current = -1;
      return;
    }
    const hist = historyRef.current.slice(0, historyIndexRef.current + 1);
    hist.push(normalized);
    historyRef.current = hist;
    historyIndexRef.current = hist.length - 1;
    setHistoryTick((n) => n + 1);
    setCurrentPath(normalized);
    setRecentMode(false);
    setRecentOpenedMode(false);
    setSelectedPaths(new Set());
    lastClickedIndexRef.current = -1;
  }, []);

  const goBack = useCallback(() => {
    if (historyIndexRef.current > 0) {
      historyIndexRef.current--;
      setHistoryTick((n) => n + 1);
      setCurrentPath(historyRef.current[historyIndexRef.current]!);
      setRecentMode(false);
      setRecentOpenedMode(false);
      setSelectedPaths(new Set());
      lastClickedIndexRef.current = -1;
    }
  }, []);

  const goForward = useCallback(() => {
    if (historyIndexRef.current < historyRef.current.length - 1) {
      historyIndexRef.current++;
      setHistoryTick((n) => n + 1);
      setCurrentPath(historyRef.current[historyIndexRef.current]!);
      setRecentMode(false);
      setRecentOpenedMode(false);
      setSelectedPaths(new Set());
      lastClickedIndexRef.current = -1;
    }
  }, []);

  const goUp = useCallback(() => {
    if (currentPath === '/' || currentPath === '') return;
    const parentPath = currentPath.substring(0, currentPath.lastIndexOf('/')) || '/';
    if (parentPath !== currentPath) navigateTo(parentPath);
  }, [currentPath, navigateTo]);

  const canGoBack = historyIndexRef.current > 0;
  const canGoForward = historyIndexRef.current < historyRef.current.length - 1;
  const canGoUp = currentPath !== '/' && currentPath !== '';
  // Keep historyTick referenced so React tracks navigation state for chrome buttons.
  void historyTick;

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

  /** Resolve pasted/typed path via fs.stat; open parent if target is a file. */
  const resolveAndNavigate = useCallback(async (raw: string) => {
    let path = raw.trim();
    if (!path) return;
    // Expand bare ~ to home if roots available
    if (path === '~' || path.startsWith('~/')) {
      try {
        const roots = await (window as any).nativesAPI?.fs?.roots?.();
        const home = Array.isArray(roots) ? roots.find((r: any) => r.id === 'home') : null;
        if (home?.path) {
          path = path === '~' ? home.path : home.path + path.slice(1);
        }
      } catch { /* keep as-is */ }
    }
    // Normalize double slashes except leading
    path = path.replace(/\/{2,}/g, '/');
    if (path.length > 1 && path.endsWith('/')) path = path.slice(0, -1);

    try {
      const fsApi = (window as any).nativesAPI?.fs;
      if (fsApi?.stat) {
        const st = await fsApi.stat(path);
        if (st?.found) {
          if (st.isDir) {
            navigateTo(st.path || path);
          } else {
            const parent = (st.path || path).substring(0, (st.path || path).lastIndexOf('/')) || '/';
            navigateTo(parent);
            // Soft-select after list loads: stash intended selection
            (window as any).__pendingSelectFile = st.path || path;
          }
          return;
        }
        showToast(t(locale, 'fileBrowser.pathNotFound'));
        return;
      }
    } catch {
      // fall through to best-effort navigate
    }
    navigateTo(path);
  }, [navigateTo, locale, showToast]);

  // Load favorites, locale, and default home root
  useEffect(() => {
    async function load() {
      try {
        const items = await loadFavorites();
        setFavorites(items);
      } catch { /* ignore */ }
      try {
        const saved = await window.nativesAPI?.getLocale?.();
        if (saved) setLocale(saved === 'en' ? 'en' : 'zh');
      } catch { /* ignore */ }
      // Prefer real home root over "/" (fanbox roots)
      try {
        const fsApi = (window as any).nativesAPI?.fs;
        if (fsApi?.roots && currentPath === '/') {
          const roots = await fsApi.roots();
          const home = Array.isArray(roots) ? roots.find((r: any) => r.id === 'home') : null;
          if (home?.path) {
            historyRef.current = [home.path];
            historyIndexRef.current = 0;
            setHistoryTick((n) => n + 1);
            setCurrentPath(home.path);
          }
        }
      } catch { /* ignore */ }
    }
    load();

    const onFavoritesChanged = () => {
      void loadFavorites().then(setFavorites);
    };
    window.addEventListener('favorites-changed', onFavoritesChanged);
    return () => window.removeEventListener('favorites-changed', onFavoritesChanged);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const isFavorite = isFavoritePath(favorites, currentPath);
  const toggleFavorite = useCallback(async (targetPath?: string) => {
    const path = targetPath ?? currentPath;
    if (!path) return;

    // Infer isDir from current entries / current folder when available
    let isDir: boolean | undefined;
    if (path === currentPath) {
      isDir = true;
    } else {
      const entry = entries.find((e) => e.path === path);
      if (entry) isDir = entry.isDir;
    }

    const { next, added } = toggleFileFavorite(favorites, path, {
      isDir,
      label: path.split(/[/\\]/).pop() || path,
    });
    setFavorites(next);
    try {
      await saveFavorites(next);
      showToast(added ? t(locale, 'fileBrowser.addedToFavorites') : t(locale, 'fileBrowser.removedFromFavorites'));
    } catch { /* ignore */ }
  }, [currentPath, favorites, entries, showToast, locale]);

  const favoritePaths = useMemo(() => favoriteFilePaths(favorites), [favorites]);
  const handleFavoriteToggle = useCallback((entry: FileEntry) => {
    void toggleFavorite(entry.path);
  }, [toggleFavorite]);

  // 「最近修改」「最近打开」互斥切换
  const handleToggleRecent = useCallback(() => {
    setRecentMode((prev) => {
      const next = !prev;
      if (next) setRecentOpenedMode(false);
      return next;
    });
  }, []);

  const handleToggleRecentOpened = useCallback(() => {
    setRecentOpenedMode((prev) => {
      const next = !prev;
      if (next) setRecentMode(false);
      return next;
    });
  }, []);

  const loadEntries = useCallback(async () => {
    // 代次守卫：只有最新一次调用允许写回状态
    const rid = ++loadIdRef.current;
    setLoading(true);
    try {
      const fsApi = getFsApi();

      if (recentOpenedMode) {
        // 最近打开模式：读取 LRU（客户端记录），逐项 stat 过滤死链接
        const paths = recentOpenedPathsRef.current;
        const settled = await Promise.all(
          paths.map(async (p) => {
            try {
              const st = await fsApi.stat(p);
              if (!st?.found || st.isDir) return null; // 死链接或已变成目录 → 跳过
              const dir = p.substring(0, p.lastIndexOf('/')) || '/';
              const name = st.name || p.split('/').pop() || '';
              return {
                name,
                path: st.path || p,
                isDir: false,
                kind: detectFileKind(name),
                hidden: name.startsWith('.'),
                size: st.size || 0,
                mtime: st.mtime || 0,
                btime: 0,
                dirHint: dir === currentPath ? undefined : dir,
              } as FileEntry;
            } catch {
              return null;
            }
          }),
        );
        if (rid !== loadIdRef.current) return;
        setEntries(settled.filter((e): e is FileEntry => e !== null));
      } else if (recentMode) {
        // 最近修改模式：调用后端递归扫描，返回按 mtime 降序的文件
        const recentData = await fsApi.recentFiles(currentPath);
        if (rid !== loadIdRef.current) return;
        if (recentData && Array.isArray(recentData)) {
          // 转换 WalkFile 格式 → FileEntry，填入 dirHint
          const recentEntries: FileEntry[] = recentData.map((f: any) => {
            const dir = f.path.substring(0, f.path.lastIndexOf('/')) || '/';
            const name = f.path.split('/').pop() || '';
            return {
              name,
              path: f.path,
              isDir: false,
              kind: detectFileKind(name),
              hidden: name.startsWith('.'),
              size: f.size || 0,
              mtime: f.mtime || 0,
              btime: 0,
              dirHint: dir === currentPath ? undefined : dir,
            };
          });
          setEntries(recentEntries);
        } else {
          setEntries([]);
        }
      } else {
        const options = { sortBy, sortDir, showHidden, probeProjects: true };
        // Prefer detailed list (entries + project badges on subdirs); fall back to plain listDir
        if (typeof fsApi.listDirDetailed === 'function') {
          const detailed = await fsApi.listDirDetailed(currentPath, options) as {
            entries?: FileEntry[];
            project?: string | null;
          };
          if (rid !== loadIdRef.current) return;
          setEntries(Array.isArray(detailed?.entries) ? detailed.entries : []);
        } else {
          const data = await fsApi.listDir(currentPath, options);
          if (rid !== loadIdRef.current) return;
          setEntries((data as FileEntry[]) || []);
        }
      }
    } catch (err) {
      if (rid !== loadIdRef.current) return;
      showToast(t(locale, 'fileBrowser.loadFailed'));
      setEntries([]);
    } finally {
      if (rid === loadIdRef.current) setLoading(false);
    }
  }, [currentPath, sortBy, sortDir, showHidden, recentMode, recentOpenedMode, locale, showToast]);

  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect
    loadEntries();
  }, [loadEntries]);

  // 「最近打开」模式下，LRU 变化时刷新列表（loadEntries 用 ref 读取 paths，需显式触发）
  useEffect(() => {
    if (!recentOpenedMode) return;
    void loadEntries();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [recentOpenedPaths, recentOpenedMode]);

  // Drag-and-drop: files from Finder + images from WeChat/browser
  const { isDragging, dragHandlers } = useFileDrop({
    currentDir: currentPath,
    onFilesDropped: useCallback(async (paths: string[]) => {
      try {
        const api = window.nativesAPI;
        if (api?.fs?.importFiles) {
          await api.fs.importFiles(paths, currentPath);
          await loadEntries();
          showToast(t(locale, 'fileBrowser.filesDropped'));
        } else {
          showToast(t(locale, 'fileBrowser.importApiUnavailable'));
        }
      } catch (err) {
        showToast(t(locale, 'fileBrowser.importFailed'));
      }
    }, [currentPath, locale, loadEntries, showToast]),
    onUrlDropped: useCallback(async (_savedPath: string) => {
      await loadEntries();
      showToast(t(locale, 'fileBrowser.imageSaved'));
    }, [loadEntries, locale, showToast]),
  });

  // Keyboard shortcuts: Cmd+[ back, Cmd+] forward
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.metaKey && e.key === '[') { e.preventDefault(); goBack(); }
      if (e.metaKey && e.key === ']') { e.preventDefault(); goForward(); }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [goBack, goForward]);

  // Grid column count calculation
  const getGridColumns = useCallback(() => {
    const container = gridContainerRef.current;
    if (!container) return 1;
    const containerWidth = container.clientWidth - SPACING.md * 2; // subtract padding
    const minCardWidth = gridSize === 'sm' ? 100 : gridSize === 'lg' ? 200 : 140;
    const gap = 10;
    return Math.max(1, Math.floor((containerWidth + gap) / (minCardWidth + gap)));
  }, [gridSize]);

  // Persist gridSize to localStorage
  useEffect(() => {
    localStorage.setItem('file-grid-size', gridSize);
  }, [gridSize]);

  // Listen for Header action events
  useEffect(() => {
    const handler = (e: Event) => {
      const detail = (e as CustomEvent).detail;
      if (!detail) return;
      if (detail.type === 'viewMode') setViewMode(detail.value);
      if (detail.type === 'sortBy') {
        const next = detail.value as FileSortBy;
        // Same field → toggle direction (so "按名称排序" is never a no-op).
        // New field → natural default (name asc; mtime/size desc).
        const resolved = nextSortForField(
          sortRef.current.sortBy,
          sortRef.current.sortDir,
          next,
        );
        setSortBy(resolved.sortBy);
        setSortDir(resolved.sortDir);
      }
      if (detail.type === 'sortDir') {
        setSortDir(nextSortDir(sortRef.current.sortDir, detail.value));
      }
      if (detail.type === 'showHidden') setShowHidden((prev) => !prev);
      if (detail.type === 'search') setSearchQuery(detail.value ?? '');
      if (detail.type === 'newFolder') {
        // Use detail.value or fall back to current path from state
        const dir = detail.value ?? '';
        if (dir) setNewItemTarget({ parentDir: dir, type: 'folder' });
      }
      if (detail.type === 'newFile') {
        const dir = detail.value ?? '';
        if (dir) setNewItemTarget({ parentDir: dir, type: 'file' });
      }
      if (detail.type === 'gridSize') setGridSize(detail.value);
      if (detail.type === 'back') goBack();
      if (detail.type === 'forward') goForward();
      if (detail.type === 'up') goUp();
      if (detail.type === 'refresh') void loadEntries();
      if (detail.type === 'toggleRecent') handleToggleRecent();
      if (detail.type === 'toggleFavorite') void toggleFavorite();
      if (detail.type === 'globalSearch') setGlobalSearchOpen(true);
      if (detail.type === 'goToPath' && typeof detail.value === 'string') {
        void resolveAndNavigate(detail.value);
      }
    };
    window.addEventListener('header-file-action', handler);
    return () => window.removeEventListener('header-file-action', handler);
  }, [goBack, goForward, goUp, loadEntries, toggleFavorite, resolveAndNavigate, handleToggleRecent]);

  // Listen for external navigation events (sidebar quick access / favorites)
  useEffect(() => {
    const applyNav = (raw: unknown) => {
      let path: string | undefined;
      if (typeof raw === 'string') path = raw;
      else if (raw && typeof raw === 'object') {
        const d = raw as { path?: string; directory?: string };
        path = d.path ?? d.directory;
      }
      if (!path) return;
      // resolveAndNavigate: file → parent dir + soft-select; dir → open
      void resolveAndNavigate(path);
    };

    // Check for pending path set before mount (race condition fix)
    const pending = (window as any).__pendingNavigateFiles;
    if (typeof pending === 'string') {
      // eslint-disable-next-line react-hooks/set-state-in-effect
      applyNav(pending);
      delete (window as any).__pendingNavigateFiles;
    }

    const handler = (e: Event) => {
      applyNav((e as CustomEvent).detail);
    };
    window.addEventListener('navigate-files', handler);
    return () => window.removeEventListener('navigate-files', handler);
  }, [resolveAndNavigate]);

  const filteredEntries = useMemo(() => {
    if (!searchQuery) return entries;
    const q = searchQuery.toLowerCase();
    return entries.filter(e => e.name.toLowerCase().includes(q));
  }, [entries, searchQuery]);

  const handleSelect = useCallback((entry: FileEntry, e?: { shiftKey?: boolean; metaKey?: boolean; ctrlKey?: boolean }) => {
    const idx = filteredEntries.findIndex(x => x.path === entry.path);
    const isMulti = !!(e && (e.metaKey || e.ctrlKey || e.shiftKey));

    if (isMulti) {
      setSelectedIndex(idx);
      setSelectedPaths(prev => {
        const next = new Set(prev);
        if (e?.shiftKey && lastClickedIndexRef.current >= 0 && idx >= 0) {
          const a = Math.min(lastClickedIndexRef.current, idx);
          const b = Math.max(lastClickedIndexRef.current, idx);
          for (let i = a; i <= b; i++) {
            const p = filteredEntries[i]?.path;
            if (p) next.add(p);
          }
        } else if (e?.metaKey || e?.ctrlKey) {
          if (next.has(entry.path)) next.delete(entry.path);
          else next.add(entry.path);
          lastClickedIndexRef.current = idx;
        }
        return next;
      });
      return;
    }

    // Single click: set selection; directories open immediately (fanbox)
    lastClickedIndexRef.current = idx;
    setSelectedIndex(idx);
    setSelectedPaths(new Set([entry.path]));
    if (entry.isDir) {
      navigateTo(entry.path);
    }
  }, [filteredEntries, navigateTo]);

  const handleOpenEntry = useCallback((entry: FileEntry) => {
    if (entry.isDir) {
      navigateTo(entry.path);
    } else {
      pushRecentFile(entry.path);
      onFileSelect?.(entry);
    }
  }, [navigateTo, onFileSelect]);

  const handleNavigate = (path: string) => {
    navigateTo(path);
  };

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

  const handleContextMenu = (e: React.MouseEvent, entry: FileEntry) => {
    e.preventDefault();
    // Stop propagation so the blank-area handler on the parent doesn't fire
    e.stopPropagation();
    // If right-click target is outside current multi-selection, select only it
    if (!selectedPaths.has(entry.path)) {
      setSelectedPaths(new Set([entry.path]));
      const idx = filteredEntries.findIndex(x => x.path === entry.path);
      setSelectedIndex(idx);
      lastClickedIndexRef.current = idx;
    }
    setContextMenu({ x: e.clientX, y: e.clientY, entry, mode: entry.isDir ? 'dir' : 'file' });
  };

  // Context menu actions
  const handleOpen = useCallback((entry: FileEntry) => {
    handleOpenEntry(entry);
  }, [handleOpenEntry]);

  const handleRename = useCallback((entry: FileEntry) => {
    setRenameTarget(entry);
    setRenameValue(entry.name);
  }, []);

  const handleRenameConfirm = useCallback(async () => {
    if (!renameTarget || !renameValue.trim()) return;
    const parentDir = renameTarget.path.substring(0, renameTarget.path.lastIndexOf('/')) || '/';
    const newPath = `${parentDir}/${renameValue.trim()}`;
    try {
      const result = await getFsApi().renameEntry(renameTarget.path, newPath);
      if (result?.ok) {
        showToast(t(locale, 'fileBrowser.renamed'));
        window.dispatchEvent(new CustomEvent('file-renamed', { detail: { oldPath: renameTarget.path, newPath } }));
        await loadEntries();
      } else {
        showToast(result?.error || t(locale, 'fileBrowser.renameFailed'));
      }
    } catch (err) {
      showToast(t(locale, 'fileBrowser.renameFailed'));
    }
    setRenameTarget(null);
    setRenameValue('');
  }, [renameTarget, renameValue, loadEntries, showToast, locale]);

  const handleTrash = useCallback((entry: FileEntry) => {
    // fanbox: files trash immediately; directories ask once
    if (entry.isDir) {
      setTrashTarget(entry);
      return;
    }
    void (async () => {
      try {
        const result = await getFsApi().trashEntry(entry.path);
        if (result?.ok) {
          showToast(t(locale, 'fileBrowser.trashed'));
          void removeRecentFile(entry.path);
          window.dispatchEvent(new CustomEvent('file-trashed', { detail: { path: entry.path } }));
          setSelectedPaths(prev => {
            const n = new Set(prev); n.delete(entry.path); return n;
          });
          await loadEntries();
        } else {
          showToast(result?.error || t(locale, 'fileBrowser.trashFailed'));
        }
      } catch {
        showToast(t(locale, 'fileBrowser.trashFailed'));
      }
    })();
  }, [loadEntries, showToast, locale]);

  const handleDuplicate = useCallback(async (entry: FileEntry) => {
    try {
      const fsApi = getFsApi();
      if (typeof fsApi.duplicateEntry !== 'function') {
        showToast(t(locale, 'fileBrowser.duplicateFailed'));
        return;
      }
      const result = await fsApi.duplicateEntry(entry.path);
      if (result?.ok) {
        showToast(t(locale, 'fileBrowser.duplicated'));
        await loadEntries();
        if (result.path) {
          window.dispatchEvent(new CustomEvent('file-flash', { detail: result.path }));
        }
      } else {
        showToast(result?.error || t(locale, 'fileBrowser.duplicateFailed'));
      }
    } catch {
      showToast(t(locale, 'fileBrowser.duplicateFailed'));
    }
  }, [loadEntries, showToast, locale]);

  /** Resolve current target paths: multi-selection, else single entry, else cursor */
  const resolveTargetPaths = useCallback((entry?: FileEntry | null) => {
    if (selectedPaths.size > 0) return Array.from(selectedPaths);
    if (entry) return [entry.path];
    if (selectedIndex >= 0 && selectedIndex < filteredEntries.length) {
      return [filteredEntries[selectedIndex]!.path];
    }
    return [] as string[];
  }, [selectedPaths, selectedIndex, filteredEntries]);

  const handleCopyEntry = useCallback(async (entry?: FileEntry) => {
    const paths = resolveTargetPaths(entry);
    if (paths.length === 0) return;
    setClipBoard({ mode: 'copy', paths });
    // System pasteboard for Finder paste (fanbox copyFile)
    try {
      const fsApi = getFsApi();
      if (typeof fsApi.clipboardCopyFiles === 'function') {
        await fsApi.clipboardCopyFiles(paths);
      } else {
        await navigator.clipboard.writeText(paths.join('\n'));
      }
    } catch {
      try { await navigator.clipboard.writeText(paths.join('\n')); } catch { /* ignore */ }
    }
    showToast(t(locale, 'fileBrowser.batchCopied').replace('{count}', String(paths.length)));
  }, [resolveTargetPaths, showToast, locale]);

  const handleCutEntry = useCallback(async (entry?: FileEntry) => {
    const paths = resolveTargetPaths(entry);
    if (paths.length === 0) return;
    setClipBoard({ mode: 'cut', paths });
    try {
      const fsApi = getFsApi();
      if (typeof fsApi.clipboardCopyFiles === 'function') {
        await fsApi.clipboardCopyFiles(paths);
      }
    } catch { /* ignore */ }
    showToast(t(locale, 'fileBrowser.cutDone').replace('{count}', String(paths.length)));
  }, [resolveTargetPaths, showToast, locale]);

  const handlePaste = useCallback(async () => {
    if (!clipBoard || clipBoard.paths.length === 0) {
      showToast(t(locale, 'fileBrowser.pasteEmpty'));
      return;
    }
    try {
      const fsApi = getFsApi();
      if (clipBoard.mode === 'copy') {
        if (typeof fsApi.copyEntries !== 'function') {
          // fallback sequential
          for (const p of clipBoard.paths) {
            await fsApi.copyEntry?.(p, currentPath);
          }
          showToast(t(locale, 'fileBrowser.pasted').replace('{count}', String(clipBoard.paths.length)));
        } else {
          const result = await fsApi.copyEntries(clipBoard.paths, currentPath);
          const count = result?.count ?? clipBoard.paths.length;
          showToast(t(locale, 'fileBrowser.pasted').replace('{count}', String(count)));
        }
      } else {
        if (typeof fsApi.moveEntries !== 'function') {
          for (const p of clipBoard.paths) {
            await fsApi.moveEntry?.(p, currentPath);
          }
          showToast(t(locale, 'fileBrowser.batchMoved').replace('{count}', String(clipBoard.paths.length)));
        } else {
          const result = await fsApi.moveEntries(clipBoard.paths, currentPath);
          const count = result?.count ?? clipBoard.paths.length;
          showToast(t(locale, 'fileBrowser.batchMoved').replace('{count}', String(count)));
        }
        setClipBoard(null); // cut is one-shot
      }
      await loadEntries();
    } catch {
      showToast(t(locale, 'fileBrowser.pasteFailed'));
    }
  }, [clipBoard, currentPath, loadEntries, showToast, locale]);

  const handleCopyPath = useCallback(async (entry: FileEntry) => {
    try {
      await navigator.clipboard.writeText(entry.path);
      showToast(t(locale, 'fileBrowser.pathCopied'));
    } catch {
      showToast(t(locale, 'fileBrowser.copyPath'));
    }
  }, [showToast, locale]);

  const handleCopyImage = useCallback(async (entry: FileEntry) => {
    try {
      const fsApi = getFsApi();
      if (typeof fsApi.clipboardCopyImage !== 'function') {
        showToast(t(locale, 'fileBrowser.clipboardImageFailed'));
        return;
      }
      const r = await fsApi.clipboardCopyImage(entry.path);
      showToast(r?.ok ? t(locale, 'fileBrowser.clipboardImageCopied') : t(locale, 'fileBrowser.clipboardImageFailed'));
    } catch {
      showToast(t(locale, 'fileBrowser.clipboardImageFailed'));
    }
  }, [showToast, locale]);

  const handleOpenWith = useCallback(async (entry: FileEntry, withApp: 'default' | 'reveal' | 'terminal' | 'editor' = 'default') => {
    try {
      const fsApi = getFsApi();
      if (typeof fsApi.openWith === 'function') {
        await fsApi.openWith(entry.path, withApp);
        return;
      }
    } catch { /* fall through */ }
    const api = (window as any).nativesAPI?.shell;
    if (withApp === 'reveal' && api?.showItemInFolder) api.showItemInFolder(entry.path);
    else if (api?.openPath) api.openPath(entry.path);
  }, []);

  const handleBatchTrash = useCallback(async () => {
    const paths = resolveTargetPaths(null);
    if (paths.length === 0) return;
    if (paths.length === 1) {
      const entry = entries.find(e => e.path === paths[0]) || filteredEntries.find(e => e.path === paths[0]);
      if (entry) { setTrashTarget(entry); return; }
    }
    // Multi: confirm via first name style message then trash
    try {
      const fsApi = getFsApi();
      if (typeof fsApi.trashEntries === 'function') {
        const r = await fsApi.trashEntries(paths);
        showToast(t(locale, 'fileBrowser.batchTrashed').replace('{count}', String(r?.count ?? paths.length)));
      } else {
        for (const p of paths) await fsApi.trashEntry(p);
        showToast(t(locale, 'fileBrowser.batchTrashed').replace('{count}', String(paths.length)));
      }
      paths.forEach((p) => { void removeRecentFile(p); });
      setSelectedPaths(new Set());
      await loadEntries();
    } catch {
      showToast(t(locale, 'fileBrowser.trashFailed'));
    }
  }, [resolveTargetPaths, entries, filteredEntries, loadEntries, showToast, locale]);


  const handleInternalMove = useCallback(async (sourcePaths: string[], destDir: string) => {
    if (!sourcePaths.length || !destDir) return;
    // Prevent moving a folder into itself
    const safe = sourcePaths.filter(p => p !== destDir && !destDir.startsWith(p + '/'));
    if (!safe.length) return;
    try {
      const fsApi = getFsApi();
      if (typeof fsApi.moveEntries === 'function') {
        const r = await fsApi.moveEntries(safe, destDir);
        showToast(t(locale, 'fileBrowser.batchMoved').replace('{count}', String(r?.count ?? safe.length)));
      } else {
        for (const pth of safe) await fsApi.moveEntry?.(pth, destDir);
        showToast(t(locale, 'fileBrowser.batchMoved').replace('{count}', String(safe.length)));
      }
      setSelectedPaths(new Set());
      await loadEntries();
    } catch {
      showToast(t(locale, 'fileBrowser.moveFailed'));
    }
  }, [loadEntries, showToast, locale]);

  // Shell operations
  const handleRevealInFinder = useCallback((entry: FileEntry) => {
    const api = (window as any).nativesAPI?.shell;
    if (api?.showItemInFolder) {
      api.showItemInFolder(entry.path);
    } else {
      showToast(t(locale, 'fileBrowser.revealInFinder') + ': ' + entry.path);
    }
  }, [showToast, locale]);

  const handleOpenInEditor = useCallback((entry: FileEntry) => {
    const api = (window as any).nativesAPI?.shell;
    if (api?.openPath) {
      api.openPath(entry.path);
    } else {
      navigator.clipboard.writeText(entry.path);
      showToast(t(locale, 'fileBrowser.copyPath'));
    }
  }, [showToast, locale]);

  const handleOpenInTerminal = useCallback(async (dir: string) => {
    const api = (window as any).nativesAPI;
    if (api?.terminal?.create && api?.terminal?.write) {
      try {
        // 打开终端面板 → 新建 PTY 会话 → cd 进目标目录
        // 用幂等的 open-terminal（仅在折叠时展开），避免终端已打开时被 toggle 关闭
        window.dispatchEvent(new CustomEvent('open-terminal'));
        const result = await api.terminal.create() as { sessionId?: string; error?: string };
        const sessionId = result?.sessionId;
        if (!sessionId) {
          showToast(t(locale, 'fileBrowser.terminalOpenFailed'));
          return;
        }
        // 单引号包裹并转义内部单引号（' → '\''），防止路径中的空格/特殊字符
        const escaped = dir.replace(/'/g, "'\\''");
        await api.terminal.write(sessionId, `cd '${escaped}'\n`);
      } catch {
        showToast(t(locale, 'fileBrowser.terminalOpenFailed'));
      }
    } else {
      // 浏览器 dev 模式降级：复制 cd 命令
      navigator.clipboard.writeText(`cd "${dir}"`);
      showToast(t(locale, 'fileBrowser.copyAsCd'));
    }
  }, [showToast, locale]);

  const handlePreview = useCallback((entry: FileEntry) => {
    onFileSelect?.(entry);
  }, [onFileSelect]);

  const handleEditRequest = useCallback((entry: FileEntry) => {
    onFileSelect?.(entry);
  }, [onFileSelect]);

  const handleDiskUsage = useCallback((dir: string) => {
    setDiskUsageTarget(dir);
  }, []);

  const doTrash = useCallback(async () => {
    if (!trashTarget) return;
    const trashedPath = trashTarget.path;
    try {
      const result = await getFsApi().trashEntry(trashedPath);
      if (result?.ok) {
        showToast(t(locale, 'fileBrowser.trashed'));
        void removeRecentFile(trashedPath);
        window.dispatchEvent(new CustomEvent('file-trashed', { detail: { path: trashedPath } }));
        await loadEntries();
      } else {
        showToast(result?.error || t(locale, 'fileBrowser.trashFailed'));
      }
    } catch (err) {
      showToast(t(locale, 'fileBrowser.trashFailed'));
    } finally {
      setTrashTarget(null);
    }
  }, [trashTarget, loadEntries, showToast, locale]);

  const handleNewFile = useCallback((parentDir: string) => {
    setNewItemTarget({ parentDir, type: 'file' });
    setNewItemName('');
  }, []);

  const handleNewFolder = useCallback((parentDir: string) => {
    setNewItemTarget({ parentDir, type: 'folder' });
    setNewItemName('');
  }, []);

  const handleNewItemConfirm = useCallback(async () => {
    if (!newItemTarget || !newItemName.trim()) return;
    const targetPath = `${newItemTarget.parentDir}/${newItemName.trim()}`;
    try {
      const result = await getFsApi().createEntry(targetPath, newItemTarget.type);
      if (result?.ok) {
        showToast(t(locale, 'fileBrowser.created'));
        await loadEntries();
      } else {
        showToast(result?.error || t(locale, 'fileBrowser.createFailed'));
      }
    } catch (err) {
      showToast(t(locale, 'fileBrowser.createFailed'));
    }
    setNewItemTarget(null);
    setNewItemName('');
  }, [newItemTarget, newItemName, loadEntries, showToast, locale]);

  const segments = currentPath.split('/').filter(Boolean);

  // Search filter (client-side)
  // Keyboard navigation for file area
  useEffect(() => {
    const handleFileKeyDown = (e: KeyboardEvent) => {
      // Only handle when no input is focused
      const target = e.target as HTMLElement;
      const isInputFocused = target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.isContentEditable;
      if (isInputFocused) return;
      // Don't handle if a dialog is open
      if (renameTarget || newItemTarget || trashTarget || contextMenu) return;

      const list = filteredEntries;
      if (list.length === 0) return;

      switch (e.key) {
        case 'ArrowDown': {
          e.preventDefault();
          setSelectedIndex(prev => {
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
          setSelectedIndex(prev => {
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
          setSelectedIndex(prev => prev < 0 ? 0 : Math.min(prev + 1, list.length - 1));
          break;
        }
        case 'ArrowLeft': {
          if (viewMode !== 'grid') break;
          e.preventDefault();
          setSelectedIndex(prev => prev < 0 ? 0 : Math.max(prev - 1, 0));
          break;
        }
        case 'Enter': {
          if (selectedIndex < 0 || selectedIndex >= list.length) break;
          e.preventDefault();
          const entry = list[selectedIndex]!;
          if (e.metaKey || e.ctrlKey) {
            onFileSelect?.(entry);
          } else {
            handleOpenEntry(entry);
          }
          break;
        }
        case 'F2': {
          if (selectedIndex < 0 || selectedIndex >= list.length) break;
          e.preventDefault();
          handleRename(list[selectedIndex]!);
          break;
        }
        case 'Delete':
        case 'Backspace': {
          // ⌘⌫ / ⌘Del → trash (fanbox). Bare Backspace goes up one directory.
          if (e.metaKey || e.ctrlKey) {
            e.preventDefault();
            if (selectedPaths.size > 1) { void handleBatchTrash(); break; }
            if (selectedIndex < 0 || selectedIndex >= list.length) break;
            handleTrash(list[selectedIndex]!);
            break;
          }
          if (e.key === 'Backspace') {
            e.preventDefault();
            const parentPath = currentPath.substring(0, currentPath.lastIndexOf('/')) || '/';
            if (parentPath !== currentPath) navigateTo(parentPath);
          }
          break;
        }
        case 'd':
        case 'D': {
          // ⌘D → duplicate
          if (!(e.metaKey || e.ctrlKey)) break;
          if (selectedIndex < 0 || selectedIndex >= list.length) break;
          e.preventDefault();
          void handleDuplicate(list[selectedIndex]!);
          break;
        }
        case 'c':
        case 'C': {
          // ⌘C → copy to clipboard (in-app + system)
          if (!(e.metaKey || e.ctrlKey)) break;
          e.preventDefault();
          void handleCopyEntry();
          break;
        }
        case 'x':
        case 'X': {
          // ⌘X → cut
          if (!(e.metaKey || e.ctrlKey)) break;
          e.preventDefault();
          void handleCutEntry();
          break;
        }
        case 'v':
        case 'V': {
          // ⌘V → paste into current directory
          if (!(e.metaKey || e.ctrlKey)) break;
          e.preventDefault();
          void handlePaste();
          break;
        }
        case 'a':
        case 'A': {
          // ⌘A → select all
          if (!(e.metaKey || e.ctrlKey)) break;
          e.preventDefault();
          setSelectedPaths(new Set(list.map(x => x.path)));
          if (list.length > 0) setSelectedIndex(0);
          break;
        }
        case 'Escape': {
          if (selectedPaths.size > 0 || selectedIndex >= 0) {
            e.preventDefault();
            setSelectedPaths(new Set());
            setSelectedIndex(-1);
            lastClickedIndexRef.current = -1;
          }
          break;
        }
        case 'Home': {
          e.preventDefault();
          if (list.length === 0) break;
          setSelectedIndex(0);
          setSelectedPaths(new Set([list[0]!.path]));
          break;
        }
        case 'End': {
          e.preventDefault();
          if (list.length === 0) break;
          const last = list.length - 1;
          setSelectedIndex(last);
          setSelectedPaths(new Set([list[last]!.path]));
          break;
        }
        case ' ': {
          if (selectedIndex < 0 || selectedIndex >= list.length) break;
          e.preventDefault();
          toggleFavorite(list[selectedIndex]!.path);
          break;
        }
      }
    };
    window.addEventListener('keydown', handleFileKeyDown);
    return () => window.removeEventListener('keydown', handleFileKeyDown);
  }, [filteredEntries, selectedIndex, viewMode, getGridColumns, onFileSelect, currentPath, renameTarget, newItemTarget, trashTarget, contextMenu, toggleFavorite, handleOpenEntry, handleRename, handleTrash, handleDuplicate, handleCopyEntry, handleCutEntry, handlePaste, handleBatchTrash, navigateTo, selectedPaths]);

  // Reset selection when entries or path change; honor pending file selection from path bar
  useEffect(() => {
    startTransition(() => {
      const pending = (window as any).__pendingSelectFile as string | undefined;
      if (pending && entries.some((e) => e.path === pending)) {
        const idx = entries.findIndex((e) => e.path === pending);
        setSelectedIndex(idx);
        setSelectedPaths(new Set([pending]));
        lastClickedIndexRef.current = idx;
        delete (window as any).__pendingSelectFile;
        return;
      }
      setSelectedIndex(-1);
      setSelectedPaths(new Set());
      lastClickedIndexRef.current = -1;
    });
  }, [currentPath, entries, searchQuery]);

  // Scroll keyboard selection into view
  useEffect(() => {
    if (selectedIndex < 0 || selectedIndex >= filteredEntries.length) return;
    const path = filteredEntries[selectedIndex]?.path;
    if (!path) return;
    const el = document.querySelector(`[data-file-entry="${CSS.escape(path)}"]`) as HTMLElement | null;
    el?.scrollIntoView({ block: 'nearest' });
  }, [selectedIndex, filteredEntries]);

  // Detect project badge from current directory entries
  const detectedProject = useMemo(() => {
    const names = new Set(entries.filter(e => !e.isDir).map(e => e.name.toLowerCase()));
    if (names.has('package.json')) return 'node' as const;
    if (names.has('index.html')) return 'web' as const;
    if (names.has('requirements.txt') || names.has('pyproject.toml')) return 'python' as const;
    if (names.has('cargo.toml')) return 'rust' as const;
    if (names.has('go.mod')) return 'go' as const;
    if (entries.some(e => e.isDir && e.name === '.git')) return 'git' as const;
    return null;
  }, [entries]);

  // Persist active project path and badge to localStorage for other components (like Assistant) to read
  useEffect(() => {
    if (currentPath && currentPath !== '/') {
      localStorage.setItem('natives:active_project_path', currentPath);
      if (detectedProject) {
        localStorage.setItem('natives:active_project_badge', detectedProject);
      } else {
        localStorage.removeItem('natives:active_project_badge');
      }
    } else {
      localStorage.removeItem('natives:active_project_path');
      localStorage.removeItem('natives:active_project_badge');
    }
  }, [currentPath, detectedProject]);

  // ── Event bridge: broadcast file-browser state for Header ──
  useEffect(() => {
    const detail = {
      viewMode, sortBy, sortDir, showHidden, gridSize,
      segments: segments.length > 0 ? segments : ['/'],
      isFavorite, breadcrumbPath: currentPath, projectBadge: detectedProject,
      canGoBack, canGoForward, canGoUp, recentMode, recentOpenedMode, searchQuery, loading,
    };
    window.dispatchEvent(new CustomEvent('header-file-state', { detail }));
  }, [viewMode, sortBy, sortDir, showHidden, gridSize, segments, isFavorite, currentPath, detectedProject, canGoBack, canGoForward, canGoUp, recentMode, recentOpenedMode, searchQuery, loading, historyTick]);

  return (
    <div style={{
      display: 'flex',
      flexDirection: 'column',
      height: '100%',
      background: 'var(--surface)',
      position: 'relative',
    }}>
      {/* Navigation chrome: back/forward/up, editable path, filter, global search */}
      <FileNavShell
        currentPath={currentPath}
        canGoBack={canGoBack}
        canGoForward={canGoForward}
        canGoUp={canGoUp}
        isFavorite={isFavorite}
        recentMode={recentMode}
        recentOpenedMode={recentOpenedMode}
        searchQuery={searchQuery}
        sortBy={sortBy}
        sortDir={sortDir}
        loading={loading}
        onBack={goBack}
        onForward={goForward}
        onUp={goUp}
        onRefresh={() => { void loadEntries(); }}
        onToggleFavorite={() => { void toggleFavorite(); }}
        onToggleRecent={handleToggleRecent}
        onToggleRecentOpened={handleToggleRecentOpened}
        onSearchChange={setSearchQuery}
        onOpenGlobalSearch={() => setGlobalSearchOpen(true)}
        onPathSubmit={(path) => { void resolveAndNavigate(path); }}
        onSortChange={handleSortChange}
      />

      {/* File area — drop zone covers entire height including empty space */}
      <div
        ref={fileAreaRef}
        {...dragHandlers}
        style={{ flex: 1, overflow: 'auto', position: 'relative' }}
        role="listbox"
        aria-label={t(locale, 'fileBrowser.ariaLabelFiles')}
        tabIndex={0}
        onClick={(e) => {
          const target = e.target as HTMLElement;
          if (!target.closest('[data-file-entry]')) {
            setSelectedPaths(new Set());
            setSelectedIndex(-1);
            lastClickedIndexRef.current = -1;
          }
        }}
        onContextMenu={(e) => {
          // Blank area right-click — only if not on a file/dir element
          const target = e.target as HTMLElement;
          if (!target.closest('[data-file-entry]')) {
            e.preventDefault();
            setContextMenu({ x: e.clientX, y: e.clientY, entry: null as any, mode: 'blank' as const });
          }
        }}
      >
        {/* Drop overlay — fills entire area including empty space below files */}
        {isDragging && (
          <div
            style={{
              position: 'absolute',
              inset: 4,
              zIndex: 20,
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              pointerEvents: 'none',
              border: '2px dashed var(--primary)',
              borderRadius: 'var(--radius, 4px)',
              background: 'var(--accent-soft, rgba(205,242,75,0.08))',
              color: 'var(--primary)',
              fontSize: FONT_SIZE.lg,
              fontWeight: 600,
            }}
          >
            {t(locale, 'fileBrowser.dropHere')}
          </div>
        )}
        {loading ? (
          <div style={{ padding: viewMode === 'grid' ? 12 : 0 }}>
            {viewMode === 'grid' ? (
              <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(140px, 1fr))', gap: SPACING.sm }}>
                {Array.from({ length: 8 }, (_, i) => <Skeleton key={i} variant="card" />)}
              </div>
            ) : (
              <Skeleton variant="table" lines={10} />
            )}
          </div>
        ) : viewMode === 'grid' ? (
          <FileGrid
            ref={gridContainerRef}
            entries={filteredEntries}
            onSelect={(entry, ev) => handleSelect(entry, ev)}
            onContextMenu={handleContextMenu}
            selectedIndex={selectedIndex}
            selectedPaths={selectedPaths}
            gridSize={gridSize}
            onEditRequest={handleOpenEntry}
            favorites={favoritePaths}
            onFavoriteToggle={handleFavoriteToggle}
            cutPaths={clipBoard?.mode === 'cut' ? new Set(clipBoard.paths) : undefined}
            onMoveDrop={handleInternalMove}
            dragPaths={selectedPaths.size > 0 ? Array.from(selectedPaths) : undefined}
          />
        ) : (
          <FileList
            entries={filteredEntries}
            sortBy={sortBy}
            sortDir={sortDir}
            onSort={handleSort}
            onSelect={(entry, ev) => handleSelect(entry, ev)}
            onContextMenu={handleContextMenu}
            showDir={recentMode || recentOpenedMode}
            selectedIndex={selectedIndex}
            selectedPaths={selectedPaths}
            onEditRequest={handleOpenEntry}
            favorites={favoritePaths}
            onFavoriteToggle={handleFavoriteToggle}
            cutPaths={clipBoard?.mode === 'cut' ? new Set(clipBoard.paths) : undefined}
            onMoveDrop={handleInternalMove}
            dragPaths={selectedPaths.size > 0 ? Array.from(selectedPaths) : undefined}
          />
        )}
      </div>


      {!loading && filteredEntries.length === 0 && recentOpenedMode && (
        <div style={{
          display: 'flex', justifyContent: 'center',
          padding: SPACING.md, borderTop: '1px solid var(--border)',
          fontSize: FONT_SIZE.sm, color: 'var(--text-secondary)',
        }}>
          {t(locale, 'fileBrowser.recentOpenedEmpty')}
        </div>
      )}

      {!loading && filteredEntries.length === 0 && !recentOpenedMode && (
        <div style={{
          display: 'flex', gap: SPACING.sm, justifyContent: 'center',
          padding: SPACING.md, borderTop: '1px solid var(--border)',
        }}>
          <button className="btn btn-ghost" onClick={() => handleNewFile(currentPath)}>{t(locale, 'fileBrowser.newFile')}</button>
          <button className="btn btn-primary" onClick={() => handleNewFolder(currentPath)}>{t(locale, 'fileBrowser.newFolder')}</button>
          {clipBoard && clipBoard.paths.length > 0 && (
            <button className="btn btn-ghost" onClick={() => { void handlePaste(); }}>{t(locale, 'fileBrowser.paste')}</button>
          )}
        </div>
      )}

      {/* Status bar */}
      {!loading && filteredEntries.length > 0 && (() => {
        const dirs = filteredEntries.filter(e => e.isDir).length;
        const files = filteredEntries.length - dirs;
        const totalSize = filteredEntries.reduce((sum, e) => sum + (e.isDir ? 0 : e.size), 0);
        return (
          <div style={{
            display: 'flex', alignItems: 'center', gap: SPACING.md,
            padding: `${SPACING.sm}px ${SPACING.md}px`, fontSize: FONT_SIZE.sm, fontFamily: 'var(--font-mono)',
            color: 'var(--text-secondary)',
            borderTop: '1px solid var(--border)',
            background: 'var(--surface)',
          }}>
            <span>{t(locale, 'fileBrowser.statusItems').replace('{count}', String(filteredEntries.length))}</span>
            {dirs > 0 && <span>{t(locale, 'fileBrowser.statusFolders').replace('{count}', String(dirs))}</span>}
            {files > 0 && <span>{t(locale, 'fileBrowser.statusFiles').replace('{count}', String(files))}</span>}
            {totalSize > 0 && <span>{fmtSize(totalSize)}</span>}
            {selectedPaths.size > 0 && (
              <span style={{ color: 'var(--primary)' }}>
                {t(locale, 'fileBrowser.selectedCount').replace('{count}', String(selectedPaths.size))}
              </span>
            )}
            {clipBoard && (
              <span style={{ opacity: 0.75 }}>
                {clipBoard.mode === 'cut' ? t(locale, 'fileBrowser.cut') : t(locale, 'fileBrowser.copy')}
                {' · '}{clipBoard.paths.length}
              </span>
            )}
            <div style={{ flex: 1 }} />
            {selectedIndex >= 0 && selectedIndex < filteredEntries.length && selectedPaths.size <= 1 && (
              <span style={{ opacity: 0.7 }} title={t(locale, 'fileBrowser.multiHint')}>
                {filteredEntries[selectedIndex]!.name}
              </span>
            )}
            <span
              onClick={() => setDiskUsageTarget(currentPath)}
              style={{
                cursor: 'pointer', color: 'var(--primary)',
                textDecoration: 'none', fontSize: FONT_SIZE.sm,
              }}
              onMouseEnter={(e) => { (e.currentTarget as HTMLElement).style.textDecoration = 'underline'; }}
              onMouseLeave={(e) => { (e.currentTarget as HTMLElement).style.textDecoration = 'none'; }}
            >
              {t(locale, 'fileBrowser.diskUsage')} →
            </span>
          </div>
        );
      })()}

      {/* Context menu */}
      {contextMenu && (
        <FileContextMenu
          entry={contextMenu.entry}
          x={contextMenu.x}
          y={contextMenu.y}
          mode={contextMenu.mode}
          parentDir={currentPath}
          onClose={() => setContextMenu(null)}
          onOpen={handleOpen}
          onOpenInTerminal={handleOpenInTerminal}
          onRevealInFinder={handleRevealInFinder}
          onOpenInEditor={handleOpenInEditor}
          onPreview={handlePreview}
          onEditImage={handleEditRequest}
          onDiskUsage={handleDiskUsage}
          onRename={handleRename}
          onTrash={handleTrash}
          onDuplicate={handleDuplicate}
          onCopy={(entry) => { void handleCopyEntry(entry); }}
          onCut={(entry) => { void handleCutEntry(entry); }}
          onPaste={() => { void handlePaste(); }}
          canPaste={!!clipBoard && clipBoard.paths.length > 0}
          onCopyPath={handleCopyPath}
          onCopyImage={handleCopyImage}
          onOpenDefault={(entry) => { void handleOpenWith(entry, 'default'); }}
          onNewFile={handleNewFile}
          onNewFolder={handleNewFolder}
          onFavorite={(entry) => { void toggleFavorite(entry.path); }}
          onUnfavorite={(entry) => { void toggleFavorite(entry.path); }}
          isFavorite={contextMenu.entry ? favoritePaths.includes(contextMenu.entry.path) : isFavorite}
        />
      )}

      {/* Rename dialog */}
      <Modal
        isOpen={!!renameTarget}
        onClose={() => setRenameTarget(null)}
        title={t(locale, 'fileBrowser.dialogRename')}
        width={340}
      >
        <input
          type="text"
          value={renameValue}
          onChange={(e) => setRenameValue(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && handleRenameConfirm()}
          className="input"
          style={{ width: '100%', fontSize: FONT_SIZE.lg }}
          autoFocus
          onFocus={(e) => {
            const v = e.currentTarget.value;
            const dot = v.lastIndexOf('.');
            // Select stem only for files with extension (not dotfiles)
            if (dot > 0 && !renameTarget?.isDir) {
              e.currentTarget.setSelectionRange(0, dot);
            } else {
              e.currentTarget.select();
            }
          }}
        />
        <div style={{ display: 'flex', gap: SPACING.sm, marginTop: 14, justifyContent: 'flex-end' }}>
          <button className="btn btn-ghost" onClick={() => setRenameTarget(null)}>{t(locale, 'common.cancel')}</button>
          <button className="btn btn-primary" onClick={handleRenameConfirm}>{t(locale, 'fileBrowser.dialogRenameBtn')}</button>
        </div>
      </Modal>

      {/* New file/folder dialog */}
      <Modal
        isOpen={!!newItemTarget}
        onClose={() => setNewItemTarget(null)}
        title={newItemTarget?.type === 'file' ? t(locale, 'fileBrowser.dialogNewFile') : t(locale, 'fileBrowser.dialogNewFolder')}
        width={340}
      >
        {newItemTarget && (
          <>
            <input
              type="text"
              value={newItemName}
              onChange={(e) => setNewItemName(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && handleNewItemConfirm()}
              placeholder={newItemTarget.type === 'file' ? t(locale, 'fileBrowser.placeholderFileName') : t(locale, 'fileBrowser.placeholderFolderName')}
              className="input"
              style={{ width: '100%', fontSize: FONT_SIZE.lg }}
              autoFocus
            />
            <div style={{ display: 'flex', gap: SPACING.sm, marginTop: 14, justifyContent: 'flex-end' }}>
              <button className="btn btn-ghost" onClick={() => setNewItemTarget(null)}>{t(locale, 'common.cancel')}</button>
              <button className="btn btn-primary" onClick={handleNewItemConfirm}>{t(locale, 'fileBrowser.dialogCreate')}</button>
            </div>
          </>
        )}
      </Modal>

      {/* Global name/content search (fanbox cmdk-class, scoped + recursive) */}
      {globalSearchOpen && (
        <FileSearch
          rootPath={currentPath}
          onClose={() => setGlobalSearchOpen(false)}
          onNavigate={async (path) => {
            setGlobalSearchOpen(false);
            await resolveAndNavigate(path);
          }}
        />
      )}

      {/* Disk Usage Panel (overlay) */}
      {diskUsageTarget && (
        <DiskUsagePanel
          dirPath={diskUsageTarget}
          onClose={() => setDiskUsageTarget(null)}
          onNavigate={(path) => { setDiskUsageTarget(null); navigateTo(path); }}
        />
      )}

      {/* Trash confirmation dialog */}
      <ConfirmDialog
        open={!!trashTarget}
        title={t(locale, 'fileBrowser.moveToTrash')}
        message={trashTarget ? t(locale, 'fileBrowser.confirmMoveToTrash').replace('{name}', trashTarget.name) : ''}
        confirmLabel={t(locale, 'fileBrowser.moveToTrash')}
        cancelLabel={t(locale, 'common.cancel')}
        danger
        onConfirm={doTrash}
        onCancel={() => setTrashTarget(null)}
      />

      {/* Toast */}
      {toast && (
        <div
          role="status"
          aria-live="polite"
          style={{
            position: 'fixed', bottom: 24, left: '50%', transform: 'translateX(-50%)',
            background: 'var(--surface)', border: '1px solid var(--border)',
            padding: `${SPACING.sm}px 18px`, borderRadius: BORDER_RADIUS.xl, fontSize: FONT_SIZE.sm, color: 'var(--text)',
            boxShadow: 'var(--shadow-popup)', zIndex: 200, animation: 'fadeIn 150ms ease',
          }}
        >
          {toast}
        </div>
      )}
    </div>
  );
}
