'use client';

import { useState, useEffect, useCallback, lazy, Suspense, useRef } from 'react';
import { Eye, Edit2 } from 'lucide-react';
import { type FileEntry } from '@/types/file';
import { t, useLocale, type Locale } from '@/i18n';
import { getExt, isMarkdownFile, isCsvFile, isArchiveFile } from '@/lib/follow-mode';
import { detectLanguage, highlightCode } from '@/lib/shiki-utils';
import { parseUnifiedDiff } from '@/lib/diff-utils';
import { useFileContent } from '@/lib/useFileContent';
import { type PreviewSubMode } from '@/components/shell/RightPanel';
import MonacoDiffView from './MonacoDiffView';
import ImageLightbox from './ImageLightbox';
import CsvTable from './CsvTable';
import ArchivePreview from './ArchivePreview';

// Lazy-loaded heavy components
const MilkdownEditor = lazy(() => import('./MilkdownEditor'));
const MonacoEditor = lazy(() => import('./MonacoEditor'));

interface FilePreviewProps {
  entry: FileEntry;
  subMode: PreviewSubMode;
  onClose: () => void;
}

export { type PreviewSubMode };

export default function FilePreview({ entry, subMode, onClose }: FilePreviewProps) {
  const [gitDiff, setGitDiff] = useState<string | null>(null);
  const [gitLoading, setGitLoading] = useState(false);
  const [gitStatus, setGitStatus] = useState<string | null>(null);
  const locale = useLocale();
  const [lightboxSrc, setLightboxSrc] = useState<string | null>(null);
  const [editMode, setEditMode] = useState(false);
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
            setGitDiff(diff || t(locale, 'fileBrowser.noChanges'));
          }
        }
        if (api?.git?.status) {
          const dirPath = entry.isDir ? entry.path : entry.path.substring(0, entry.path.lastIndexOf('/')) || '/';
          const status = await api.git.status(dirPath);
          if (!cancelled && status) {
            const fileStatus = (status.files || []).find(
              (f: { path: string; status: string }) => f.path === entry.path || f.path === entry.name
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

  const httpPort = window.__nativesHttpPort || 3001;
  const ext = getExt(entry.name);
  const isMarkdown = isMarkdownFile(entry.name);
  const isCsv = isCsvFile(entry.name);
  const isArchive = isArchiveFile(entry.name);
  const isCode = entry.kind === 'text' && !isMarkdown && !isCsv;

  return (
    <div ref={containerRef} tabIndex={-1} role="dialog" aria-label={entry.name} style={{
      display: 'flex',
      flexDirection: 'column',
      flex: 1,
      minHeight: 0,
      background: 'transparent',
    }}>
      {/* Top bar: edit toggle for code preview */}
      {subMode === 'preview' && isCode && (
        <div style={{
          display: 'flex',
          justifyContent: 'flex-end',
          padding: '4px 8px',
          borderBottom: '1px solid var(--vibe-sidebar-border)',
        }}>
          <button
            className="flex items-center justify-center p-1 rounded-lg text-[var(--text-faint)] hover:bg-[var(--vibe-btn-hover-bg)] hover:text-[var(--vibe-btn-hover-color)] transition-all"
            onClick={() => setEditMode(!editMode)}
            title={editMode ? 'View mode' : 'Edit mode'}
          >
            {editMode ? <Eye size={13} /> : <Edit2 size={13} />}
          </button>
        </div>
      )}

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
                httpPort={httpPort}
                locale={locale}
                editMode={editMode}
                ext={ext}
              />
            )
            : (
              <PreviewContent
                entry={entry}
                httpPort={httpPort}
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

function PreviewContent({ entry, httpPort, locale, isMarkdown, isCsv, isArchive, onImageClick }: {
  entry: FileEntry;
  httpPort: number;
  locale: Locale;
  isMarkdown: boolean;
  isCsv: boolean;
  isArchive: boolean;
  onImageClick: (src: string) => void;
}) {
  // Image with lightbox
  if (entry.kind === 'image') {
    const src = `http://localhost:${httpPort}/api/fs/raw?path=${encodeURIComponent(entry.path)}`;
    return (
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', flex: 1, cursor: 'zoom-in' }}>
        <img
          src={src}
          alt={entry.name}
          onClick={() => onImageClick(src)}
          style={{
            maxWidth: '100%', maxHeight: '100%', objectFit: 'contain',
            background: 'repeating-conic-gradient(#80808033 0% 25%, transparent 0% 50%) 50% / 20px 20px',
          }}
        />
      </div>
    );
  }

  if (entry.kind === 'video') {
    return (
      <video controls style={{ maxWidth: '100%', maxHeight: '100%' }}>
        <source src={`http://localhost:${httpPort}/api/fs/raw?path=${encodeURIComponent(entry.path)}`} />
      </video>
    );
  }

  if (entry.kind === 'audio') {
    return (
      <audio controls style={{ width: '100%' }}>
        <source src={`http://localhost:${httpPort}/api/fs/raw?path=${encodeURIComponent(entry.path)}`} />
      </audio>
    );
  }

  if (entry.kind === 'pdf') {
    return (
      <iframe
        src={`http://localhost:${httpPort}/api/fs/raw?path=${encodeURIComponent(entry.path)}`}
        style={{ width: '100%', height: '100%', border: 'none' }}
      />
    );
  }

  // CSV/TSV table
  if (isCsv) {
    return <CsvPreview path={entry.path} httpPort={httpPort} locale={locale} delimiter={entry.name.endsWith('.tsv') ? '\t' : ','} />;
  }

  // Markdown WYSIWYG (Milkdown Crepe)
  if (isMarkdown) {
    return <MdWysiwygPreview path={entry.path} httpPort={httpPort} locale={locale} />;
  }

  // HTML isolated preview
  if (entry.name.endsWith('.html') || entry.name.endsWith('.htm')) {
    return <HtmlFilePreview path={entry.path} httpPort={httpPort} locale={locale} />;
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

function CsvPreview({ path, httpPort, locale, delimiter }: { path: string; httpPort: number; locale: Locale; delimiter: string }) {
  const { content } = useFileContent(path, httpPort);
  if (content === null) {
    return <div style={{ padding: 20, textAlign: 'center', color: 'var(--text-faint)', fontSize: 12 }}>{t(locale, 'filePreview.failedLoad')}</div>;
  }
  return <CsvTable content={content} delimiter={delimiter} />;
}

// ── Markdown WYSIWYG Preview ──

function MdWysiwygPreview({ path, httpPort, locale }: { path: string; httpPort: number; locale: Locale }) {
  const { content } = useFileContent(path, httpPort);
  if (content === null) {
    return <div style={{ padding: 20, textAlign: 'center', color: 'var(--text-faint)', fontSize: 12 }}>{t(locale, 'filePreview.failedLoad')}</div>;
  }

  return (
    <Suspense fallback={<div style={{ padding: 20, textAlign: 'center', color: 'var(--text-faint)', fontSize: 12 }}>Loading editor...</div>}>
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

function HtmlFilePreview({ path, httpPort, locale }: { path: string; httpPort: number; locale: Locale }) {
  const { content } = useFileContent(path, httpPort);
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
      sandbox="allow-scripts allow-forms"
      style={{ width: '100%', height: '100%', border: 'none', background: '#fff' }}
    />
  );
}

// ── Code Preview (shiki highlight + Monaco editor) ──

function CodePreview({ entry, httpPort, locale, editMode, ext }: {
  entry: FileEntry;
  httpPort: number;
  locale: Locale;
  editMode: boolean;
  ext: string;
}) {
  const [highlightedHtml, setHighlightedHtml] = useState<string>('');
  const { content: code, loading } = useFileContent(entry.path, httpPort);

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
      <Suspense fallback={<div style={{ padding: 20, textAlign: 'center', color: 'var(--text-faint)', fontSize: 12 }}>Loading editor...</div>}>
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
