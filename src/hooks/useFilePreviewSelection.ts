'use client';

/**
 * useFilePreviewSelection — FileBrowser 打开/预览选择状态。
 *
 * 职责边界（F3-04 拆分，ARCH-002）：
 * - 打开条目：目录 → 导航；文件 → 登记 selfOpened + 记最近打开 + onFileSelect
 *   （preview 单轨已由 P2-03 收敛：onFileSelect → FilePreview → usePreview）
 * - handlePreview / handleEditRequest：右键「预览」/ 图片编辑入口
 *
 * 本 hook 只把「用户选中哪个文件去预览」的决策收口；实际预览渲染完全委托
 * P2-03 的 usePreview/PreviewSurface（不在此重复算法）。
 */

import { useCallback } from 'react';
import { type FileEntry } from '@/types/file';
import { pushRecentFile } from '@/lib/recent-files-client';

export interface UseFilePreviewSelectionOptions {
  onFileSelect?: (entry: FileEntry) => void;
  navigateTo: (path: string) => void;
  /** 打开动作登记 selfOpened，3s 窗口内 fs_watch 忽略其假变更 */
  markSelfOpened: (path: string) => void;
}

export interface UseFilePreviewSelectionResult {
  /** 打开条目：目录 → 导航；文件 → 登记 selfOpened + 记最近打开 + 触发 preview */
  handleOpenEntry: (entry: FileEntry) => void;
  handlePreview: (entry: FileEntry) => void;
  handleEditRequest: (entry: FileEntry) => void;
}

export function useFilePreviewSelection({
  onFileSelect,
  navigateTo,
  markSelfOpened,
}: UseFilePreviewSelectionOptions): UseFilePreviewSelectionResult {
  const handleOpenEntry = useCallback(
    (entry: FileEntry) => {
      if (entry.isDir) {
        navigateTo(entry.path);
      } else {
        // 登记 selfOpened：打开动作本身触发的假变更（LaunchServices xattr）3s 内忽略
        markSelfOpened(entry.path);
        pushRecentFile(entry.path);
        onFileSelect?.(entry);
      }
    },
    [navigateTo, markSelfOpened, onFileSelect],
  );

  const handlePreview = useCallback(
    (entry: FileEntry) => {
      if (!entry.isDir) markSelfOpened(entry.path);
      onFileSelect?.(entry);
    },
    [markSelfOpened, onFileSelect],
  );

  const handleEditRequest = useCallback(
    (entry: FileEntry) => {
      if (!entry.isDir) markSelfOpened(entry.path);
      onFileSelect?.(entry);
    },
    [markSelfOpened, onFileSelect],
  );

  return { handleOpenEntry, handlePreview, handleEditRequest };
}
