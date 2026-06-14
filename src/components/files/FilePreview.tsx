'use client';

import { useState, useEffect } from 'react';
import { type FileEntry } from '@/types/file';

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
            setGitDiff(diff || 'No changes detected');
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
                'M': 'Modified', 'A': 'Added', 'D': 'Deleted',
                'R': 'Renamed', '??': 'Untracked', 'UU': 'Conflict',
              };
              setGitStatus(statusMap[fileStatus.status] || fileStatus.status);
            } else {
              setGitStatus('Unchanged');
            }
          }
        }
      } catch {
        if (!cancelled) setGitDiff('Not in a git repository');
      } finally {
        if (!cancelled) setGitLoading(false);
      }
    }
    loadGit();
    return () => { cancelled = true; };
  }, [activeTab, entry.path, entry.name, entry.isDir]);

  const tabs: { id: PreviewTab; label: string }[] = [
    { id: 'preview', label: 'Preview' },
    { id: 'code', label: 'Code' },
    { id: 'git', label: 'Git' },
    { id: 'info', label: 'Info' },
  ];

  const httpPort = window.__nativesHttpPort || 3001;

  return (
    <div style={{
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
        <button
          className="btn btn-ghost"
          onClick={onClose}
          style={{ fontSize: 16, padding: '0 6px', lineHeight: '24px' }}
        >
          ✕
        </button>
      </div>

      {/* Tabs */}
      <div style={{ display: 'flex', borderBottom: '1px solid var(--border, #262920)' }}>
        {tabs.map((tab) => (
          <button
            key={tab.id}
            className={`btn btn-ghost`}
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
      <div style={{ flex: 1, overflow: 'auto', padding: 12 }}>
        {activeTab === 'preview' && (
          <PreviewContent entry={entry} httpPort={httpPort} />
        )}
        {activeTab === 'code' && (
          <CodePreview entry={entry} httpPort={httpPort} />
        )}
        {activeTab === 'git' && (
          <GitDiffView diff={gitDiff} loading={gitLoading} status={gitStatus} fileName={entry.name} />
        )}
        {activeTab === 'info' && (
          <FileInfo entry={entry} />
        )}
      </div>
    </div>
  );
}

function PreviewContent({ entry, httpPort }: { entry: FileEntry; httpPort: number }) {
  if (entry.kind === 'image') {
    return (
      <img
        src={`http://localhost:${httpPort}/api/fs/raw?path=${encodeURIComponent(entry.path)}`}
        alt={entry.name}
        style={{
          maxWidth: '100%', maxHeight: '100%', objectFit: 'contain',
          background: 'repeating-conic-gradient(#80808033 0% 25%, transparent 0% 50%) 50% / 20px 20px',
        }}
      />
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

  return (
    <div style={{ color: 'var(--text-dim, #9b9d8c)', fontSize: 12, padding: 20, textAlign: 'center' }}>
      No preview available for {entry.kind} files.
    </div>
  );
}

function CodePreview({ entry, httpPort }: { entry: FileEntry; httpPort: number }) {
  const [code, setCode] = useState<string | null>(null);
  const [highlighted, setHighlighted] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    async function load() {
      setLoading(true);
      try {
        // Fetch file content
        const resp = await fetch(`http://localhost:${httpPort}/api/fs/raw?path=${encodeURIComponent(entry.path)}`);
        if (!resp.ok) throw new Error('Failed to fetch');
        const text = await resp.text();
        if (cancelled) return;
        setCode(text);

        // Detect language from extension
        const ext = entry.name.split('.').pop()?.toLowerCase() || '';
        const langMap: Record<string, string> = {
          ts: 'typescript', tsx: 'tsx', js: 'javascript', jsx: 'jsx',
          py: 'python', rb: 'ruby', rs: 'rust', go: 'go',
          java: 'java', kt: 'kotlin', swift: 'swift',
          css: 'css', scss: 'scss', less: 'less',
          html: 'html', xml: 'xml', svg: 'svg',
          json: 'json', yaml: 'yaml', yml: 'yaml', toml: 'toml',
          md: 'markdown', sh: 'bash', zsh: 'bash', fish: 'fish',
          sql: 'sql', graphql: 'graphql',
          c: 'c', cpp: 'cpp', h: 'c', hpp: 'cpp',
          cs: 'csharp', php: 'php', lua: 'lua', vim: 'vim',
          dockerfile: 'dockerfile', makefile: 'makefile',
        };
        const lang = langMap[ext] || 'text';

        // Highlight with Shiki
        try {
          const { codeToHtml } = await import('shiki');
          const html = await codeToHtml(text, {
            lang,
            theme: 'vitesse-dark',
          });
          if (!cancelled) setHighlighted(html);
        } catch {
          // Fallback: plain text in a pre block
          if (!cancelled) setHighlighted(null);
        }
      } catch {
        if (!cancelled) setCode('Failed to load file');
      } finally {
        if (!cancelled) setLoading(false);
      }
    }
    load();
    return () => { cancelled = true; };
  }, [entry.path, entry.name, httpPort]);

  if (loading) {
    return (
      <div style={{ padding: 20, textAlign: 'center', color: 'var(--text-faint)', fontSize: 12 }}>
        Loading...
      </div>
    );
  }

  if (highlighted) {
    return (
      <div
        style={{
          fontSize: 12, lineHeight: 1.6,
          fontFamily: 'var(--font-mono, monospace)',
          overflow: 'auto', height: '100%',
          background: 'var(--bg-2, #131410)',
          borderRadius: 4,
        }}
        dangerouslySetInnerHTML={{ __html: highlighted }}
      />
    );
  }

  // Fallback: plain text
  return (
    <pre style={{
      margin: 0, fontSize: 12, lineHeight: 1.6,
      fontFamily: 'var(--font-mono, monospace)',
      color: 'var(--text)', whiteSpace: 'pre-wrap', wordBreak: 'break-all',
      background: 'var(--bg-2, #131410)',
      padding: 12, borderRadius: 4,
    }}>
      {code}
    </pre>
  );
}

function FileInfo({ entry }: { entry: FileEntry }) {
  const rows: [string, string][] = [
    ['Name', entry.name],
    ['Path', entry.path],
    ['Type', entry.kind],
    ['Size', `${(entry.size / 1024).toFixed(1)} KB (${entry.size} bytes)`],
    ['Modified', new Date(entry.mtime).toLocaleString()],
    ['Created', new Date(entry.btime).toLocaleString()],
    ['Hidden', entry.hidden ? 'Yes' : 'No'],
  ];

  if (entry.isDir) rows.push(['Directory', 'Yes']);
  if (entry.symlink) rows.push(['Symlink', entry.symlink]);
  if (entry.projectBadge) rows.push(['Project', entry.projectBadge]);

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

function GitDiffView({ diff, loading, status, fileName }: {
  diff: string | null;
  loading: boolean;
  status: string | null;
  fileName: string;
}) {
  if (loading) {
    return (
      <div style={{ color: 'var(--text-faint)', fontSize: 12, padding: 20, textAlign: 'center' }}>
        Loading git info...
      </div>
    );
  }

  if (!diff || diff === 'Not in a git repository') {
    return (
      <div style={{ color: 'var(--text-faint)', fontSize: 12, padding: 20, textAlign: 'center' }}>
        Not in a git repository
      </div>
    );
  }

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
            background: status === 'Unchanged' ? 'var(--accent,#cdf24b)' : '#e6b800',
          }} />
          <span>{status}</span>
        </div>
      )}
      <pre style={{
        margin: 0, fontSize: 11, lineHeight: 1.5,
        fontFamily: 'var(--font-mono, monospace)',
        color: 'var(--text)', whiteSpace: 'pre-wrap', wordBreak: 'break-all',
      }}>
        {lines.map((line, i) => {
          let color = 'var(--text)';
          if (line.startsWith('+') && !line.startsWith('+++')) color = '#4ec9b0';
          else if (line.startsWith('-') && !line.startsWith('---')) color = '#d9534f';
          else if (line.startsWith('@@')) color = '#5b9cf5';
          else if (line.startsWith('diff') || line.startsWith('index')) color = 'var(--text-faint)';
          return (
            <div key={i} style={{ color, background: line.startsWith('+') && !line.startsWith('+++') ? '#4ec9b010' : line.startsWith('-') && !line.startsWith('---') ? '#d9534f10' : undefined }}>
              {line || ' '}
            </div>
          );
        })}
      </pre>
    </div>
  );
}
