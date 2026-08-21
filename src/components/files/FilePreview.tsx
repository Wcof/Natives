'use client';

import { useState, useEffect, useMemo, useRef } from 'react';
import { type FileEntry } from '@/types/file';
import { t, useLocale } from '@/i18n';
import { getExt, isMarkdownFile, shouldPreviewAsCode } from '@/lib/follow-mode';
import { type PreviewSource, type PreviewSubMode } from '@/lib/preview/contracts';
import FindReplaceBar from './FindReplaceBar';
import { useFindReplace } from '@/hooks/useFindReplace';
import ImageLightbox from './ImageLightbox';
import PreviewSurface from '@/components/preview/PreviewSurface';
import { createBuiltinRegistry, createDefaultContext } from '@/lib/preview/composition';
import { PreviewService } from '@/lib/preview/service';
import { CodeEditPane } from './preview-panes/code-editor';
import { ImageEditPane } from './preview-panes/image-editor';
import { MdWysiwygEditor } from './preview-panes/markdown-editor';
import { FileInfo, GitDiffView } from './preview-panes/meta';

// 兼容既有授权测试的 re-export（FIL-004 拆分后仍从组件模块可导入）。
export { authorizeImageEditAsset } from '@/lib/preview/image-edit';

interface FilePreviewProps {
  entry: FileEntry;
  subMode: PreviewSubMode;
  onClose: () => void;
  editMode?: boolean;
}

/**
 * Preview Capability 唯一只读预览 pipeline（T19/T201）。
 *
 * 只读预览（!editMode）一律走统一 PreviewSurface（builtin registry / PreviewService
 * / usePreview）。旧的 localStorage rollback 开关与 legacy 预览分支已物理删除
 * （MIG-003），不再有运行时回退路径。
 *
 * Editor 写路径（editMode）独立于只读预览：code → Monaco、markdown → Milkdown、
 * image → ImageEditor；video/audio/pdf/csv/archive/html 等无编辑能力类型仍经
 * PreviewSurface 只读呈现。Feature 只持有选择/布局状态，不复制预览算法（R-E3）。
 *
 * 内容 pane 已按职责拆至 `preview-panes/`（FIL-004）：
 * - code-editor：Monaco 写路径（CodeEditPane / CodeEditorPane）
 * - image-editor：ImageEditor 写路径 + 只读图片预览
 * - markdown-editor：Milkdown WYSIWYG 写路径
 * - meta：保存冲突弹窗 / 文件信息 / Git Diff 视图
 */
