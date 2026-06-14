'use client';

import { useState } from 'react';
import { type FileEntry } from '@/types/file';

interface FilePreviewProps {
  entry: FileEntry;
  onClose: () => void;
}

type PreviewTab = 'preview' | 'code' | 'git' | 'info';

export default function FilePreview({ entry, onClose }: FilePreviewProps) {
  const [activeTab, setActiveTab] = useState<PreviewTab>('preview');

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
          <div style={{ color: 'var(--text-dim, #9b9d8c)', fontSize: 12 }}>
            Git status and diff for {entry.name} will appear here.
          </div>
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
  return (
    <iframe
      src={`http://localhost:${httpPort}/api/fs/raw?path=${encodeURIComponent(entry.path)}`}
      style={{
        width: '100%', height: '100%', border: 'none',
        background: 'var(--bg-2, #131410)',
        color: 'var(--text, #f2f2ea)',
        fontFamily: 'var(--font-ui, monospace)',
        fontSize: 12,
        whiteSpace: 'pre-wrap',
      }}
      sandbox="allow-same-origin"
    />
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
