'use client';

import { useState, useEffect, useCallback, lazy, Suspense, useRef } from 'react';
import { type FileEntry } from '@/types/file';
import { t, useLocale, type Locale } from '@/i18n';
import { getExt, isMarkdownFile, isCsvFile, isArchiveFile } from '@/lib/follow-mode';
import { detectLanguage, highlightCode } from '@/lib/shiki-utils';
import { parseUnifiedDiff } from '@/lib/diff-utils';
import { useFileContent } from '@/lib/useFileContent';
import MonacoDiffView from './MonacoDiffView';
import ImageLightbox from './ImageLightbox';
import CsvTable from './CsvTable';
import ArchivePreview from './ArchivePreview';

// Lazy-loaded heavy components
const MilkdownEditor = lazy(() => import('./MilkdownEditor'));
const MonacoEditor = lazy(() => import('./MonacoEditor'));

interface FilePreviewProps {
  entry: FileEntry;
  onClose: () => void;
}

type PreviewTab = 'preview' | 'code' | 'git' | 'info';

export default function FilePreview({ entry, onClose }: FilePreviewProps) {
  const [activeTab, setActiveTab] = useState<PreviewTab>('preview');
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

  // Load git diff when git tab is selected
  useEffect(() => {
    if (activeTab !== 'git') return;
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
  }, [activeTab, entry.path, entry.name, entry.isDir, locale]);

  const tabs: { id: PreviewTab; label: string }[] = [
    { id: 'preview', label: t(locale, 'filePreview.tabPreview') },
    { id: 'code', label: t(locale, 'filePreview.tabCode') },
    { id: 'git', label: t(locale, 'filePreview.tabGit') },
    { id: 'info', label: t(locale, 'filePreview.tabInfo') },
  ];

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
      height: '100%',
      background: 'var(--bg, #0b0c0a)',
    }}>
      {/* Header */}
      <div style={{
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'space-between',
        padding: '8px 12px',
        borderBottom: '1px solid var(--border, #262920)',
      }}>
        <div style={{ fontSize: 13, fontWeight: 600, color: 'var(--text, #f2f2ea)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
          {entry.name}
        </div>
        <div style={{ display: 'flex', gap: 4, alignItems: 'center' }}>
          {/* Edit mode toggle for code files */}
          {activeTab === 'code' && isCode && (
            <button
              className="btn btn-ghost"
              onClick={() => setEditMode(!editMode)}
              style={{ fontSize: 11, padding: '2px 8px' }}
              title={editMode ? 'View mode' : 'Edit mode'}
            >
              {editMode ? '👁' : '✏️'}
            </button>
          )}
          <button
            className="btn btn-ghost"
            onClick={onClose}
            aria-label="Close preview"
            style={{ fontSize: 16, padding: '0 6px', lineHeight: '24px' }}
          >
            ✕
          </button>
        </div>
      </div>

      {/* Tabs */}
      <div role="tablist" style={{ display: 'flex', borderBottom: '1px solid var(--border, #262920)' }}>
        {tabs.map((tab) => (
          <button
            key={tab.id}
            role="tab"
            aria-selected={activeTab === tab.id}
            className="btn btn-ghost"
            onClick={() => setActiveTab(tab.id)}
            style={{
              flex: 1,
              fontSize: 11,
              padding: '6px 8px',
              borderBottom: activeTab === tab.id ? '2px solid var(--accent, #cdf24b)' : '2px solid transparent',
              color: activeTab === tab.id ? 'var(--accent, #cdf24b)' : 'var(--text-dim, #9b9d8c)',
              borderRadius: 0,
            }}
          >
            {tab.label}
          </button>
        ))}
      </div>

      {/* Content */}
      <div style={{ flex: 1, overflow: 'auto', padding: isMarkdown || isCsv || (activeTab === 'code' && editMode) ? 0 : 12 }}>
        {activeTab === 'preview' && (
          <PreviewContent
            entry={entry}
            httpPort={httpPort}
            locale={locale}
            isMarkdown={isMarkdown}
            isCsv={isCsv}
            isArchive={isArchive}
            onImageClick={setLightboxSrc}
          />
        )}
        {activeTab === 'code' && (
          <CodePreview
            entry={entry}
            httpPort={httpPort}
            locale={locale}
            editMode={editMode}
            ext={ext}
          />
        )}
        {activeTab === 'git' && (
          <GitDiffView diff={gitDiff} loading={gitLoading} status={gitStatus} fileName={entry.name} locale={locale} />
        )}
        {activeTab === 'info' && (
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
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', height: '100%', cursor: 'zoom-in' }}>
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
    <div style={{ color: 'var(--text-dim, #9b9d8c)', fontSize: 12, padding: 20, textAlign: 'center' }}>
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
      sandbox="allow-scripts allow-forms allow-popups"
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
          fontFamily: 'var(--font-mono, monospace)',
          color: 'var(--text)', whiteSpace: 'pre-wrap', wordBreak: 'break-all',
          background: 'var(--bg-2, #131410)', padding: 12, borderRadius: 4,
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
          fontFamily: 'var(--font-mono, monospace)',
          background: 'var(--bg-2, #131410)',
          padding: 12,
          borderRadius: 4,
          overflow: 'auto',
        }}
      />
    );
  }

  // Fallback: plain text
  return (
    <pre style={{
      margin: 0, fontSize: 12, lineHeight: 1.6,
      fontFamily: 'var(--font-mono, monospace)',
      color: 'var(--text)', whiteSpace: 'pre-wrap', wordBreak: 'break-all',
      background: 'var(--bg-2, #131410)', padding: 12, borderRadius: 4,
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
          <tr key={key} style={{ borderBottom: '1px solid var(--border, #262920)' }}>
            <td style={{ padding: '6px 8px', color: 'var(--text-dim, #9b9d8c)', fontWeight: 600, width: 80, verticalAlign: 'top' }}>
              {key}
            </td>
            <td style={{ padding: '6px 8px', color: 'var(--text, #f2f2ea)', wordBreak: 'break-all' }}>
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
      <div style={{ color: 'var(--text-faint)', fontSize: 12, padding: 20, textAlign: 'center' }}>
        {t(locale, 'filePreview.gitLoading')}
      </div>
    );
  }

  if (!diff || diff === t(locale, 'fileBrowser.notInRepo')) {
    return (
      <div style={{ color: 'var(--text-faint)', fontSize: 12, padding: 20, textAlign: 'center' }}>
        {t(locale, 'fileBrowser.notInRepo')}
      </div>
    );
  }

  const parsed = parseUnifiedDiff(diff);
  if (!parsed) {
    const lines = diff.split('\n');
    return (
      <div>
        {status && (
          <div style={{
            display: 'flex', alignItems: 'center', gap: 6, marginBottom: 10,
            fontSize: 12, color: 'var(--text-dim)',
          }}>
            <span style={{
              width: 8, height: 8, borderRadius: '50%',
              background: status === t(locale, 'filePreview.gitUnchanged') ? 'var(--accent,#cdf24b)' : 'var(--warning)',
            }} />
            <span>{status}</span>
          </div>
        )}
        <pre style={{ margin: 0, fontSize: 11, lineHeight: 1.5, fontFamily: 'var(--font-mono, monospace)', color: 'var(--text)', whiteSpace: 'pre-wrap', wordBreak: 'break-all' }}>
          {lines.map((line, i) => {
            let color = 'var(--text)';
            const isChanged = line.startsWith('+') && !line.startsWith('+++');
            const isRemoved = line.startsWith('-') && !line.startsWith('---');
            if (isChanged) color = 'var(--diff-add)';
            else if (isRemoved) color = 'var(--danger)';
            else if (line.startsWith('@@')) color = '#5b9cf5';
            else if (line.startsWith('diff') || line.startsWith('index')) color = 'var(--text-faint)';
            return (
              <div key={i} className={isChanged || isRemoved ? 'anim-clFlash' : ''} style={{ color, background: isChanged ? '#4ec9b010' : isRemoved ? '#d9534f10' : undefined }}>
                {line || ' '}
              </div>
            );
          })}
        </pre>
      </div>
    );
  }

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
      {status && (
        <div style={{
          display: 'flex', alignItems: 'center', gap: 6, marginBottom: 8, padding: '4px 8px',
          fontSize: 11, color: 'var(--text-dim)',
        }}>
          <span style={{
            width: 8, height: 8, borderRadius: '50%',
            background: status === t(locale, 'filePreview.gitUnchanged') ? 'var(--accent,#cdf24b)' : 'var(--warning)',
          }} />
          <span>{status}</span>
        </div>
      )}
      <div style={{ flex: 1, minHeight: 200 }}>
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

