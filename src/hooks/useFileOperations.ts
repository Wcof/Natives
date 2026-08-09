'use client';

/**
 * useFileOperations — FileBrowser 文件操作 / 多选操作 / 对话框状态。
 *
 * 职责边界（F3-04 拆分，ARCH-002）：
 * - 剪贴板（copy/cut → paste）、重命名、删除/批量删除、新建文件/文件夹
 * - 解压 / 压缩、复制路径 / 图片、openWith（默认/reveal/terminal/editor）
 * - 批量移动（grid/list 内部 drop 的目标目录移动）
 * - 对话框状态：rename / new item / trash 确认 / disk usage overlay
 *
 * 纯展示渲染（Modal/ConfirmDialog/StatusBar）留在 FileBrowser 组合层；
 * 本 hook 只持有状态与动作，不产出 DOM。
 */

import { useCallback, useState } from 'react';
import { t, type Locale } from '@/i18n';
import { type FileEntry } from '@/types/file';
import { fsApi, archiveApi } from '@/lib/files-api';
import { FILE_EVENTS, dispatchFileEvent } from '@/lib/file-events';
import { removeRecentFile } from '@/lib/recent-files-client';

/** 当前目标路径解析：多选 > 传入条目 > 光标条目（纯函数，便于单测） */
export function resolveTargetPaths(
  selectedPaths: Set<string>,
  selectedIndex: number,
  filteredEntries: FileEntry[],
  entry?: FileEntry | null,
): string[] {
  if (selectedPaths.size > 0) return Array.from(selectedPaths);
  if (entry) return [entry.path];
  if (selectedIndex >= 0 && selectedIndex < filteredEntries.length) {
    return [filteredEntries[selectedIndex]!.path];
  }
  return [];
}

export interface UseFileOperationsOptions {
  entries: FileEntry[];
  filteredEntries: FileEntry[];
  selectedPaths: Set<string>;
  selectedIndex: number;
  /** 批量删除/移动后清空或剔除选择（selection 状态归 useFileSelection，这里只调用 setter） */
  setSelectedPaths: React.Dispatch<React.SetStateAction<Set<string>>>;
  currentPath: string;
  locale: Locale;
  loadEntries: () => Promise<void>;
  /** 打开/预览类动作登记 selfOpened，3s 窗口内 fs_watch 忽略其假变更 */
  markSelfOpened: (path: string) => void;
  showToast: (msg: string) => void;
}

export interface UseFileOperationsResult {
  clipBoard: { mode: 'copy' | 'cut'; paths: string[] } | null;
  renameTarget: FileEntry | null;
  renameValue: string;
  setRenameTarget: (target: FileEntry | null) => void;
  setRenameValue: (value: string) => void;
  newItemTarget: { parentDir: string; type: 'file' | 'folder' } | null;
  newItemName: string;
  setNewItemTarget: (target: { parentDir: string; type: 'file' | 'folder' } | null) => void;
  setNewItemName: (value: string) => void;
  trashTarget: FileEntry | null;
  setTrashTarget: (target: FileEntry | null) => void;
  diskUsageTarget: string | null;
  setDiskUsageTarget: (target: string | null) => void;
  resolveTargetPaths: (entry?: FileEntry | null) => string[];
  handleRename: (entry: FileEntry) => void;
  handleRenameConfirm: () => Promise<void>;
  handleTrash: (entry: FileEntry) => void;
  doTrash: () => Promise<void>;
  handleDuplicate: (entry: FileEntry) => Promise<void>;
  handleExtract: (entry: FileEntry) => Promise<void>;
  handleCompress: (entry?: FileEntry) => Promise<void>;
  handleCopyEntry: (entry?: FileEntry) => Promise<void>;
  handleCutEntry: (entry?: FileEntry) => Promise<void>;
  handlePaste: () => Promise<void>;
  handleCopyPath: (entry: FileEntry) => Promise<void>;
  handleCopyImage: (entry: FileEntry) => Promise<void>;
  handleOpenWith: (entry: FileEntry, withApp?: 'default' | 'reveal' | 'terminal' | 'editor') => Promise<void>;
  handleBatchTrash: () => Promise<void>;
  handleInternalMove: (sourcePaths: string[], destDir: string) => Promise<void>;
  handleRevealInFinder: (entry: FileEntry) => void;
  handleOpenInEditor: (entry: FileEntry) => void;
  handleOpenInTerminal: (dir: string) => Promise<void>;
  handleDiskUsage: (dir: string) => void;
  handleNewFile: (parentDir: string) => void;
  handleNewFolder: (parentDir: string) => void;
  handleNewItemConfirm: () => Promise<void>;
}

