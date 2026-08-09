'use client';

/**
 * useFileDrop — FileBrowser 拖拽落盘编排（finder 文件 / 浏览器图片 URL）。
 *
 * 职责边界（F3-04 拆分，ARCH-002）：
 * - 系统文件（Finder）→ importFiles 导入当前目录 + 刷新 + toast
 * - text/uri-list（WeChat/浏览器图片）→ 保存后刷新 + toast
 *
 * 底层拖拽机制（dragenter/leave/over/drop、isDragging、URL 保存）复用
 * `src/lib/use-file-drop.ts`（唯一实现，不造第二套）；本 hook 只叠加
 * FileBrowser 的导入编排（nativesAPI.fs.importFiles + loadEntries + toast）。
 */

import { useCallback } from 'react';
import { t, type Locale } from '@/i18n';
import { useFileDrop as useFileDragMechanics } from '@/lib/use-file-drop';

export interface UseFileDropOptions {
  currentPath: string;
  loadEntries: () => Promise<void>;
  showToast: (msg: string) => void;
  locale: Locale;
}

export interface UseFileDropResult {
  isDragging: boolean;
  dragHandlers: {
    onDragEnter: (e: React.DragEvent) => void;
    onDragLeave: (e: React.DragEvent) => void;
    onDragOver: (e: React.DragEvent) => void;
    onDrop: (e: React.DragEvent) => void;
  };
}

export function useFileDrop({
  currentPath,
  loadEntries,
  showToast,
  locale,
}: UseFileDropOptions): UseFileDropResult {
  const { isDragging, dragHandlers } = useFileDragMechanics({
    currentDir: currentPath,
    onFilesDropped: useCallback(
      async (paths: string[]) => {
        try {
          const api = window.nativesAPI;
          if (api?.fs?.importFiles) {
            await api.fs.importFiles(paths, currentPath);
            await loadEntries();
            showToast(t(locale, 'fileBrowser.filesDropped'));
          } else {
            showToast(t(locale, 'fileBrowser.importApiUnavailable'));
          }
        } catch {
          showToast(t(locale, 'fileBrowser.importFailed'));
        }
      },
      [currentPath, locale, loadEntries, showToast],
    ),
    onUrlDropped: useCallback(
      async (_savedPath: string) => {
        await loadEntries();
        showToast(t(locale, 'fileBrowser.imageSaved'));
      },
      [loadEntries, locale, showToast],
    ),
  });

  return { isDragging, dragHandlers };
}
