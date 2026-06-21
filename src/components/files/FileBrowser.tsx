'use client';

import { startTransition, useState, useEffect, useCallback, useRef, useMemo } from 'react';
import { SPACING, FONT_SIZE, BORDER_RADIUS } from '@/lib/design-tokens';
import { type FileEntry, type FileKind } from '@/types/file';
import { t, type Locale } from '@/i18n';
import FileGrid from './FileGrid';
import FileList from './FileList';
import FileContextMenu from './FileContextMenu';
import DiskUsagePanel from './DiskUsagePanel';
import Skeleton from '@/components/ui/Skeleton';
import ConfirmDialog from '@/components/ui/ConfirmDialog';
import Modal from '@/components/ui/Modal';
import { pushRecentFile } from '@/lib/recent-files-client';
import { fmtSize } from '@/lib/format';
import { useFileDrop } from '@/lib/use-file-drop';

export interface FavoriteItem {
  path: string;
  addedAt: number;
}

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
  const [selectedIndex, setSelectedIndex] = useState(-1);
  const [gridSize, setGridSize] = useState<'sm' | 'md' | 'lg'>(() => {
    if (typeof window !== 'undefined') {
      return (localStorage.getItem('file-grid-size') as 'sm' | 'md' | 'lg') || 'md';
    }
    return 'md';
  });

  // Navigation history for back/forward
  const historyRef = useRef<string[]>(['/']);
  const historyIndexRef = useRef(0);
  const fileAreaRef = useRef<HTMLDivElement>(null);
  const gridContainerRef = useRef<HTMLDivElement>(null);

  const navigateTo = useCallback((path: string) => {
    // Truncate forward history and append
    const hist = historyRef.current.slice(0, historyIndexRef.current + 1);
    hist.push(path);
    historyRef.current = hist;
    historyIndexRef.current = hist.length - 1;
    setCurrentPath(path);
  }, []);

  const goBack = useCallback(() => {
    if (historyIndexRef.current > 0) {
      historyIndexRef.current--;
      setCurrentPath(historyRef.current[historyIndexRef.current]!);
    }
  }, []);

  const goForward = useCallback(() => {
    if (historyIndexRef.current < historyRef.current.length - 1) {
      historyIndexRef.current++;
      setCurrentPath(historyRef.current[historyIndexRef.current]!);
    }
  }, []);

  const canGoBack = historyIndexRef.current > 0;
  const canGoForward = historyIndexRef.current < historyRef.current.length - 1;

  const showToast = useCallback((msg: string) => {
    setToast(msg);
    setTimeout(() => setToast(null), 2200);
  }, []);

  // Load favorites and locale
  useEffect(() => {
    async function load() {
      try {
        const stored = await window.nativesAPI?.db?.get?.('settings:favorites');
        if (stored) {
          const parsed = JSON.parse(stored as string);
          // Migrate old format: string[] → FavoriteItem[]
          if (Array.isArray(parsed) && parsed.length > 0 && typeof parsed[0] === 'string') {
            const migrated: FavoriteItem[] = (parsed as string[]).map((p, i) => ({ path: p, addedAt: Date.now() + i }));
            setFavorites(migrated);
          } else {
            setFavorites(parsed as FavoriteItem[]);
          }
        }
      } catch { /* ignore */ }
      try {
        const saved = await window.nativesAPI?.getLocale?.();
        if (saved) setLocale(saved === 'en' ? 'en' : 'zh');
      } catch { /* ignore */ }
    }
    load();
  }, []);

  const isFavorite = favorites.some((f) => f.path === currentPath);
  const toggleFavorite = useCallback(async (targetPath?: string) => {
    const path = targetPath ?? currentPath;
    const existing = favorites.find((f) => f.path === path);
    const next = existing
      ? favorites.filter((f) => f.path !== path)
      : [...favorites, { path, addedAt: Date.now() }];
    setFavorites(next);
    try {
      await window.nativesAPI?.db?.set?.('settings:favorites', JSON.stringify(next));
      window.dispatchEvent(new CustomEvent('favorites-changed'));
      showToast(existing ? t(locale, 'fileBrowser.removedFromFavorites') : t(locale, 'fileBrowser.addedToFavorites'));
    } catch { /* ignore */ }
  }, [currentPath, favorites, showToast, locale]);

  const favoritePaths = useMemo(() => favorites.map((f) => f.path), [favorites]);
  const handleFavoriteToggle = useCallback((entry: FileEntry) => {
    void toggleFavorite(entry.path);
  }, [toggleFavorite]);

  const loadEntries = useCallback(async () => {
    setLoading(true);
    try {
      const fsApi = getFsApi();

      if (recentMode) {
        // 最近修改模式：调用后端递归扫描，返回按 mtime 降序的文件
        const recentData = await fsApi.recentFiles(currentPath);
        if (recentData && Array.isArray(recentData)) {
          // 转换 WalkFile 格式 → FileEntry，填入 dirHint
          const recentEntries: FileEntry[] = recentData.map((f: any) => {
            const dir = f.path.substring(0, f.path.lastIndexOf('/')) || '/';
            const name = f.path.split('/').pop() || '';
            return {
              name,
              path: f.path,
              isDir: false,
              kind: 'text' as FileKind,
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
        const options = { sortBy, sortDir, showHidden };
        const data = await fsApi.listDir(currentPath, options);
        setEntries(data || []);
      }
    } catch (err) {
      console.error('[FileBrowser] loadEntries error:', err);
      setEntries([]);
    } finally {
      setLoading(false);
    }
  }, [currentPath, sortBy, sortDir, showHidden, recentMode]);

  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect
    loadEntries();
  }, [loadEntries]);

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
          console.error('[FileBrowser] fs.importFiles API not available');
          showToast(t(locale, 'fileBrowser.importFailed'));
        }
      } catch (err) {
        console.error('[FileBrowser] import files failed:', err);
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
      if (detail.type === 'sortBy') { setSortBy(detail.value); setSortDir('asc'); }
      if (detail.type === 'sortDir') setSortDir((prev) => prev === 'asc' ? 'desc' : 'asc');
      if (detail.type === 'showHidden') setShowHidden((prev) => !prev);
      if (detail.type === 'search') setSearchQuery(detail.value ?? '');
      if (detail.type === 'newFolder') {
        // Use detail.value or fall back to current path from state
        const dir = detail.value ?? '';
        if (dir) setNewItemTarget({ parentDir: dir, type: 'folder' });
      }
      if (detail.type === 'gridSize') setGridSize(detail.value);
    };
    window.addEventListener('header-file-action', handler);
    return () => window.removeEventListener('header-file-action', handler);
  }, []);

  // Listen for external navigation events (from sidebar quick access)
  useEffect(() => {
    // Check for pending path set before mount (race condition fix)
    const pending = (window as any).__pendingNavigateFiles;
    if (typeof pending === 'string') {
    // eslint-disable-next-line react-hooks/set-state-in-effect
      navigateTo(pending);
      delete (window as any).__pendingNavigateFiles;
    }

    const handler = (e: Event) => {
      const path = (e as CustomEvent).detail;
      if (typeof path === 'string') {
        navigateTo(path);
      }
    };
    window.addEventListener('navigate-files', handler);
    return () => window.removeEventListener('navigate-files', handler);
  }, []);

  const handleSelect = useCallback((entry: FileEntry) => {
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

  const handleSort = (newSortBy: 'name' | 'mtime' | 'size') => {
    if (newSortBy === sortBy) {
      setSortDir(sortDir === 'asc' ? 'desc' : 'asc');
    } else {
      setSortBy(newSortBy);
      setSortDir('asc');
    }
  };

  const handleContextMenu = (e: React.MouseEvent, entry: FileEntry) => {
    e.preventDefault();
    // Stop propagation so the blank-area handler on the parent doesn't fire
    e.stopPropagation();
    setContextMenu({ x: e.clientX, y: e.clientY, entry, mode: entry.isDir ? 'dir' : 'file' });
  };

  // Context menu actions
  const handleOpen = useCallback((entry: FileEntry) => {
    if (entry.isDir) {
      navigateTo(entry.path);
    } else {
      onFileSelect?.(entry);
    }
  }, [onFileSelect, navigateTo]);

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
  }, [renameTarget, renameValue, loadEntries, showToast]);

  const handleTrash = useCallback((entry: FileEntry) => {
    setTrashTarget(entry);
    return;
  }, []);

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
    if (api?.terminal?.openInDir) {
      // Electron: create new PTY session and cd into dir
      const result = await api.terminal.openInDir(dir);
      if (result?.sessionId) {
        window.dispatchEvent(new CustomEvent('toggle-terminal'));
      }
    } else {
      // Web fallback: copy cd command
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
  }, [newItemTarget, newItemName, loadEntries, showToast]);

  const segments = currentPath.split('/').filter(Boolean);

  // Search filter (client-side)
  const filteredEntries = useMemo(() => {
    if (!searchQuery) return entries;
    const q = searchQuery.toLowerCase();
    return entries.filter(e => e.name.toLowerCase().includes(q));
  }, [entries, searchQuery]);

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
            handleSelect(entry);
          }
          break;
        }
        case 'Backspace': {
          e.preventDefault();
          const parentPath = currentPath.substring(0, currentPath.lastIndexOf('/')) || '/';
          if (parentPath !== currentPath) {
            navigateTo(parentPath);
          }
          break;
        }
        case 'F2': {
          if (selectedIndex < 0 || selectedIndex >= list.length) break;
          e.preventDefault();
          handleRename(list[selectedIndex]!);
          break;
        }
        case 'Delete': {
          if (selectedIndex < 0 || selectedIndex >= list.length) break;
          if (!e.metaKey) break;
          e.preventDefault();
          handleTrash(list[selectedIndex]!);
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
  }, [filteredEntries, selectedIndex, viewMode, getGridColumns, onFileSelect, currentPath, renameTarget, newItemTarget, trashTarget, contextMenu, toggleFavorite, handleSelect, handleRename, handleTrash]);

  // Reset selectedIndex when entries or path change
  useEffect(() => {
    startTransition(() => { setSelectedIndex(-1); });
  }, [currentPath, entries, searchQuery]);

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

  // ── Event bridge: broadcast file-browser state for Header ──
  useEffect(() => {
    const detail = {
      viewMode, sortBy, sortDir, showHidden, gridSize,
      segments: segments.length > 0 ? segments : ['/'],
      isFavorite, breadcrumbPath: currentPath, projectBadge: detectedProject,
    };
    window.dispatchEvent(new CustomEvent('header-file-state', { detail }));
  }, [viewMode, sortBy, sortDir, showHidden, gridSize, segments, isFavorite, currentPath, detectedProject]);

  return (
    <div style={{
      display: 'flex',
      flexDirection: 'column',
      height: '100%',
      background: 'var(--vibe-content-bg)',
      position: 'relative',
    }}>
      {/* File area — drop zone covers entire height including empty space */}
      <div
        {...dragHandlers}
        style={{ flex: 1, overflow: 'auto', position: 'relative' }}
        role="listbox"
        aria-label={t(locale, 'fileBrowser.ariaLabelFiles')}
        tabIndex={0}
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
              border: '2px dashed var(--accent)',
              borderRadius: 'var(--radius, 4px)',
              background: 'var(--accent-soft, rgba(205,242,75,0.08))',
              color: 'var(--accent)',
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
            onSelect={handleSelect}
            onContextMenu={handleContextMenu}
            selectedIndex={selectedIndex}
            gridSize={gridSize}
            onEditRequest={handleEditRequest}
            favorites={favoritePaths}
            onFavoriteToggle={handleFavoriteToggle}
          />
        ) : (
          <FileList
            entries={filteredEntries}
            sortBy={sortBy}
            sortDir={sortDir}
            onSort={handleSort}
            onSelect={handleSelect}
            onContextMenu={handleContextMenu}
            showDir={recentMode}
            selectedIndex={selectedIndex}
            onEditRequest={handleEditRequest}
            favorites={favoritePaths}
            onFavoriteToggle={handleFavoriteToggle}
          />
        )}
      </div>

      {/* Status bar */}
      {!loading && filteredEntries.length > 0 && (() => {
        const dirs = filteredEntries.filter(e => e.isDir).length;
        const files = filteredEntries.length - dirs;
        const totalSize = filteredEntries.reduce((sum, e) => sum + (e.isDir ? 0 : e.size), 0);
        return (
          <div style={{
            display: 'flex', alignItems: 'center', gap: SPACING.md,
            padding: `${SPACING.sm}px ${SPACING.md}px`, fontSize: FONT_SIZE.sm, fontFamily: 'var(--font-mono)',
            color: 'var(--vibe-btn-text)',
            borderTop: '1px solid var(--vibe-btn-border)',
            background: 'var(--vibe-toolbar-bg)',
          }}>
            <span>{filteredEntries.length} 项</span>
            {dirs > 0 && <span>{dirs} 个文件夹</span>}
            {files > 0 && <span>{files} 个文件</span>}
            {totalSize > 0 && <span>{fmtSize(totalSize)}</span>}
            <div style={{ flex: 1 }} />
            <span
              onClick={() => setDiskUsageTarget(currentPath)}
              style={{
                cursor: 'pointer', color: 'var(--accent)',
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
          onNewFile={handleNewFile}
          onNewFolder={handleNewFolder}
          onFavorite={(entry) => {
            const api = (window as any).nativesAPI;
            if (!isFavorite) {
              toggleFavorite();
            }
          }}
          onUnfavorite={(entry) => {
            if (isFavorite) {
              toggleFavorite();
            }
          }}
          isFavorite={isFavorite}
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
            background: 'var(--vibe-btn-bg)', border: '1px solid var(--vibe-btn-border)',
            padding: `${SPACING.sm}px 18px`, borderRadius: BORDER_RADIUS.xl, fontSize: FONT_SIZE.lg, color: 'var(--text)',
            boxShadow: 'var(--vibe-sidebar-shadow)', zIndex: 200, animation: 'fadeIn 0.18s cubic-bezier(0.16, 1, 0.3, 1)',
            backdropFilter: 'blur(12px)', WebkitBackdropFilter: 'blur(12px)',
          }}
        >
          {toast}
        </div>
      )}
    </div>
  );
}
