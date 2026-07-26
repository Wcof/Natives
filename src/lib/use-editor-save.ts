'use client';

/**
 * useEditorSave — 编辑器共享保存管线（fanbox 编辑体验移植）
 *
 * - 串行化：所有保存进同一条 Promise 链，防抖到点的保存与
 *   卸载时的 flush 不互踩（fanbox `chain`）
 * - 乐观锁：写入携带 expectedMtime，后端返回 `conflict: true` 时
 *   不落盘，暴露冲突态由 UI 弹窗决定「覆盖 / 暂不」——外部（尤其
 *   agent）的修改不再被静默覆盖
 * - 外部变更热重载：监听 `fs-watch-change`，本地未脏且非自己刚写入
 *   （1.5s 窗口）时通知调用方重读磁盘；脏则不动，靠保存时冲突兜底
 */

import { useCallback, useEffect, useRef, useState } from 'react';
import { fsApi, fsWatchApiOrNull } from '@/lib/files-api';

interface UseEditorSaveOptions {
  path: string;
  /** 磁盘读取时的 mtime（ms），保存后自动跟进为写入返回的新 mtime */
  initialMtime: number | null;
  /** 本地是否有未落盘编辑；热重载前询问，脏则跳过 */
  isDirty?: () => boolean;
  /** 外部变更且本地未脏 → 调用方应重读磁盘 */
  onExternalChange?: () => void;
}

export function useEditorSave({ path, initialMtime, isDirty, onExternalChange }: UseEditorSaveOptions) {
  const mtimeRef = useRef<number | null>(initialMtime);
  useEffect(() => {
    mtimeRef.current = initialMtime;
  }, [initialMtime, path]);

  const chainRef = useRef<Promise<void>>(Promise.resolve());
  const lastSaveAtRef = useRef(0);
  /** 冲突时暂存未落盘内容，等待用户决定 */
  const [conflictContent, setConflictContent] = useState<string | null>(null);
  const [savedAt, setSavedAt] = useState<number | null>(null);
  const [saveError, setSaveError] = useState(false);

  const doWrite = useCallback(async (content: string, force: boolean) => {
    // force（用户确认覆盖）时不传 expectedMtime，跳过后端冲突检测
    const expected = force ? undefined : mtimeRef.current ?? undefined;
    const result = (await fsApi().writeFileAtomic(path, content, expected)) as
      | { mtime?: number; conflict?: boolean }
      | undefined;
    if (result?.conflict) {
      setConflictContent(content);
      return;
    }
    if (typeof result?.mtime === 'number') mtimeRef.current = result.mtime;
    lastSaveAtRef.current = Date.now();
    setSavedAt(Date.now());
    setSaveError(false);
    setConflictContent(null);
  }, [path]);

  const save = useCallback((content: string, opts?: { force?: boolean }) => {
    const run = chainRef.current
      .then(() => doWrite(content, !!opts?.force))
      .catch(() => setSaveError(true));
    chainRef.current = run;
    return run;
  }, [doWrite]);

  /** 用户确认覆盖磁盘上的外部版本 */
  const overwrite = useCallback(() => {
    if (conflictContent === null) return;
    void save(conflictContent, { force: true });
  }, [conflictContent, save]);

  /** 暂不保存：关闭弹窗，编辑内容留在编辑器里（fanbox 语义） */
  const dismissConflict = useCallback(() => setConflictContent(null), []);

  // 外部变更热重载（数据源：FileBrowser 对当前目录的 fs_watch）
  useEffect(() => {
    const api = fsWatchApiOrNull();
    if (!api || !onExternalChange) return;
    const off = api.onChange((event: { path: string; kind: string }) => {
      if (event.path !== path || event.kind === 'remove') return;
      if (Date.now() - lastSaveAtRef.current < 1500) return; // 自己刚写入触发的事件
      if (isDirty?.()) return;
      onExternalChange();
    });
    return off;
  }, [path, onExternalChange, isDirty]);

  return { save, hasConflict: conflictContent !== null, overwrite, dismissConflict, savedAt, saveError };
}
