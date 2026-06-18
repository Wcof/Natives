'use client';

import { useState, useEffect } from 'react';
import { Folder, FileText } from 'lucide-react';
import { t, type Locale } from '@/i18n';
import { formatSize } from '@/lib/diff-utils';

interface ArchiveEntry {
  name: string;
  size: number;
  isDir: boolean;
}

interface ArchivePreviewProps {
  path: string;
  locale: Locale;
}

export default function ArchivePreview({ path, locale }: ArchivePreviewProps) {
  const [entries, setEntries] = useState<ArchiveEntry[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [truncated, setTruncated] = useState(false);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const result = await window.nativesAPI?.archive?.list?.(path);
        if (!cancelled && result) {
          setEntries(result.entries || []);
          setTruncated(result.truncated || false);
        }
      } catch (err) {
        if (!cancelled) setError((err as Error).message);
      }
    })();
    return () => { cancelled = true; };
  }, [path]);

  if (error) {
    return (
      <div style={{ color: 'var(--text-faint)', fontSize: 12, padding: 20, textAlign: 'center' }}>
        {error}
      </div>
    );
  }

  if (!entries) {
    return (
      <div style={{ color: 'var(--text-faint)', fontSize: 12, padding: 20, textAlign: 'center' }}>
        {t(locale, 'common.loading')}
      </div>
    );
  }

  if (entries.length === 0) {
    return (
      <div style={{ color: 'var(--text-faint)', fontSize: 12, padding: 20, textAlign: 'center' }}>
        Empty archive
      </div>
    );
  }

  return (
    <div style={{ fontSize: 12 }}>
      <div style={{
        padding: '6px 10px',
        color: 'var(--text-dim)',
        borderBottom: '1px solid var(--vibe-btn-border)',
        marginBottom: 4,
      }}>
        {entries.length} entries{truncated ? ' (showing first 1000)' : ''}
      </div>
      <div style={{ overflow: 'auto', maxHeight: 'calc(100vh - 200px)' }}>
        {entries.map((entry, i) => (
          <div key={i} style={{
            display: 'flex',
            alignItems: 'center',
            gap: 8,
            padding: '3px 10px',
            borderBottom: '1px solid var(--vibe-btn-border)',
          }}>
            <span style={{ color: entry.isDir ? 'var(--accent)' : 'var(--text-dim)', fontSize: 14, display: 'inline-flex', alignItems: 'center', justifyContent: 'center' }}>
              {entry.isDir ? <Folder size={14} /> : <FileText size={14} />}
            </span>
            <span style={{
              flex: 1,
              color: 'var(--vibe-brand-text)',
              fontFamily: 'var(--font-mono, monospace)',
              overflow: 'hidden',
              textOverflow: 'ellipsis',
              whiteSpace: 'nowrap',
            }}>
              {entry.name}
            </span>
            {!entry.isDir && entry.size > 0 && (
              <span style={{ color: 'var(--text-faint)', whiteSpace: 'nowrap' }}>
                {formatSize(entry.size)}
              </span>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}
