'use client';

import { startTransition, useState, useEffect, useCallback, useMemo, lazy, Suspense, useRef } from 'react';
import { Pencil } from 'lucide-react';
import { MathCurveLoader } from '@/components/ui/MathCurveLoader';
import { type FileEntry } from '@/types/file';
import { t, useLocale, type Locale } from '@/i18n';
import { getExt, isMarkdownFile, shouldPreviewAsCode } from '@/lib/follow-mode';
import { detectLanguage } from '@/lib/shiki-utils';
import { parseUnifiedDiff } from '@/lib/diff-utils';
import { useFileContent } from '@/lib/useFileContent';
import { useEditorSave } from '@/lib/use-editor-save';
import { fsApi, hasNativeFiles } from '@/lib/files-api';
import { rewriteLocalImages, type LocalImageRewrite } from '@/lib/markdown-local-images';
import ConfirmDialog from '@/components/ui/ConfirmDialog';
import { type PreviewSource, type PreviewSubMode } from '@/lib/preview/contracts';
import MonacoDiffView from '@/components/assistant/diff/MonacoDiffView';
import ImageLightbox from './ImageLightbox';
import PreviewSurface from '@/components/ui/preview/PreviewSurface';
import { createBuiltinRegistry, createDefaultContext } from '@/lib/preview/composition';
import { PreviewService } from '@/lib/preview/service';

// Lazy-loaded heavy components
const MilkdownEditor = lazy(() => import('./MilkdownEditor'));
const MonacoEditor = lazy(() => import('./MonacoEditor'));
const ImageEditor = lazy(() => import('./ImageEditor'));

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
 */

export default function FilePreview({ entry, subMode, onClose, editMode = false }: FilePreviewProps) {
  const [gitDiff, setGitDiff] = useState<string | null>(null);
  const [gitLoading, setGitLoading] = useState(false);
  const [gitStatus, setGitStatus] = useState<string | null>(null);
  const locale = useLocale();
  const [lightboxSrc, setLightboxSrc] = useState<string | null>(null);
  const containerRef = useRef<HTMLDivElement>(null);

  // Escape to close
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    const el = containerRef.current;
    el?.addEventListener('keydown', handleKeyDown);
    return () => el?.removeEventListener('keydown', handleKeyDown);
  }, [onClose]);

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

// ── Code Edit Pane（Monaco 写路径；只读代码预览统一走 PreviewSurface）──

