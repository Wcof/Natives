'use client';

/**
 * FilePreview 代码编辑 pane（Monaco 写路径）。
 * 从 FilePreview.tsx 抽出（FIL-004：format views / lifecycle 分责）。
 *
 * - 停笔 800ms 防抖自动保存，⌘S 立即 flush（保存串行化见 useEditorSave）
 * - 卸载/切文件时 flush 未保存内容（guardDirty，不丢字）
 * - 外部变更且本地未脏 → 静默重读磁盘并重建编辑器（key remount）
 * - mtime 冲突 → 弹窗「覆盖 / 暂不」，绝不静默覆盖外部修改
 */

import { Suspense, lazy, useCallback, useEffect, useRef, useState } from 'react';
import { t, type Locale } from '@/i18n';
import { type FileEntry } from '@/types/file';
import { useFileContent } from '@/lib/useFileContent';
import { useEditorSave } from '@/lib/use-editor-save';
import { MathCurveLoader } from '@/components/ui/MathCurveLoader';
import { formatSavedStatusLabel, startSavedStatusTicker } from '../saved-status-ticker';
import { SaveConflictDialog } from './meta';

// Lazy-loaded heavy component
const MonacoEditor = lazy(() => import('../MonacoEditor'));

/** 只读代码 → Monaco 编辑写路径的适配 pane（读文件 + 转发 CodeEditorPane） */
export function CodeEditPane({ entry, locale, ext }: {
  entry: FileEntry;
  locale: Locale;
  ext: string;
}) {
  const { content: code, loading, mtime, reload } = useFileContent(entry.path);

  if (loading || code === null) {
    return (
      <div style={{ color: 'var(--text-disabled)', fontSize: 12, padding: 20, textAlign: 'center' }}>
        {t(locale, 'common.loading')}
      </div>
    );
  }

  return <CodeEditorPane entry={entry} code={code} mtime={mtime} reload={reload} locale={locale} ext={ext} />;
}

/** Monaco 编辑器写路径（保存 / 外部重读 / 冲突 / 已保存状态条） */
function CodeEditorPane({ entry, code, mtime, reload, locale, ext }: {
  entry: FileEntry;
  code: string;
  mtime: number | null;
  reload: () => void;
  locale: Locale;
  ext: string;
}) {
  const dirtyRef = useRef(false);
  const latestRef = useRef(code);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [reloadTick, setReloadTick] = useState(0);
  const [savedTickLabel, setSavedTickLabel] = useState<string | null>(null);
  const monacoRef = useRef<import('monaco-editor').editor.IStandaloneCodeEditor | null>(null);

  const { save, hasConflict, overwrite, dismissConflict, savedAt, saveError } = useEditorSave({
    path: entry.path,
    initialMtime: mtime,
    isDirty: useCallback(() => dirtyRef.current, []),
    onExternalChange: useCallback(() => {
      reload();
      setReloadTick((n) => n + 1); // Monaco defaultValue 只在挂载时生效，remount 换内容
    }, [reload]),
  });

  // 外部重读后同步 latestRef（未脏时才会走到这里）
  useEffect(() => {
    if (!dirtyRef.current) latestRef.current = code;
  }, [code]);

  const doSave = useCallback((value: string) => {
    dirtyRef.current = false;
    void save(value);
  }, [save]);

  const handleChange = useCallback((value: string) => {
    latestRef.current = value;
    dirtyRef.current = true;
    if (debounceRef.current) clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(() => {
      debounceRef.current = null;
      doSave(latestRef.current);
    }, 800);
  }, [doSave]);

  const handleManualSave = useCallback((value: string) => {
    if (debounceRef.current) { clearTimeout(debounceRef.current); debounceRef.current = null; }
    latestRef.current = value;
    doSave(value);
  }, [doSave]);

  // guardDirty：卸载/切文件时 flush 未保存内容。save 随 path 换代，
  // cleanup 捕获的是旧文件的保存函数，flush 落在正确的文件上。
  useEffect(() => {
    return () => {
      if (debounceRef.current) { clearTimeout(debounceRef.current); debounceRef.current = null; }
      if (dirtyRef.current) {
        dirtyRef.current = false;
        void save(latestRef.current);
      }
    };
  }, [save]);

  // 「N 秒前已保存」状态条仅在文档可见时刷新。
  useEffect(() => {
    if (savedAt === null) { setSavedTickLabel(null); return; }
    const update = () => {
      setSavedTickLabel(formatSavedStatusLabel(savedAt, Date.now(), {
        justNow: t(locale, 'filePreview.savedJustNow'),
        secondsAgo: t(locale, 'filePreview.savedSecondsAgo'),
      }));
    };
    return startSavedStatusTicker(savedAt, update);
  }, [savedAt, locale]);

  return (
    <div style={{ display: 'flex', flexDirection: 'column', flex: 1, minHeight: 0 }}>
      <div style={{ flex: 1, minHeight: 0 }}>
        <Suspense fallback={
          <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', padding: 40, gap: 12 }}>
            <MathCurveLoader size={40} />
            <div style={{ color: 'var(--text-disabled)', fontSize: 12 }}>Loading editor...</div>
          </div>
        }>
          <MonacoEditor
            key={`${entry.path}:${reloadTick}`}
            content={code}
            language={ext}
            onChange={handleChange}
            onSave={handleManualSave}
            onEditorReady={(editor) => {
              monacoRef.current = editor;
            }}
          />
        </Suspense>
      </div>
      {(savedTickLabel || saveError) && (
        <div style={{
          padding: '3px 10px', fontSize: 11, fontFamily: 'var(--font-mono)',
          color: saveError ? 'var(--danger)' : 'var(--text-disabled)',
          borderTop: '1px solid var(--border-subtle)',
        }}>
          {saveError ? t(locale, 'filePreview.saveFailed') : savedTickLabel}
        </div>
      )}
      <SaveConflictDialog
        open={hasConflict}
        fileName={entry.name}
        locale={locale}
        onOverwrite={overwrite}
        onDismiss={dismissConflict}
      />
    </div>
  );
}
