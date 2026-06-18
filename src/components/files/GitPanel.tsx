'use client';

import { useState, useEffect } from 'react';
import { type GitStatus } from '@/types/file';
import { t, type Locale } from '@/i18n';

interface GitPanelProps {
  repoPath: string;
}

export default function GitPanel({ repoPath }: GitPanelProps) {
  const [status, setStatus] = useState<GitStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [locale, setLocale] = useState<Locale>('en');
  const httpPort = window.__nativesHttpPort || 3001;

  useEffect(() => {
    async function loadLocale() {
      try {
        const saved = await window.nativesAPI?.getLocale?.();
        if (saved === 'en') setLocale('en'); else setLocale('zh');
      } catch { /* ignore */ }
    }
    loadLocale();
  }, []);

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

  if (loading) return <div style={{ padding: 12, color: 'var(--text-faint)' }}>{t(locale, 'fileBrowser.loadingGitStatus')}</div>;
  if (!status) return <div style={{ padding: 12, color: 'var(--text-faint)' }}>{t(locale, 'fileBrowser.notInRepo')}</div>;

  const STATUS_LABELS: Record<string, string> = {
    M: t(locale, 'filePreview.gitModified'),
    A: t(locale, 'filePreview.gitAdded'),
    D: t(locale, 'filePreview.gitDeleted'),
    R: t(locale, 'filePreview.gitRenamed'),
    '??': t(locale, 'filePreview.gitUntracked'),
    UU: t(locale, 'filePreview.gitConflict'),
  };

  const STATUS_COLORS: Record<string, string> = {
    M: 'var(--diff-mod)', A: 'var(--diff-add)', D: 'var(--diff-del)', R: 'var(--diff-mod)', '??': 'var(--text-faint)', UU: 'var(--danger)',
  };

  return (
    <div style={{ padding: 8 }}>
      {/* Branch info */}
      <div style={{
        display: 'flex', alignItems: 'center', gap: 6,
        padding: '6px 8px', marginBottom: 8,
        background: 'var(--vibe-toolbar-bg)',
        borderRadius: 'var(--radius, 4px)',
        fontSize: 12, fontWeight: 600, color: 'var(--vibe-brand-text)',
      }}>
        <span>⎇</span>
        <span>{status.branch}</span>
        <span style={{ fontSize: 10, color: 'var(--vibe-btn-text)' }}>
          {status.files.length}
        </span>
      </div>

      {/* File list */}
      {status.files.length === 0 ? (
        <div style={{ padding: 20, textAlign: 'center', color: 'var(--vibe-btn-text)', fontSize: 12 }}>
          {t(locale, 'fileBrowser.workingTreeClean')}
        </div>
      ) : (
        status.files.map((file) => (
          <div
            key={file.path}
            style={{
              display: 'flex', alignItems: 'center', gap: 6,
              padding: '4px 8px', fontSize: 12,
              borderBottom: '1px solid var(--vibe-btn-border)',
              color: 'var(--vibe-brand-text)',
            }}
          >
            <span className="anim-changedBreath" style={{
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
              <span style={{ fontSize: 10, color: 'var(--vibe-btn-text)' }}>
                ← {file.oldPath}
              </span>
            )}
          </div>
        ))
      )}
    </div>
  );
}
