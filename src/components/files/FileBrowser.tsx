'use client';

import { useState, useEffect, useCallback, useRef } from 'react';
import { type FileEntry, type FileKind } from '@/types/file';
import { t, type Locale } from '@/i18n';
import FileGrid from './FileGrid';
import FileList from './FileList';
import FileBreadcrumb from './FileBreadcrumb';
import FileToolbar from './FileToolbar';
import FileContextMenu from './FileContextMenu';
import DiskUsagePanel from './DiskUsagePanel';
import Skeleton from '@/components/ui/Skeleton';
import ConfirmDialog from '@/components/ui/ConfirmDialog';
import { useFocusTrap } from '@/lib/useFocusTrap';
import { pushRecentFile } from '@/lib/recent-files-client';
import { webFsClient } from '@/lib/web-fs-client';
import { fmtSize } from '@/lib/format';

/** Electron IPC 可用时用 nativesAPI.fs，否则降级到 webFsClient (Next.js API Routes) */
function getFsApi() {
  const native = (window as any).nativesAPI?.fs;
  return native || webFsClient;
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
  const renameTrap = useFocusTrap();
  const newItemTrap = useFocusTrap();
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
  const [favorites, setFavorites] = useState<string[]>([]);
  const [diskUsageTarget, setDiskUsageTarget] = useState<string | null>(null);
  const [locale, setLocale] = useState<Locale>('zh');
  const [recentMode, setRecentMode] = useState(false);

  // Navigation history for back/forward
  const historyRef = useRef<string[]>(['/']);
  const historyIndexRef = useRef(0);

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
        if (stored) setFavorites(JSON.parse(stored));
      } catch { /* ignore */ }
      try {
        const saved = await window.nativesAPI?.getLocale?.();
        if (saved) setLocale(saved === 'en' ? 'en' : 'zh');
      } catch { /* ignore */ }
    }
    load();
  }, []);

  const isFavorite = favorites.includes(currentPath);
  const toggleFavorite = useCallback(async () => {
    const next = isFavorite
      ? favorites.filter((f) => f !== currentPath)
      : [...favorites, currentPath];
    setFavorites(next);
    try {
      await window.nativesAPI?.db?.set?.('settings:favorites', JSON.stringify(next));
      window.dispatchEvent(new CustomEvent('favorites-changed'));
      showToast(isFavorite ? t(locale, 'fileBrowser.removedFromFavorites') : t(locale, 'fileBrowser.addedToFavorites'));
    } catch { /* ignore */ }
  }, [currentPath, favorites, isFavorite, showToast]);

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
    loadEntries();
  }, [loadEntries]);

  // Keyboard shortcuts: Cmd+[ back, Cmd+] forward
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.metaKey && e.key === '[') { e.preventDefault(); goBack(); }
      if (e.metaKey && e.key === ']') { e.preventDefault(); goForward(); }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [goBack, goForward]);

  // Listen for Header action events
  useEffect(() => {
    const handler = (e: Event) => {
      const detail = (e as CustomEvent).detail;
      if (!detail) return;
      if (detail.type === 'viewMode') setViewMode(detail.value);
      if (detail.type === 'sortBy') setSortBy(detail.value);
      if (detail.type === 'sortDir') setSortDir((prev) => prev === 'asc' ? 'desc' : 'asc');
      if (detail.type === 'showHidden') setShowHidden((prev) => !prev);
    };
    window.addEventListener('header-file-action', handler);
    return () => window.removeEventListener('header-file-action', handler);
  }, []);

  // Listen for external navigation events (from sidebar quick access)
  useEffect(() => {
    // Check for pending path set before mount (race condition fix)
    const pending = (window as any).__pendingNavigateFiles;
    if (typeof pending === 'string') {
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

  const handleSelect = (entry: FileEntry) => {
    if (entry.isDir) {
      navigateTo(entry.path);
    } else {
      pushRecentFile(entry.path);
      onFileSelect?.(entry);
    }
  };

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
  }, [showToast]);

  const handleOpenInEditor = useCallback((entry: FileEntry) => {
    const api = (window as any).nativesAPI?.shell;
    if (api?.openPath) {
      api.openPath(entry.path);
    } else {
      navigator.clipboard.writeText(entry.path);
      showToast(t(locale, 'fileBrowser.copyPath'));
    }
  }, [showToast]);

  const handleOpenInTerminal = useCallback((dir: string) => {
    const api = (window as any).nativesAPI;
    // Try terminal:openInDir IPC first (Electron)
    if (api?.terminal?.write) {
      window.dispatchEvent(new CustomEvent('toggle-terminal'));
      // Also copy cd shortcut as fallback
    }
    navigator.clipboard.writeText(`cd "${dir}"`);
    showToast(t(locale, 'fileBrowser.copyAsCd'));
  }, [showToast]);

  const handlePreview = useCallback((entry: FileEntry) => {
    onFileSelect?.(entry);
  }, [onFileSelect]);

  const handleDiskUsage = useCallback((dir: string) => {
    setDiskUsageTarget(dir);
  }, []);

  const doTrash = useCallback(async () => {
    if (!trashTarget) return;
    try {
      const result = await getFsApi().trashEntry(trashTarget.path);
      if (result?.ok) {
        showToast(t(locale, 'fileBrowser.trashed'));
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

  // ── Event bridge: broadcast file-browser state for Header ──
  useEffect(() => {
    const detail = {
      viewMode, sortBy, sortDir, showHidden,
      segments: segments.length > 0 ? segments : ['/'],
      isFavorite, breadcrumbPath: currentPath,
    };
    window.dispatchEvent(new CustomEvent('header-file-state', { detail }));
  }, [viewMode, sortBy, sortDir, showHidden, segments, isFavorite, currentPath]);

  return (
    <div style={{
      display: 'flex',
      flexDirection: 'column',
      height: '100%',
      background: 'var(--bg, #0b0c0a)',
    }}>
      {/* Navigation */}
      <FileBreadcrumb
        segments={segments.length > 0 ? segments : ['/']}
        onNavigate={handleNavigate}
        isFavorite={isFavorite}
        onToggleFavorite={toggleFavorite}
      />
      <FileToolbar
        searchQuery={searchQuery}
        recentMode={recentMode}
        canGoBack={canGoBack}
        canGoForward={canGoForward}
        onSearchChange={setSearchQuery}
        onRecentModeToggle={() => setRecentMode((v) => !v)}
        onRefresh={loadEntries}
        onNewFile={() => handleNewFile(currentPath)}
        onNewFolder={() => handleNewFolder(currentPath)}
        onBack={goBack}
        onForward={goForward}
      />

      {/* File area */}
      <div
        style={{ flex: 1, overflow: 'auto' }}
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
        {loading ? (
          <div style={{ padding: viewMode === 'grid' ? 12 : 0 }}>
            {viewMode === 'grid' ? (
              <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(140px, 1fr))', gap: 10 }}>
                {Array.from({ length: 8 }, (_, i) => <Skeleton key={i} variant="card" />)}
              </div>
            ) : (
              <Skeleton variant="table" lines={10} />
            )}
          </div>
        ) : viewMode === 'grid' ? (
          <FileGrid entries={entries} onSelect={handleSelect} onContextMenu={handleContextMenu} />
        ) : (
          <FileList
            entries={entries}
            sortBy={sortBy}
            sortDir={sortDir}
            onSort={handleSort}
            onSelect={handleSelect}
            onContextMenu={handleContextMenu}
            showDir={recentMode}
          />
        )}
      </div>

      {/* Status bar (fanbox-style) */}
      {!loading && entries.length > 0 && (() => {
        const dirs = entries.filter(e => e.isDir).length;
        const files = entries.length - dirs;
        const totalSize = entries.reduce((sum, e) => sum + (e.isDir ? 0 : e.size), 0);
        return (
          <div style={{
            display: 'flex', alignItems: 'center', gap: 12,
            padding: '4px 12px', fontSize: 11, fontFamily: 'var(--font-mono)',
            color: 'var(--text-faint, #62655a)',
            borderTop: '1px solid var(--border, #262920)',
            background: 'var(--bg-2, #131410)',
          }}>
            <span>{entries.length} 项</span>
            {dirs > 0 && <span>{dirs} 个文件夹</span>}
            {files > 0 && <span>{files} 个文件</span>}
            {totalSize > 0 && <span>{fmtSize(totalSize)}</span>}
            <div style={{ flex: 1 }} />
            <span
              onClick={() => setDiskUsageTarget(currentPath)}
              style={{
                cursor: 'pointer', color: 'var(--accent,#FFF5E6)',
                textDecoration: 'none', fontSize: 11,
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
      {renameTarget && (
        <div
          ref={renameTrap.dialogRef}
          role="dialog"
          aria-modal="true"
          aria-label={t(locale, 'fileBrowser.dialogRename')}
          className="anim-editRipple"
          style={{
            position: 'fixed', inset: 0, background: 'rgba(0,0,0,0.5)',
            display: 'flex', alignItems: 'center', justifyContent: 'center', zIndex: 60,
          }}
          onClick={() => setRenameTarget(null)}
          onKeyDown={(e) => {
            renameTrap.handleKeyDown(e);
            if (e.key === 'Escape') setRenameTarget(null);
          }}
        >
          <div style={{
            background: 'var(--bg-2,#131410)', border: '1px solid var(--border,#262920)',
            borderRadius: 10, padding: 20, width: 340,
          }} onClick={(e) => e.stopPropagation()}>
            <div style={{ fontSize: 13, fontWeight: 600, color: 'var(--text)', marginBottom: 12 }}>
              {t(locale, 'fileBrowser.dialogRename')}
            </div>
            <input
              type="text"
              value={renameValue}
              onChange={(e) => setRenameValue(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && handleRenameConfirm()}
              style={{
                width: '100%', padding: '8px 10px', background: 'var(--bg,#0b0c0a)',
                border: '1px solid var(--border,#262920)', borderRadius: 6,
                color: 'var(--text)', fontSize: 13, outline: 'none',
              }}
              autoFocus
            />
            <div style={{ display: 'flex', gap: 8, marginTop: 14, justifyContent: 'flex-end' }}>
              <button className="btn" onClick={() => setRenameTarget(null)}>{t(locale, 'common.cancel')}</button>
              <button className="btn btn-primary" onClick={handleRenameConfirm}>{t(locale, 'fileBrowser.dialogRenameBtn')}</button>
            </div>
          </div>
        </div>
      )}

      {/* New file/folder dialog */}
      {newItemTarget && (
        <div
          ref={newItemTrap.dialogRef}
          role="dialog"
          aria-modal="true"
          aria-label={newItemTarget.type === 'file' ? t(locale, 'fileBrowser.dialogNewFile') : t(locale, 'fileBrowser.dialogNewFolder')}
          className="anim-editRipple"
          style={{
            position: 'fixed', inset: 0, background: 'rgba(0,0,0,0.5)',
            display: 'flex', alignItems: 'center', justifyContent: 'center', zIndex: 60,
          }}
          onClick={() => setNewItemTarget(null)}
          onKeyDown={(e) => {
            newItemTrap.handleKeyDown(e);
            if (e.key === 'Escape') setNewItemTarget(null);
          }}
        >
          <div style={{
            background: 'var(--bg-2,#131410)', border: '1px solid var(--border,#262920)',
            borderRadius: 10, padding: 20, width: 340,
          }} onClick={(e) => e.stopPropagation()}>
            <div style={{ fontSize: 13, fontWeight: 600, color: 'var(--text)', marginBottom: 12 }}>
              {newItemTarget.type === 'file' ? t(locale, 'fileBrowser.dialogNewFile') : t(locale, 'fileBrowser.dialogNewFolder')}
            </div>
            <input
              type="text"
              value={newItemName}
              onChange={(e) => setNewItemName(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && handleNewItemConfirm()}
              placeholder={newItemTarget.type === 'file' ? t(locale, 'fileBrowser.placeholderFileName') : t(locale, 'fileBrowser.placeholderFolderName')}
              style={{
                width: '100%', padding: '8px 10px', background: 'var(--bg,#0b0c0a)',
                border: '1px solid var(--border,#262920)', borderRadius: 6,
                color: 'var(--text)', fontSize: 13, outline: 'none',
              }}
              autoFocus
            />
            <div style={{ display: 'flex', gap: 8, marginTop: 14, justifyContent: 'flex-end' }}>
              <button className="btn" onClick={() => setNewItemTarget(null)}>{t(locale, 'common.cancel')}</button>
              <button className="btn btn-primary" onClick={handleNewItemConfirm}>{t(locale, 'fileBrowser.dialogCreate')}</button>
            </div>
          </div>
        </div>
      )}

      {/* Disk Usage Panel (overlay) */}
      {diskUsageTarget && (
        <DiskUsagePanel
          dirPath={diskUsageTarget}
          onClose={() => setDiskUsageTarget(null)}
          onNavigate={(path) => { setDiskUsageTarget(null); navigateTo(path); }}
        />
      )}

      {/* Toast */}
      {toast && (
        <div
          role="status"
          aria-live="polite"
          style={{
            position: 'fixed', bottom: 24, left: '50%', transform: 'translateX(-50%)',
            background: 'var(--bg-3,#1c1e17)', border: '1px solid var(--border,#262920)',
            padding: '10px 18px', borderRadius: 10, fontSize: 13, color: 'var(--text)',
            zIndex: 200, animation: 'fadeIn 0.18s cubic-bezier(0.16, 1, 0.3, 1)',
          }}
        >
          {toast}
        </div>
      )}
    </div>
  );
}