function CodeEditPane({ entry, locale, ext }: {
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

// ── Image Edit Pane（写路径：只读预览由 PreviewSurface 呈现，此处仅编辑入口）──

/** 为图像编辑构建可被 canvas 读取的 URL（HEIC/TIFF 走后端 sips 转码，失败回退 convertFileSrc）。 */
function useImageEditUrl(path: string): string | null {
  const [url, setUrl] = useState<string | null>(null);

  useEffect(() => {
    if (!hasNativeFiles()) return;
    const fs = fsApi();

    // HEIC/TIFF：webview 不支持直接解码，走后端 sips 转码缓存（W8）。
    const lowerExt = path.split('.').pop()?.toLowerCase() || '';
    if (['heic', 'heif', 'tif', 'tiff'].includes(lowerExt) && fs.convertImagePreview) {
      let cancelled = false;
      (async () => {
        try {
          const converted = await fs.convertImagePreview(path);
          if (!cancelled && converted?.ok && converted.jpegPath) {
            setUrl(fs.convertFileSrc?.(converted.jpegPath) ?? '');
            return;
          }
        } catch { /* fall through */ }
        if (!cancelled) setUrl(fs.convertFileSrc?.(path) ?? '');
      })();
      return () => { cancelled = true; };
    }

    if (fs.convertFileSrc) {
      startTransition(() => { setUrl(fs.convertFileSrc?.(path) ?? ""); });
      return;
    }

    // 浏览器 dev 兜底：readFile → Blob URL（编辑需要可读字节，不能只用 asset URL）
    let cancelled = false;
    let createdUrl: string | null = null;
    (async () => {
      try {
        const result = (await fs.readFile(path)) as string | { content?: string; encoding?: string };
        if (cancelled) return;
        const content = typeof result === 'string' ? result : result?.content;
        if (!content) return;
        const ext = path.split('.').pop()?.toLowerCase() || '';
        const mimeMap: Record<string, string> = {
          png: 'image/png', jpg: 'image/jpeg', jpeg: 'image/jpeg',
          gif: 'image/gif', webp: 'image/webp', svg: 'image/svg+xml',
        };
        const mime = mimeMap[ext] || 'application/octet-stream';
        let blob: Blob;
        if (typeof result !== 'string' && result?.encoding === 'base64') {
          const byteString = atob(content);
          const ab = new ArrayBuffer(byteString.length);
          const ia = new Uint8Array(ab);
          for (let i = 0; i < byteString.length; i++) ia[i] = byteString.charCodeAt(i);
          blob = new Blob([ab], { type: mime });
        } else {
          blob = new Blob([content], { type: mime });
        }
        if (!cancelled) {
          createdUrl = URL.createObjectURL(blob);
          setUrl(createdUrl);
        }
      } catch { /* ignore */ }
    })();
    return () => {
      cancelled = true;
      if (createdUrl) URL.revokeObjectURL(createdUrl);
    };
  }, [path]);

  return url;
}

function ImageEditPane({ entry, locale, onImageClick }: {
  entry: FileEntry;
  locale: Locale;
  onImageClick: (src: string) => void;
}) {
  const imageUrl = useImageEditUrl(entry.path);
  const [imageEditing, setImageEditing] = useState(false);

  if (!imageUrl) return null;

  if (imageEditing) {
    return (
      <div style={{ display: 'flex', flexDirection: 'column', flex: 1, minHeight: 0 }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 8, padding: '4px 8px', borderBottom: '1px solid var(--border)' }}>
          <button
            onClick={() => setImageEditing(false)}
            className="text-xs px-2 py-1 rounded"
            style={{ background: 'var(--surface)', color: 'var(--text-secondary)' }}
          >
            {t(locale, 'filePreview.backToPreview')}
          </button>
          <span className="text-xs" style={{ color: 'var(--text-disabled)' }}>{entry.name}</span>
        </div>
        <Suspense fallback={<MathCurveLoader />}>
          <ImageEditor
            imagePath={imageUrl}
            imageName={entry.name}
            onSave={(dataUrl, ext, asNew) => {
              if (dataUrl && hasNativeFiles()) {
                const base64 = dataUrl.split(',')[1] || '';
                const p = entry.path || '';
                const dir = p.substring(0, p.lastIndexOf('/')) || '/';
                const entryName = entry.name || 'image';
                const name = asNew
                  ? entryName.replace(/\.[^.]+$/, '') + '-edited.' + ext
                  : entryName;
                fsApi().saveBlob(dir, name, base64).catch(() => {});
              }
              setImageEditing(false);
            }}
            onClose={() => setImageEditing(false)}
          />
        </Suspense>
      </div>
    );
  }

  return (
    <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', flex: 1, position: 'relative' }}>
      <img
        src={imageUrl}
        alt={entry.name}
        onClick={() => onImageClick(imageUrl)}
        style={{
          maxWidth: '100%', maxHeight: '100%', objectFit: 'contain',
          background: 'repeating-conic-gradient(color-mix(in srgb, var(--neutral-500) 20%, transparent) 0% 25%, transparent 0% 50%) 50% / 20px 20px',
          cursor: 'zoom-in',
        }}
      />
      <button
        onClick={(e) => { e.stopPropagation(); setImageEditing(true); }}
        title={t(locale, 'filePreview.editImage')}
        style={{
          position: 'absolute', top: 8, right: 8,
          display: 'flex', alignItems: 'center', gap: 4,
          padding: '4px 8px', borderRadius: 6, fontSize: 12,
          background: 'var(--surface)', color: 'var(--text-secondary)',
          border: '1px solid var(--border)', cursor: 'pointer',
        }}
      >
        <Pencil size={13} />
        {t(locale, 'filePreview.editImage')}
      </button>
    </div>
  );
}

// ── Markdown WYSIWYG Editor（写路径）──

function MdWysiwygEditor({ path, locale }: { path: string; locale: Locale }) {
  const { content, mtime, reload } = useFileContent(path);
  const dirtyRef = useRef(false);
  const { save, hasConflict, overwrite, dismissConflict } = useEditorSave({
    path,
    initialMtime: mtime,
    isDirty: useCallback(() => dirtyRef.current, []),
    // 外部（agent）改了文件且本地未脏 → 静默重读；content 变化会重建 Crepe
    onExternalChange: reload,
  });

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
      <Suspense fallback={
        <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', padding: 40, gap: 12 }}>
          <MathCurveLoader size={40} />
          <div style={{ color: 'var(--text-disabled)', fontSize: 12 }}>Loading editor...</div>
        </div>
      }>
        <MilkdownEditor
          content={displayContent}
          filePath={path}
          locale={locale}
          onSave={(newContent) => {
            const restored = rewriteRef.current ? rewriteRef.current.restore(newContent) : newContent;
            void save(restored);
          }}
          onDirtyChange={(dirty) => { dirtyRef.current = dirty; }}
        />
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

/** 保存冲突弹窗：磁盘版本比编辑基线新（外部/agent 修改），由用户决定覆盖或暂不保存 */
function SaveConflictDialog({ open, fileName, locale, onOverwrite, onDismiss }: {
  open: boolean;
  fileName: string;
  locale: Locale;
  onOverwrite: () => void;
  onDismiss: () => void;
}) {
  return (
    <ConfirmDialog
      open={open}
      title={t(locale, 'filePreview.conflictTitle')}
      message={t(locale, 'filePreview.conflictMessage').replace('{name}', fileName)}
      confirmLabel={t(locale, 'filePreview.conflictOverwrite')}
      cancelLabel={t(locale, 'filePreview.conflictKeep')}
      danger
      onConfirm={onOverwrite}
      onCancel={onDismiss}
    />
  );
}

// ── Code Editor Pane（Monaco + fanbox 编辑三件套）──
//
// - 停笔 800ms 防抖自动保存，⌘S 立即 flush（保存串行化见 useEditorSave）
// - 卸载/切文件时 flush 未保存内容（guardDirty，不丢字）
// - 外部变更且本地未脏 → 静默重读磁盘并重建编辑器（key remount）
// - mtime 冲突 → 弹窗「覆盖 / 暂不」，绝不静默覆盖外部修改

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

  // 「N 秒前已保存」状态条，每秒刷新
  useEffect(() => {
    if (savedAt === null) { setSavedTickLabel(null); return; }
    const update = () => {
      const secs = Math.max(0, Math.round((Date.now() - savedAt) / 1000));
      setSavedTickLabel(
        secs < 2
          ? t(locale, 'filePreview.savedJustNow')
          : t(locale, 'filePreview.savedSecondsAgo').replace('{seconds}', String(secs)),
      );
    };
    update();
    const timer = setInterval(update, 1000);
    return () => clearInterval(timer);
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

// ── File Info ──

function FileInfo({ entry, locale }: { entry: FileEntry; locale: Locale }) {
  const rows: [string, string][] = [
    [t(locale, 'filePreview.infoName'), entry.name],
    [t(locale, 'filePreview.infoPath'), entry.path],
    [t(locale, 'filePreview.infoType'), entry.kind],
    [t(locale, 'filePreview.infoSize'), `${(entry.size / 1024).toFixed(1)} KB (${entry.size} bytes)`],
    [t(locale, 'filePreview.infoModified'), new Date(entry.mtime).toLocaleString()],
    [t(locale, 'filePreview.infoCreated'), new Date(entry.btime).toLocaleString()],
    [t(locale, 'filePreview.infoHidden'), entry.hidden ? 'Yes' : 'No'],
  ];

  if (entry.isDir) rows.push([t(locale, 'filePreview.infoDirectory'), 'Yes']);
  if (entry.symlink) rows.push([t(locale, 'filePreview.infoSymlink'), entry.symlink]);
  if (entry.projectBadge) rows.push([t(locale, 'filePreview.infoProject'), entry.projectBadge]);

  return (
    <table style={{ width: '100%', fontSize: 12, borderCollapse: 'collapse' }}>
      <tbody>
        {rows.map(([key, val]) => (
          <tr key={key} style={{ borderBottom: '1px solid var(--border-subtle)' }}>
            <td style={{ padding: '6px 8px', color: 'var(--text-secondary)', fontWeight: 600, width: 80, verticalAlign: 'top' }}>
              {key}
            </td>
            <td style={{ padding: '6px 8px', color: 'var(--text)', wordBreak: 'break-all' }}>
              {val}
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

// ── Git Diff View ──

function GitDiffView({ diff, loading, status, fileName, locale }: {
  diff: string | null;
  loading: boolean;
  status: string | null;
  fileName: string;
  locale: Locale;
}) {
  if (loading) {
    return (
      <div style={{
        flex: 1,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        color: 'var(--text-disabled)',
        fontSize: 12,
        padding: 20,
      }}>
        {t(locale, 'filePreview.gitLoading')}
      </div>
    );
  }

  const noChangesMsg = t(locale, 'fileBrowser.noChanges');
  const notInRepoMsg = t(locale, 'fileBrowser.notInRepo');

  if (!diff || diff === notInRepoMsg || diff === noChangesMsg) {
    const tipText = diff === notInRepoMsg ? notInRepoMsg : noChangesMsg;
    return (
      <div style={{
        flex: 1,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        padding: 20,
      }}>
        <div style={{
          display: 'flex',
          flexDirection: 'column',
          alignItems: 'center',
          gap: 8,
          padding: '16px 24px',
          borderRadius: 8,
          background: 'color-mix(in srgb, var(--text-disabled) 6%, transparent)',
          border: '1px solid color-mix(in srgb, var(--text-disabled) 10%, transparent)',
          color: 'var(--text-secondary)',
          fontSize: 13,
          maxWidth: 260,
          textAlign: 'center',
          lineHeight: 1.5,
        }}>
          <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" style={{ opacity: 0.6 }}>
            <circle cx="12" cy="12" r="10" />
            <path d="M12 16v-4" />
            <path d="M12 8h.01" />
          </svg>
          <span>{tipText}</span>
        </div>
      </div>
    );
  }

  const parsed = parseUnifiedDiff(diff);
  if (!parsed) {
    const lines = diff.split('\n');
    return (
      <div style={{ flex: 1 }}>
        {status && (
          <div style={{
            display: 'flex', alignItems: 'center', gap: 6, marginBottom: 10,
            fontSize: 12, color: 'var(--text-secondary)',
          }}>
            <span style={{
              width: 8, height: 8, borderRadius: '50%',
              background: status === t(locale, 'filePreview.gitUnchanged') ? 'var(--primary)' : 'var(--warning)',
            }} />
            <span>{status}</span>
          </div>
        )}
        <pre style={{ margin: 0, fontSize: 11, lineHeight: 1.5, fontFamily: 'var(--font-mono)', color: 'var(--text)', whiteSpace: 'pre-wrap', wordBreak: 'break-all' }}>
          {lines.map((line, i) => {
            let color = 'var(--text)';
            const isChanged = line.startsWith('+') && !line.startsWith('+++');
            const isRemoved = line.startsWith('-') && !line.startsWith('---');
            if (isChanged) color = 'var(--primary)';
            else if (isRemoved) color = 'var(--danger)';
            else if (line.startsWith('@@')) color = 'var(--info)';
            else if (line.startsWith('diff') || line.startsWith('index')) color = 'var(--text-disabled)';
            return (
              <div key={i} className={isChanged || isRemoved ? 'anim-clFlash' : ''} style={{ color, background: isChanged ? 'var(--primary-soft)' : isRemoved ? 'color-mix(in srgb, var(--danger) 8%, transparent)' : undefined }}>
                {line || ' '}
              </div>
            );
          })}
        </pre>
      </div>
    );
  }

  return (
    <div style={{ display: 'flex', flexDirection: 'column', flex: 1, minHeight: 0 }}>
      {status && (
        <div style={{
          display: 'flex', alignItems: 'center', gap: 6, marginBottom: 8, padding: '4px 8px',
          fontSize: 11, color: 'var(--text-secondary)',
        }}>
          <span style={{
            width: 8, height: 8, borderRadius: '50%',
            background: status === t(locale, 'filePreview.gitUnchanged') ? 'var(--primary)' : 'var(--warning)',
          }} />
          <span>{status}</span>
        </div>
      )}
      <div style={{ flex: 1, minHeight: 0 }}>
        <MonacoDiffView
          original={parsed.original}
          modified={parsed.modified}
          language={detectLanguage(fileName)}
          fileName={fileName}
        />
      </div>
    </div>
  );
}
