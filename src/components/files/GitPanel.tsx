'use client';

import { useState, useEffect } from 'react';
import { type GitStatus } from '@/types/file';

interface GitPanelProps {
  repoPath: string;
}

export default function GitPanel({ repoPath }: GitPanelProps) {
  const [status, setStatus] = useState<GitStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const httpPort = window.__nativesHttpPort || 3001;

  useEffect(() => {
    async function load() {
      setLoading(true);
      try {
        const res = await fetch(`http://localhost:${httpPort}/api/fs/git?path=${encodeURIComponent(repoPath)}`);
        if (res.ok) {
          const data = await res.json();
          setStatus(data);
        }
      } catch { /* ignore */ }
      setLoading(false);
    }
    load();
  }, [repoPath, httpPort]);

  if (loading) return <div style={{ padding: 12, color: 'var(--text-faint)' }}>Loading git status...</div>;
  if (!status) return <div style={{ padding: 12, color: 'var(--text-faint)' }}>Not a git repository</div>;

  const STATUS_LABELS: Record<string, string> = {
    M: 'Modified', A: 'Added', D: 'Deleted', R: 'Renamed', '??': 'Untracked', UU: 'Conflicted',
  };

  const STATUS_COLORS: Record<string, string> = {
    M: '#cdf24b', A: '#4bcdf2', D: '#f24b4b', R: '#f2a14b', '??': '#9b9d8c', UU: '#f24b4b',
  };

  return (
    <div style={{ padding: 8 }}>
      {/* Branch info */}
      <div style={{
        display: 'flex', alignItems: 'center', gap: 6,
        padding: '6px 8px', marginBottom: 8,
        background: 'var(--bg-2, #131410)',
        borderRadius: 'var(--radius, 4px)',
        fontSize: 12, fontWeight: 600, color: 'var(--text, #f2f2ea)',
      }}>
        <span>⎇</span>
        <span>{status.branch}</span>
        <span style={{ fontSize: 10, color: 'var(--text-faint, #62655a)' }}>
          {status.files.length} file{status.files.length !== 1 ? 's' : ''}
        </span>
      </div>

      {/* File list */}
      {status.files.length === 0 ? (
        <div style={{ padding: 20, textAlign: 'center', color: 'var(--text-faint, #62655a)', fontSize: 12 }}>
          Working tree clean
        </div>
      ) : (
        status.files.map((file) => (
          <div
            key={file.path}
            style={{
              display: 'flex', alignItems: 'center', gap: 6,
              padding: '4px 8px', fontSize: 12,
              borderBottom: '1px solid var(--border, #262920)',
              color: 'var(--text, #f2f2ea)',
            }}
          >
            <span style={{
              fontSize: 10, fontWeight: 700, padding: '1px 4px', borderRadius: 2,
              background: `${STATUS_COLORS[file.status] || '#888'}25`,
              color: STATUS_COLORS[file.status] || '#888',
            }}>
              {file.status}
            </span>
            <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
              {file.path}
            </span>
            {file.oldPath && (
              <span style={{ fontSize: 10, color: 'var(--text-faint, #62655a)' }}>
                ← {file.oldPath}
              </span>
            )}
          </div>
        ))
      )}
    </div>
  );
}
