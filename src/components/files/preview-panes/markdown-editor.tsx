'use client';

/**
 * FilePreview Markdown WYSIWYG 编辑 pane（Milkdown 写路径）。
 * 从 FilePreview.tsx 抽出（FIL-004：format views / lifecycle 分责）。
 *
 * - 外部变更且本地未脏 → 静默重读并重建 Crepe
 * - mtime 冲突 → SaveConflictDialog「覆盖 / 暂不」
 * - 本地图片改写：`![](./图/x.png)` → convertFileSrc URL，落盘经 restore 还原
 * - Cmd/Ctrl+F 在编辑区打开查找替换（ProseMirror transaction）
 */

import { Suspense, lazy, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { t, type Locale } from '@/i18n';
import { useFileContent } from '@/lib/useFileContent';
import { useEditorSave } from '@/lib/use-editor-save';
import { fsApi, hasNativeFiles } from '@/lib/files-api';
import { rewriteLocalImages, type LocalImageRewrite } from '@/lib/markdown-local-images';
import { MathCurveLoader } from '@/components/ui/MathCurveLoader';
import FindReplaceBar from '../FindReplaceBar';
import { SaveConflictDialog } from './meta';

// Lazy-loaded heavy component
const MilkdownEditor = lazy(() => import('../MilkdownEditor'));
import type { MilkdownFindReplace } from '../MilkdownEditor';

/** Markdown WYSIWYG 编辑 pane（editMode + isMarkdown） */
export function MdWysiwygEditor({ path, locale }: { path: string; locale: Locale }) {
  const { content, mtime, reload } = useFileContent(path);
  const dirtyRef = useRef(false);
  const { save, hasConflict, overwrite, dismissConflict } = useEditorSave({
    path,
    initialMtime: mtime,
    isDirty: useCallback(() => dirtyRef.current, []),
    // 外部（agent）改了文件且本地未脏 → 静默重读；content 变化会重建 Crepe
    onExternalChange: reload,
  });

  // 审计收口 #12：Milkdown 编辑路径的查找替换——ProseMirror transaction 改内存
  // 模型并走 dirty→autosave→expectedMtime 冲突链（MilkdownEditor 内 queueSave）。
  const [findOpen, setFindOpen] = useState(false);
  const [findQuery, setFindQuery] = useState('');
  const [matchCase, setMatchCase] = useState(false);
  const [findIndex, setFindIndex] = useState(-1);
  const [findCount, setFindCount] = useState(0);
  const [replacement, setReplacement] = useState('');
  const milkdownFindRef = useRef<MilkdownFindReplace | null>(null);
  const editorHostRef = useRef<HTMLDivElement | null>(null);

  const runFind = useCallback(
    (value: string) => {
      const handle = milkdownFindRef.current;
      if (!handle) return;
      const result = handle.find(value, matchCase);
      setFindIndex(result.index);
      setFindCount(result.count);
    },
    [matchCase],
  );
  const handleFindReplaceReady = useCallback((handle: MilkdownFindReplace | null) => {
    milkdownFindRef.current = handle;
  }, []);

  // Cmd/Ctrl+F 在 Milkdown 编辑区打开查找（FilePreview 键盘段已排除 .milkdown-host）。
  useEffect(() => {
    const el = editorHostRef.current;
    if (!el) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'f') {
        e.preventDefault();
        e.stopPropagation();
        setFindOpen(true);
      }
    };
    el.addEventListener('keydown', onKeyDown, true);
    return () => el.removeEventListener('keydown', onKeyDown, true);
  }, []);

  // 本地图片改写：`![](./图/x.png)` → convertFileSrc URL（渲染可见），
  // 落盘经 restore 精确还原原文；用户新拖入的资产 URL 还原为真实路径
  const baseDir = path.substring(0, path.lastIndexOf('/')) || '/';
  const rewriteRef = useRef<LocalImageRewrite | null>(null);
  const displayContent = useMemo(() => {
    if (content === null) return null;
    const convert = hasNativeFiles() ? fsApi().convertFileSrc : undefined;
    if (!convert) {
      rewriteRef.current = null;
      return content;
    }
    const rewrite = rewriteLocalImages(content, baseDir, (abs) => convert(abs) ?? abs);
    rewriteRef.current = rewrite;
    return rewrite.text;
  }, [content, baseDir]);

  if (displayContent === null) {
    return <div style={{ padding: 20, textAlign: 'center', color: 'var(--text-disabled)', fontSize: 12 }}>{t(locale, 'filePreview.failedLoad')}</div>;
  }

  return (
    <>
      {findOpen && (
        <FindReplaceBar
          locale={locale}
          query={findQuery}
          onQueryChange={(value) => {
            setFindQuery(value);
            runFind(value);
          }}
          matchCase={matchCase}
          onToggleMatchCase={() => {
            const next = !matchCase;
            setMatchCase(next);
            runFind(findQuery);
          }}
          index={findIndex}
          count={findCount}
          onNavigate={(direction) => {
            const handle = milkdownFindRef.current;
            if (!handle) return;
            const count = handle.find(findQuery, matchCase).count;
            if (count <= 0) return;
            const next =
              direction === 'next'
                ? (findIndex < 0 ? 0 : (findIndex + 1) % count)
                : (findIndex < 0 ? count - 1 : (findIndex - 1 + count) % count);
            setFindIndex(next);
          }}
          onClose={() => setFindOpen(false)}
          canReplace
          replacement={replacement}
          onReplacementChange={setReplacement}
          onReplaceOne={() => {
            const ok = milkdownFindRef.current?.replaceOne(replacement) ?? false;
            if (ok) runFind(findQuery);
          }}
          onReplaceAll={() => {
            const n = milkdownFindRef.current?.replaceAll(replacement) ?? 0;
            if (n > 0) runFind(findQuery);
          }}
        />
      )}
      <Suspense fallback={
        <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', padding: 40, gap: 12 }}>
          <MathCurveLoader size={40} />
          <div style={{ color: 'var(--text-disabled)', fontSize: 12 }}>Loading editor...</div>
        </div>
      }>
        <div ref={editorHostRef} style={{ flex: 1, minHeight: 0 }}>
          <MilkdownEditor
            content={displayContent}
            filePath={path}
            locale={locale}
            onSave={(newContent) => {
              const restored = rewriteRef.current ? rewriteRef.current.restore(newContent) : newContent;
              void save(restored);
            }}
            onDirtyChange={(dirty) => { dirtyRef.current = dirty; }}
            onFindReplaceReady={handleFindReplaceReady}
          />
        </div>
      </Suspense>
      <SaveConflictDialog
        open={hasConflict}
        fileName={path.split('/').pop() || path}
        locale={locale}
        onOverwrite={overwrite}
        onDismiss={dismissConflict}
      />
    </>
  );
}