export default function FilePreview({ entry, subMode, onClose, editMode = false }: FilePreviewProps) {
  const [gitDiff, setGitDiff] = useState<string | null>(null);
  const [gitLoading, setGitLoading] = useState(false);
  const [gitStatus, setGitStatus] = useState<string | null>(null);
  const locale = useLocale();
  const [lightboxSrc, setLightboxSrc] = useState<string | null>(null);
  const containerRef = useRef<HTMLDivElement>(null);

  // 问题12：面板聚焦时 Cmd/Ctrl+F 打开查找；Escape 先关 find bar 再关预览。
  const findReplace = useFindReplace(containerRef);

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement | null;
      // Monaco/Milkdown 编辑区拥有自己的 Cmd/Ctrl+F（Monaco 内建查找替换、
      // Milkdown ProseMirror 查找），不抢它们的快捷键。
      if (target?.closest?.('.monaco-editor, .milkdown-host')) return;
      if (e.key === 'Escape') {
        if (findReplace.open) {
          findReplace.close();
          e.preventDefault();
          e.stopPropagation();
          return;
        }
        onClose();
      } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'f') {
        // 只读 Preview 统一查找入口；不抢全局快捷键（事件仅来自本面板容器）。
        e.preventDefault();
        e.stopPropagation();
        findReplace.openFind();
      }
    };
    const el = containerRef.current;
    el?.addEventListener('keydown', handleKeyDown, true);
    return () => el?.removeEventListener('keydown', handleKeyDown, true);
  }, [onClose, findReplace]);

  // Load git diff when git sub-mode is active
  useEffect(() => {
    if (subMode !== 'git') return;
    let cancelled = false;

    async function loadGit() {
      setGitLoading(true);
      try {
        const api = window.nativesAPI;
        if (api?.git?.diff) {
          const diff = await api.git.diff(entry.path);
          if (!cancelled) {
            setGitDiff((diff as string) || t(locale, 'fileBrowser.noChanges'));
          }
        }
        if (api?.git?.status) {
          const dirPath = entry.isDir ? entry.path : entry.path.substring(0, entry.path.lastIndexOf('/')) || '/';
          const status = await api.git.status(dirPath) as unknown as { files: Array<{ path: string; status: string }> };
          if (!cancelled && status) {
            const fileStatus = (status.files || []).find(
              (f) => f.path === entry.path || f.path === entry.name
            );
            if (fileStatus) {
              const statusMap: Record<string, string> = {
                'M': t(locale, 'filePreview.gitModified'),
                'A': t(locale, 'filePreview.gitAdded'),
                'D': t(locale, 'filePreview.gitDeleted'),
                'R': t(locale, 'filePreview.gitRenamed'),
                '??': t(locale, 'filePreview.gitUntracked'),
                'UU': t(locale, 'filePreview.gitConflict'),
              };
              setGitStatus(statusMap[fileStatus.status] || fileStatus.status);
            } else {
              setGitStatus(t(locale, 'filePreview.gitUnchanged'));
            }
          }
        }
      } catch {
        if (!cancelled) setGitDiff(t(locale, 'fileBrowser.notInRepo'));
      } finally {
        if (!cancelled) setGitLoading(false);
      }
    }
    loadGit();
    return () => { cancelled = true; };
  }, [subMode, entry.path, entry.name, entry.isDir, locale]);

  const ext = getExt(entry.name);
  const isMarkdown = isMarkdownFile(entry.name);
  const isCode = shouldPreviewAsCode(entry.kind, entry.name, editMode);

  // 统一只读预览管线（surface-local controller 由 usePreview 持有）
  const previewService = useMemo(
    () => new PreviewService(createBuiltinRegistry(), createDefaultContext()),
    [],
  );
  const previewSource = useMemo<PreviewSource>(
    () => ({
      type: 'file',
      path: entry.path,
      name: entry.name,
      kind: entry.kind,
      size: entry.size,
      mtime: entry.mtime,
    }),
    [entry.path, entry.name, entry.kind, entry.size, entry.mtime],
  );

  return (
    <div ref={containerRef} tabIndex={-1} role="dialog" aria-label={entry.name} style={{
      display: 'flex',
      flexDirection: 'column',
      flex: 1,
      minHeight: 0,
      background: 'transparent',
    }}>
      {findReplace.open && (
        <FindReplaceBar
          locale={locale}
          query={findReplace.query}
          onQueryChange={findReplace.setQuery}
          matchCase={findReplace.matchCase}
          onToggleMatchCase={() => findReplace.setMatchCase((v) => !v)}
          index={findReplace.index}
          count={findReplace.count}
          onNavigate={findReplace.navigate}
          onClose={findReplace.close}
        />
      )}
      {/* Content */}
      <div style={{
        flex: 1,
        minHeight: 0,
        overflow: 'auto',
        padding: isMarkdown || (subMode === 'preview' && isCode && editMode) ? 0 : '0 4px',
        display: 'flex',
        flexDirection: 'column',
      }}>
        {subMode === 'preview' && (
          editMode ? (
            // ── Editor 写路径（独立于只读预览）──
            isCode ? (
              <CodeEditPane entry={entry} locale={locale} ext={ext} />
            ) : isMarkdown ? (
              <MdWysiwygEditor path={entry.path} locale={locale} />
            ) : entry.kind === 'image' ? (
              <ImageEditPane entry={entry} locale={locale} onImageClick={setLightboxSrc} />
            ) : (
              // 无编辑能力的类型（video/audio/pdf/csv/archive/html/…）仍只读呈现
              <PreviewSurface source={previewSource} surface="files" service={previewService} />
            )
          ) : (
            <PreviewSurface source={previewSource} surface="files" service={previewService} />
          )
        )}
        {subMode === 'git' && (
          <GitDiffView diff={gitDiff} loading={gitLoading} status={gitStatus} fileName={entry.name} locale={locale} />
        )}
        {subMode === 'info' && (
          <FileInfo entry={entry} locale={locale} />
        )}
      </div>

      {/* Lightbox */}
      {lightboxSrc && (
        <ImageLightbox src={lightboxSrc} alt={entry.name} onClose={() => setLightboxSrc(null)} />
      )}
    </div>
  );
}
