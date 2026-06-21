'use client';

import { startTransition, useState, useEffect, useCallback, lazy, Suspense, useRef } from 'react';
import { Eye, Edit2, Pencil } from 'lucide-react';
import { MathCurveLoader } from '@/components/ui/MathCurveLoader';
import { type FileEntry } from '@/types/file';
import { t, useLocale, type Locale } from '@/i18n';
import { getExt, isMarkdownFile, isCsvFile, isArchiveFile } from '@/lib/follow-mode';
import { detectLanguage, highlightCode } from '@/lib/shiki-utils';
import { IFRAME_SANDBOX } from '@/lib/iframe-manager';
import { parseUnifiedDiff } from '@/lib/diff-utils';
import { useFileContent } from '@/lib/useFileContent';
import { type PreviewSubMode } from '@/components/shell/RightPanel';
import MonacoDiffView from './MonacoDiffView';
import ImageLightbox from './ImageLightbox';
import CsvTable from './CsvTable';
import ArchivePreview from './ArchivePreview';
import { getHttpPort } from '@/lib/natives-http-port';

// Lazy-loaded heavy components
const MilkdownEditor = lazy(() => import('./MilkdownEditor'));
const MonacoEditor = lazy(() => import('./MonacoEditor'));
const ImageEditor = lazy(() => import('./ImageEditor'));

interface FilePreviewProps {
  entry: FileEntry;
  subMode: PreviewSubMode;
  onClose: () => void;
  editMode?: boolean;
  onEditModeChange?: (mode: boolean) => void;
}

export { type PreviewSubMode };