export function useFileOperations({
  entries,
  filteredEntries,
  selectedPaths,
  selectedIndex,
  setSelectedPaths,
  currentPath,
  locale,
  loadEntries,
  markSelfOpened,
  showToast,
}: UseFileOperationsOptions): UseFileOperationsResult {
  const [clipBoard, setClipBoard] = useState<{ mode: 'copy' | 'cut'; paths: string[] } | null>(null);
  const [renameTarget, setRenameTarget] = useState<FileEntry | null>(null);
  const [renameValue, setRenameValue] = useState('');
  const [newItemTarget, setNewItemTarget] = useState<{ parentDir: string; type: 'file' | 'folder' } | null>(null);
  const [newItemName, setNewItemName] = useState('');
  const [trashTarget, setTrashTarget] = useState<FileEntry | null>(null);
  const [diskUsageTarget, setDiskUsageTarget] = useState<string | null>(null);

  const resolveTargetPathsCb = useCallback(
    (entry?: FileEntry | null) => resolveTargetPaths(selectedPaths, selectedIndex, filteredEntries, entry),
    [selectedPaths, selectedIndex, filteredEntries],
  );

  const handleRename = useCallback((entry: FileEntry) => {
    setRenameTarget(entry);
    setRenameValue(entry.name);
  }, []);

  const handleRenameConfirm = useCallback(async () => {
    if (!renameTarget || !renameValue.trim()) return;
    const parentDir = renameTarget.path.substring(0, renameTarget.path.lastIndexOf('/')) || '/';
    const newPath = `${parentDir}/${renameValue.trim()}`;
    try {
      const result = await fsApi().renameEntry(renameTarget.path, newPath);
      if (result?.ok) {
        showToast(t(locale, 'fileBrowser.renamed'));
        dispatchFileEvent(FILE_EVENTS.fileRenamed, { oldPath: renameTarget.path, newPath });
        await loadEntries();
      } else {
        showToast(result?.error || t(locale, 'fileBrowser.renameFailed'));
      }
    } catch {
      showToast(t(locale, 'fileBrowser.renameFailed'));
    }
    setRenameTarget(null);
    setRenameValue('');
  }, [renameTarget, renameValue, loadEntries, showToast, locale]);

  const handleTrash = useCallback(
    (entry: FileEntry) => {
      // fanbox: files trash immediately; directories ask once
      if (entry.isDir) {
        setTrashTarget(entry);
        return;
      }
      void (async () => {
        try {
          const result = await fsApi().trashEntry(entry.path);
          if (result?.ok) {
            showToast(t(locale, 'fileBrowser.trashed'));
            void removeRecentFile(entry.path);
            dispatchFileEvent(FILE_EVENTS.fileTrashed, { path: entry.path });
            setSelectedPaths((prev) => {
              const n = new Set(prev);
              n.delete(entry.path);
              return n;
            });
            await loadEntries();
          } else {
            showToast(result?.error || t(locale, 'fileBrowser.trashFailed'));
          }
        } catch {
          showToast(t(locale, 'fileBrowser.trashFailed'));
        }
      })();
    },
    [loadEntries, showToast, locale, setSelectedPaths],
  );

  const handleDuplicate = useCallback(
    async (entry: FileEntry) => {
      try {
        const fs = fsApi();
        if (typeof fs.duplicateEntry !== 'function') {
          showToast(t(locale, 'fileBrowser.duplicateFailed'));
          return;
        }
        const result = await fs.duplicateEntry(entry.path);
        if (result?.ok) {
          showToast(t(locale, 'fileBrowser.duplicated'));
          await loadEntries();
          if (result.path) {
            dispatchFileEvent(FILE_EVENTS.fileFlash, result.path);
          }
        } else {
          showToast(result?.error || t(locale, 'fileBrowser.duplicateFailed'));
        }
      } catch {
        showToast(t(locale, 'fileBrowser.duplicateFailed'));
      }
    },
    [loadEntries, showToast, locale],
  );

  // ── W7 解压 / 压缩（后端 archive_ops.rs：safe 解压防 zip-slip，zip 打包）──
  const handleExtract = useCallback(
    async (entry: FileEntry) => {
      showToast(t(locale, 'fileBrowser.extracting'));
      try {
        const result = await archiveApi().extract(entry.path);
        if (result?.ok) {
          showToast(t(locale, 'fileBrowser.extracted').replace('{count}', String(result.entryCount)));
          dispatchFileEvent(FILE_EVENTS.fileFlash, result.destPath);
          await loadEntries();
        } else {
          showToast(t(locale, 'fileBrowser.extractFailed'));
        }
      } catch {
        showToast(t(locale, 'fileBrowser.extractFailed'));
      }
    },
    [loadEntries, showToast, locale],
  );

  const handleCompress = useCallback(
    async (entry?: FileEntry) => {
      // 右键目标在多选集内 → 打包整个选中集（与批量删除/移动同语义）
      const paths = resolveTargetPathsCb(entry ?? null);
      if (paths.length === 0) return;
      showToast(t(locale, 'fileBrowser.compressing'));
      try {
        const result = await archiveApi().compress(paths);
        if (result?.ok) {
          showToast(
            t(locale, 'fileBrowser.compressed').replace('{name}', result.zipPath.split('/').pop() || 'zip'),
          );
          dispatchFileEvent(FILE_EVENTS.fileFlash, result.zipPath);
          await loadEntries();
        } else {
          showToast(t(locale, 'fileBrowser.compressFailed'));
        }
      } catch {
        showToast(t(locale, 'fileBrowser.compressFailed'));
      }
    },
    [resolveTargetPathsCb, loadEntries, showToast, locale],
  );

  const handleCopyEntry = useCallback(
    async (entry?: FileEntry) => {
      const paths = resolveTargetPathsCb(entry);
      if (paths.length === 0) return;
      setClipBoard({ mode: 'copy', paths });
      // System pasteboard for Finder paste (fanbox copyFile)
      try {
        const fs = fsApi();
        if (typeof fs.clipboardCopyFiles === 'function') {
          await fs.clipboardCopyFiles(paths);
        } else {
          await navigator.clipboard.writeText(paths.join('\n'));
        }
      } catch {
        try {
          await navigator.clipboard.writeText(paths.join('\n'));
        } catch {
          /* ignore */
        }
      }
      showToast(t(locale, 'fileBrowser.batchCopied').replace('{count}', String(paths.length)));
    },
    [resolveTargetPathsCb, showToast, locale],
  );

  const handleCutEntry = useCallback(
    async (entry?: FileEntry) => {
      const paths = resolveTargetPathsCb(entry);
      if (paths.length === 0) return;
      setClipBoard({ mode: 'cut', paths });
      try {
        const fs = fsApi();
        if (typeof fs.clipboardCopyFiles === 'function') {
          await fs.clipboardCopyFiles(paths);
        }
      } catch {
        /* ignore */
      }
      showToast(t(locale, 'fileBrowser.cutDone').replace('{count}', String(paths.length)));
    },
    [resolveTargetPathsCb, showToast, locale],
  );

  const handlePaste = useCallback(async () => {
    if (!clipBoard || clipBoard.paths.length === 0) {
      showToast(t(locale, 'fileBrowser.pasteEmpty'));
      return;
    }
    try {
      const fs = fsApi();
      if (clipBoard.mode === 'copy') {
        if (typeof fs.copyEntries !== 'function') {
          // fallback sequential
          for (const p of clipBoard.paths) {
            await fs.copyEntry?.(p, currentPath);
          }
          showToast(t(locale, 'fileBrowser.pasted').replace('{count}', String(clipBoard.paths.length)));
        } else {
          const result = await fs.copyEntries(clipBoard.paths, currentPath);
          const count = result?.count ?? clipBoard.paths.length;
          showToast(t(locale, 'fileBrowser.pasted').replace('{count}', String(count)));
        }
      } else {
        if (typeof fs.moveEntries !== 'function') {
          for (const p of clipBoard.paths) {
            await fs.moveEntry?.(p, currentPath);
          }
          showToast(t(locale, 'fileBrowser.batchMoved').replace('{count}', String(clipBoard.paths.length)));
        } else {
          const result = await fs.moveEntries(clipBoard.paths, currentPath);
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

  const handleCopyPath = useCallback(
    async (entry: FileEntry) => {
      try {
        await navigator.clipboard.writeText(entry.path);
        showToast(t(locale, 'fileBrowser.pathCopied'));
      } catch {
        showToast(t(locale, 'fileBrowser.copyPath'));
      }
    },
    [showToast, locale],
  );

  const handleCopyImage = useCallback(
    async (entry: FileEntry) => {
      try {
        const fs = fsApi();
        if (typeof fs.clipboardCopyImage !== 'function') {
          showToast(t(locale, 'fileBrowser.clipboardImageFailed'));
          return;
        }
        const r = await fs.clipboardCopyImage(entry.path);
        showToast(r?.ok ? t(locale, 'fileBrowser.clipboardImageCopied') : t(locale, 'fileBrowser.clipboardImageFailed'));
      } catch {
        showToast(t(locale, 'fileBrowser.clipboardImageFailed'));
      }
    },
    [showToast, locale],
  );

  const handleOpenWith = useCallback(
    async (entry: FileEntry, withApp: 'default' | 'reveal' | 'terminal' | 'editor' = 'default') => {
      if (!entry.isDir) markSelfOpened(entry.path);
      try {
        const fs = fsApi();
        if (typeof fs.openWith !== 'function') {
          showToast(t(locale, 'fileBrowser.openDefault') + ': ' + entry.path);
          return;
        }
        await fs.openWith(entry.path, withApp);
      } catch {
        showToast(t(locale, 'fileBrowser.openDefault') + ': ' + entry.path);
      }
    },
    [markSelfOpened, showToast, locale],
  );

  const handleBatchTrash = useCallback(async () => {
    const paths = resolveTargetPathsCb(null);
    if (paths.length === 0) return;
    if (paths.length === 1) {
      const entry = entries.find((e) => e.path === paths[0]) || filteredEntries.find((e) => e.path === paths[0]);
      if (entry) {
        setTrashTarget(entry);
        return;
      }
    }
    // Multi: confirm via first name style message then trash
    try {
      const fs = fsApi();
      if (typeof fs.trashEntries === 'function') {
        const r = await fs.trashEntries(paths);
        showToast(t(locale, 'fileBrowser.batchTrashed').replace('{count}', String(r?.count ?? paths.length)));
      } else {
        for (const p of paths) await fs.trashEntry(p);
        showToast(t(locale, 'fileBrowser.batchTrashed').replace('{count}', String(paths.length)));
      }
      paths.forEach((p) => {
        void removeRecentFile(p);
      });
      setSelectedPaths(new Set());
      await loadEntries();
    } catch {
      showToast(t(locale, 'fileBrowser.trashFailed'));
    }
  }, [resolveTargetPathsCb, entries, filteredEntries, loadEntries, showToast, locale, setSelectedPaths]);

  const handleInternalMove = useCallback(
    async (sourcePaths: string[], destDir: string) => {
      if (!sourcePaths.length || !destDir) return;
      // Prevent moving a folder into itself
      const safe = sourcePaths.filter((p) => p !== destDir && !destDir.startsWith(p + '/'));
      if (!safe.length) return;
      try {
        const fs = fsApi();
        if (typeof fs.moveEntries === 'function') {
          const r = await fs.moveEntries(safe, destDir);
          showToast(t(locale, 'fileBrowser.batchMoved').replace('{count}', String(r?.count ?? safe.length)));
        } else {
          for (const pth of safe) await fs.moveEntry?.(pth, destDir);
          showToast(t(locale, 'fileBrowser.batchMoved').replace('{count}', String(safe.length)));
        }
        setSelectedPaths(new Set());
        await loadEntries();
      } catch {
        showToast(t(locale, 'fileBrowser.moveFailed'));
      }
    },
    [loadEntries, showToast, locale, setSelectedPaths],
  );

  // Shell operations (all through the fs.openWith chain; no legacy shell bypass)
  const handleRevealInFinder = useCallback(
    (entry: FileEntry) => {
      void handleOpenWith(entry, 'reveal');
    },
    [handleOpenWith],
  );

  const handleOpenInEditor = useCallback(
    (entry: FileEntry) => {
      void handleOpenWith(entry, 'editor');
    },
    [handleOpenWith],
  );

  const handleOpenInTerminal = useCallback(
    async (dir: string) => {
      const api = window.nativesAPI;
      if (api?.terminal?.create && api?.terminal?.write) {
        try {
          // 打开终端面板 → 新建 PTY 会话 → cd 进目标目录
          // 用幂等的 open-terminal（仅在折叠时展开），避免终端已打开时被 toggle 关闭
          dispatchFileEvent(FILE_EVENTS.openTerminal);
          const result = (await api.terminal.create()) as { sessionId?: string; error?: string };
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
    },
    [showToast, locale],
  );

  const handleDiskUsage = useCallback((dir: string) => {
    setDiskUsageTarget(dir);
  }, []);

  const doTrash = useCallback(async () => {
    if (!trashTarget) return;
    const trashedPath = trashTarget.path;
    try {
      const result = await fsApi().trashEntry(trashedPath);
      if (result?.ok) {
        showToast(t(locale, 'fileBrowser.trashed'));
        void removeRecentFile(trashedPath);
        dispatchFileEvent(FILE_EVENTS.fileTrashed, { path: trashedPath });
        await loadEntries();
      } else {
        showToast(result?.error || t(locale, 'fileBrowser.trashFailed'));
      }
    } catch {
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
      const result = await fsApi().createEntry(targetPath, newItemTarget.type);
      if (result?.ok) {
        showToast(t(locale, 'fileBrowser.created'));
        await loadEntries();
      } else {
        showToast(result?.error || t(locale, 'fileBrowser.createFailed'));
      }
    } catch {
      showToast(t(locale, 'fileBrowser.createFailed'));
    }
    setNewItemTarget(null);
    setNewItemName('');
  }, [newItemTarget, newItemName, loadEntries, showToast, locale]);

  return {
    clipBoard,
    renameTarget,
    renameValue,
    setRenameTarget,
    setRenameValue,
    newItemTarget,
    newItemName,
    setNewItemTarget,
    setNewItemName,
    trashTarget,
    setTrashTarget,
    diskUsageTarget,
    setDiskUsageTarget,
    resolveTargetPaths: resolveTargetPathsCb,
    handleRename,
    handleRenameConfirm,
    handleTrash,
    doTrash,
    handleDuplicate,
    handleExtract,
    handleCompress,
    handleCopyEntry,
    handleCutEntry,
    handlePaste,
    handleCopyPath,
    handleCopyImage,
    handleOpenWith,
    handleBatchTrash,
    handleInternalMove,
    handleRevealInFinder,
    handleOpenInEditor,
    handleOpenInTerminal,
    handleDiskUsage,
    handleNewFile,
    handleNewFolder,
    handleNewItemConfirm,
  };
}
