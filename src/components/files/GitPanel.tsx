'use client';

import { useState, useEffect } from 'react';
import { SPACING, FONT_SIZE } from '@/lib/design-tokens';
import { type GitStatus } from '@/types/file';
import { t, type Locale } from '@/i18n';

interface GitPanelProps {
  repoPath: string;
}

export default function GitPanel({ repoPath }: GitPanelProps) {
  const [status, setStatus] = useState<GitStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [locale, setLocale] = useState<Locale>('en');

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
        const api = window.nativesAPI;
        if (!api?.git?.status) {
          setStatus(null);
          setLoading(false);
          return;
        }
        const data = await api.git.status(repoPath);
        setStatus(data as GitStatus | null);
      } catch { /* ignore */ }
      setLoading(false);
    }
    load();
  }, [repoPath]);

  if (loading) return <div style={{ padding: SPACING.md, color: 'var(--text-faint)' }}>{t(locale, 'fileBrowser.loadingGitStatus')}</div>;
  if (!status) return <div style={{ padding: SPACING.md, color: 'var(--text-faint)' }}>{t(locale, 'fileBrowser.notInRepo')}</div>;

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
    <div style={{ padding: SPACING.sm }}>
      {/* Branch info */}
      <div style={{
        display: 'flex', alignItems: 'center', gap: SPACING.xs,
        padding: '6px 8px', marginBottom: SPACING.sm,
        background: 'var(--vibe-toolbar-bg)',
        borderRadius: 'var(--radius, 4px)',
        fontSize: FONT_SIZE.md, fontWeight: 600, color: 'var(--vibe-brand-text)',
      }}>
        <span>⎇</span>
        <span>{status.branch}</span>
        <span style={{ fontSize: FONT_SIZE.xs, color: 'var(--vibe-btn-text)' }}>
          {status.files.length}
        </span>
      </div>

      {/* File list */}
      {status.files.length === 0 ? (
        <div style={{ padding: SPACING.xl, textAlign: 'center', color: 'var(--vibe-btn-text)', fontSize: FONT_SIZE.md }}>
          {t(locale, 'fileBrowser.workingTreeClean')}
        </div>
      ) : (
        status.files.map((file) => (
          <div
            key={file.path}
            style={{
              display: 'flex', alignItems: 'center', gap: SPACING.xs,
              padding: '4px 8px', fontSize: FONT_SIZE.md,
              borderBottom: '1px solid var(--vibe-btn-border)',
              color: 'var(--vibe-brand-text)',
            }}
          >
            <span className="anim-changedBreath" style={{
              fontSize: FONT_SIZE.xs, fontWeight: 700, padding: '1px 4px', borderRadius: 2,
              background: `${STATUS_COLORS[file.status] || '#888'}25`,
              color: STATUS_COLORS[file.status] || '#888',
            }}>
              {file.status}
            </span>
            <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
              {file.path}
            </span>
            {file.oldPath && (
              <span style={{ fontSize: FONT_SIZE.xs, color: 'var(--vibe-btn-text)' }}>
                ← {file.oldPath}
              </span>
            )}
          </div>
        ))
      )}
    </div>
  );
}