export default function FilePreview({ entry, subMode, onClose, editMode = false, onEditModeChange }: FilePreviewProps) {
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

  const [httpPort, setHttpPort] = useState<number | null>(null);
  const ext = getExt(entry.name);
  const isMarkdown = isMarkdownFile(entry.name);
  const isCsv = isCsvFile(entry.name);
  const isArchive = isArchiveFile(entry.name);
  const isCode = entry.kind === 'text' && !isMarkdown && !isCsv;

  useEffect(() => {
    getHttpPort().then(setHttpPort).catch(() => {});
  }, []);

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
        padding: isMarkdown || isCsv || (subMode === 'preview' && isCode && editMode) ? 0 : '0 4px',
        display: 'flex',
        flexDirection: 'column',
      }}>
        {subMode === 'preview' && (
          isCode
            ? (
              <CodePreview
                entry={entry}
                locale={locale}
                editMode={editMode}
                ext={ext}
              />
            )
            : (
              <PreviewContent
                entry={entry}
                locale={locale}
                isMarkdown={isMarkdown}
                isCsv={isCsv}
                isArchive={isArchive}
                onImageClick={setLightboxSrc}
              />
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

// ── Preview Content ──

/** Build a blob URL for a file via Tauri IPC. Returns null for binary types that need a URL. */
function useFileBlobUrl(path: string, kind?: string): string | null {
  const [url, setUrl] = useState<string | null>(null);

  useEffect(() => {
    const api = window.nativesAPI;
    if (kind === 'image' || kind === 'video' || kind === 'audio' || kind === 'pdf') {
      if (api?.fs?.convertFileSrc) {
        startTransition(() => { setUrl(api.fs?.convertFileSrc?.(path) ?? ""); });
        return;
      }
    }

    let cancelled = false;
    (async () => {
      try {
        if (api?.fs?.readFile) {
          const result = await api.fs.readFile(path) as any;
          if (cancelled) return;
          const content = typeof result === 'string' ? result : result?.content;
          if (!content) return;
          // Infer MIME from extension
          const ext = path.split('.').pop()?.toLowerCase() || '';
          const mimeMap: Record<string, string> = {
            png: 'image/png', jpg: 'image/jpeg', jpeg: 'image/jpeg',
            gif: 'image/gif', webp: 'image/webp', svg: 'image/svg+xml',
            mp4: 'video/mp4', webm: 'video/webm', mov: 'video/quicktime',
            mp3: 'audio/mpeg', wav: 'audio/wav', ogg: 'audio/ogg',
            pdf: 'application/pdf',
          };
          const mime = mimeMap[ext] || 'application/octet-stream';
          // If encoding is base64, decode it
          let blob: Blob;
          if (result?.encoding === 'base64') {
            const byteString = atob(content);
            const ab = new ArrayBuffer(byteString.length);
            const ia = new Uint8Array(ab);
            for (let i = 0; i < byteString.length; i++) ia[i] = byteString.charCodeAt(i);
            blob = new Blob([ab], { type: mime });
          } else {
            blob = new Blob([content], { type: mime });
          }
          if (!cancelled) setUrl(URL.createObjectURL(blob));
        }
      } catch { /* ignore */ }
    })();
    return () => { cancelled = true; };
  }, [path, kind]);

  return url;
}

function PreviewContent({ entry, locale, isMarkdown, isCsv, isArchive, onImageClick }: {
  entry: FileEntry;
  locale: Locale;
  isMarkdown: boolean;
  isCsv: boolean;
  isArchive: boolean;
  onImageClick: (src: string) => void;
}) {
  const blobUrl = useFileBlobUrl(entry.path, entry.kind);
  const [imageEditing, setImageEditing] = useState(false);

  // Image with lightbox + edit button
  if (entry.kind === 'image' && blobUrl) {
    if (imageEditing) {
      return (
        <div style={{ display: 'flex', flexDirection: 'column', flex: 1, minHeight: 0 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 8, padding: '4px 8px', borderBottom: '1px solid var(--border)' }}>
            <button
              onClick={() => setImageEditing(false)}
              className="text-xs px-2 py-1 rounded"
              style={{ background: 'var(--vibe-btn-bg)', color: 'var(--text-dim)' }}
            >
              {t(locale, 'filePreview.backToPreview')}
            </button>
            <span className="text-xs" style={{ color: 'var(--text-faint)' }}>{entry.name}</span>
          </div>
          <Suspense fallback={<MathCurveLoader />}>
            <ImageEditor
              imagePath={blobUrl}
              imageName={entry.name}
              onSave={(dataUrl, ext, asNew) => {
                // Save via fs.saveBlob
                const api = window.nativesAPI;
                if (api?.fs?.saveBlob && dataUrl) {
                  const base64 = dataUrl.split(',')[1] || '';
                  const p = entry.path || '';
                  const dir = p.substring(0, p.lastIndexOf('/')) || '/';
                  const entryName = entry.name || 'image';
                  const name = asNew
                    ? entryName.replace(/\.[^.]+$/, '') + '-edited.' + ext
                    : entryName;
                  api.fs.saveBlob(dir, name, base64).catch(() => {});
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
          src={blobUrl}
          alt={entry.name}
          onClick={() => onImageClick(blobUrl)}
          style={{
            maxWidth: '100%', maxHeight: '100%', objectFit: 'contain',
            background: 'repeating-conic-gradient(#80808033 0% 25%, transparent 0% 50%) 50% / 20px 20px',
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
            background: 'var(--vibe-btn-bg)', color: 'var(--text-dim)',
            border: '1px solid var(--border)', cursor: 'pointer',
            backdropFilter: 'blur(8px)',
          }}
        >
          <Pencil size={13} />
          {t(locale, 'filePreview.editImage')}
        </button>
      </div>
    );
  }

  if (entry.kind === 'video' && blobUrl) {
    return (
      <video controls style={{ maxWidth: '100%', maxHeight: '100%' }}>
        <source src={blobUrl} />
      </video>
    );
  }

  if (entry.kind === 'audio' && blobUrl) {
    return (
      <audio controls style={{ width: '100%' }}>
        <source src={blobUrl} />
      </audio>
    );
  }

  if (entry.kind === 'pdf' && blobUrl) {
    return (
      <iframe
        src={blobUrl}
        style={{ width: '100%', height: '100%', border: 'none' }}
      />
    );
  }

  // CSV/TSV table
  if (isCsv) {
    return <CsvPreview path={entry.path} locale={locale} delimiter={entry.name.endsWith('.tsv') ? '\t' : ','} />;
  }

  // Markdown WYSIWYG (Milkdown Crepe)
  if (isMarkdown) {
    return <MdWysiwygPreview path={entry.path} locale={locale} />;
  }

  // HTML isolated preview
  if (entry.name.endsWith('.html') || entry.name.endsWith('.htm')) {
    return <HtmlFilePreview path={entry.path} locale={locale} />;
  }

  // Archive preview
  if (isArchive) {
    return <ArchivePreview path={entry.path} locale={locale} />;
  }

  return (
    <div style={{ color: 'var(--text-faint)', fontSize: 12, padding: 20, textAlign: 'center' }}>
      {t(locale, 'filePreview.noPreview').replace('{kind}', entry.kind)}
    </div>
  );
}

// ── CSV Preview ──

function CsvPreview({ path, locale, delimiter }: { path: string; locale: Locale; delimiter: string }) {
  const { content } = useFileContent(path);
  if (content === null) {
    return <div style={{ padding: 20, textAlign: 'center', color: 'var(--text-faint)', fontSize: 12 }}>{t(locale, 'filePreview.failedLoad')}</div>;
  }
  return <CsvTable content={content} delimiter={delimiter} />;
}

// ── Markdown WYSIWYG Preview ──

function MdWysiwygPreview({ path, locale }: { path: string; locale: Locale }) {
  const { content } = useFileContent(path);
  if (content === null) {
    return <div style={{ padding: 20, textAlign: 'center', color: 'var(--text-faint)', fontSize: 12 }}>{t(locale, 'filePreview.failedLoad')}</div>;
  }

  return (
    <Suspense fallback={
      <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', padding: 40, gap: 12 }}>
        <MathCurveLoader size={40} />
        <div style={{ color: 'var(--text-faint)', fontSize: 12 }}>Loading editor...</div>
      </div>
    }>
      <MilkdownEditor
        content={content}
        filePath={path}
        onSave={async (newContent) => {
          await window.nativesAPI?.fs?.writeFileAtomic?.(path, newContent);
        }}
      />
    </Suspense>
  );
}

function HtmlFilePreview({ path, locale }: { path: string; locale: Locale }) {
  const { content } = useFileContent(path);
  if (content === null) {
    return <div style={{ padding: 20, textAlign: 'center', color: 'var(--text-faint)', fontSize: 12 }}>{t(locale, 'filePreview.failedLoad')}</div>;
  }
  return (
    <iframe
      srcDoc={content}
      // File preview iframe: this is a LOCAL file viewer, NOT a plugin execution
      // environment. `allow-popups` is deliberately excluded to prevent HTML files
      // from opening new windows. `allow-same-origin` is excluded per security
      // standard R-S2 (plugin iframes must never have it; file previews follow
      // the same rule to keep the surface minimal).
      sandbox={IFRAME_SANDBOX}
      style={{ width: '100%', height: '100%', border: 'none', background: '#fff' }}
    />
  );
}

// ── Code Preview (shiki highlight + Monaco editor) ──

function CodePreview({ entry, locale, editMode, ext }: {
  entry: FileEntry;
  locale: Locale;
  editMode: boolean;
  ext: string;
}) {
  const [highlightedHtml, setHighlightedHtml] = useState<string>('');
  const { content: code, loading } = useFileContent(entry.path);

  // shiki syntax highlighting
  useEffect(() => {
    if (editMode || !code || ext === 'json') return;
    let cancelled = false;
    highlightCode(code, detectLanguage(entry.name)).then(html => {
      if (!cancelled) setHighlightedHtml(html);
    });
    return () => { cancelled = true; };
  }, [code, ext, entry.name, editMode]);

  if (loading || code === null) {
    return (
      <div style={{ color: 'var(--text-faint)', fontSize: 12, padding: 20, textAlign: 'center' }}>
        {t(locale, 'common.loading')}
      </div>
    );
  }

  // JSON: pretty print
  if (ext === 'json' && !editMode) {
    try {
      const pretty = JSON.stringify(JSON.parse(code), null, 2);
      return (
    // eslint-disable-next-line react-hooks/error-boundaries
        <pre style={{
          margin: 0, fontSize: 12, lineHeight: 1.6,
          fontFamily: 'var(--font-mono)',
          color: 'var(--vibe-brand-text)', whiteSpace: 'pre-wrap', wordBreak: 'break-all',
          background: 'transparent', padding: 12,
        }}>
          {pretty}
        </pre>
      );
    } catch { /* fall through to shiki */ }
  }

  // Edit mode: Monaco Editor
  if (editMode) {
    return (
      <Suspense fallback={
        <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', padding: 40, gap: 12 }}>
          <MathCurveLoader size={40} />
          <div style={{ color: 'var(--text-faint)', fontSize: 12 }}>Loading editor...</div>
        </div>
      }>
        <MonacoEditor
          content={code}
          language={ext}
          onSave={async (newContent) => {
            await window.nativesAPI?.fs?.writeFileAtomic?.(entry.path, newContent);
          }}
        />
      </Suspense>
    );
  }

  // View mode: shiki syntax highlighting
  if (highlightedHtml) {
    return (
      <div
        dangerouslySetInnerHTML={{ __html: highlightedHtml }}
        style={{
          fontSize: 12,
          lineHeight: 1.6,
          fontFamily: 'var(--font-mono)',
          background: 'transparent',
          padding: 12,
          overflow: 'auto',
        }}
      />
    );
  }

  // Fallback: plain text
  return (
    <pre style={{
      margin: 0, fontSize: 12, lineHeight: 1.6,
      fontFamily: 'var(--font-mono)',
      color: 'var(--vibe-brand-text)', whiteSpace: 'pre-wrap', wordBreak: 'break-all',
      background: 'transparent', padding: 12,
    }}>
      {code}
    </pre>
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
          <tr key={key} style={{ borderBottom: '1px solid var(--vibe-border-subtle)' }}>
            <td style={{ padding: '6px 8px', color: 'var(--text-dim)', fontWeight: 600, width: 80, verticalAlign: 'top' }}>
              {key}
            </td>
            <td style={{ padding: '6px 8px', color: 'var(--vibe-brand-text)', wordBreak: 'break-all' }}>
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
        color: 'var(--text-faint)',
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
          background: 'color-mix(in srgb, var(--text-faint) 6%, transparent)',
          border: '1px solid color-mix(in srgb, var(--text-faint) 10%, transparent)',
          color: 'var(--text-dim)',
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
            fontSize: 12, color: 'var(--text-dim)',
          }}>
            <span style={{
              width: 8, height: 8, borderRadius: '50%',
              background: status === t(locale, 'filePreview.gitUnchanged') ? 'var(--vibe-accent-color)' : 'var(--warning)',
            }} />
            <span>{status}</span>
          </div>
        )}
        <pre style={{ margin: 0, fontSize: 11, lineHeight: 1.5, fontFamily: 'var(--font-mono)', color: 'var(--vibe-brand-text)', whiteSpace: 'pre-wrap', wordBreak: 'break-all' }}>
          {lines.map((line, i) => {
            let color = 'var(--vibe-brand-text)';
            const isChanged = line.startsWith('+') && !line.startsWith('+++');
            const isRemoved = line.startsWith('-') && !line.startsWith('---');
            if (isChanged) color = 'var(--vibe-accent-color)';
            else if (isRemoved) color = 'var(--danger)';
            else if (line.startsWith('@@')) color = 'var(--info)';
            else if (line.startsWith('diff') || line.startsWith('index')) color = 'var(--text-faint)';
            return (
              <div key={i} className={isChanged || isRemoved ? 'anim-clFlash' : ''} style={{ color, background: isChanged ? 'var(--accent-soft)' : isRemoved ? 'color-mix(in srgb, var(--danger) 8%, transparent)' : undefined }}>
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
          fontSize: 11, color: 'var(--text-dim)',
        }}>
          <span style={{
            width: 8, height: 8, borderRadius: '50%',
            background: status === t(locale, 'filePreview.gitUnchanged') ? 'var(--vibe-accent-color)' : 'var(--warning)',
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
