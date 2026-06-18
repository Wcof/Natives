'use client';

import { type FileEntry } from '@/types/file';
import { FolderOpen } from 'lucide-react';
import { t, useLocale } from '@/i18n';
import FileCard from './FileCard';
import { FONT_SIZE, SPACING } from '@/lib/design-tokens';

interface FileGridProps {
  entries: FileEntry[];
  onSelect: (entry: FileEntry) => void;
  onContextMenu?: (e: React.MouseEvent, entry: FileEntry) => void;
}

export default function FileGrid({ entries, onSelect, onContextMenu }: FileGridProps) {
  const locale = useLocale();

  if (entries.length === 0) {
    return (
      <div style={{
        display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center',
        height: '100%', color: 'var(--vibe-btn-text)', fontSize: FONT_SIZE.lg, gap: SPACING.sm,
      }}>
        <FolderOpen size={24} style={{ color: 'var(--vibe-btn-text)' }} />
        {t(locale, 'fileBrowser.empty')}
      </div>
    );
  }

  return (
    <div style={{
      display: 'grid',
      gridTemplateColumns: 'repeat(auto-fill, minmax(140px, 1fr))',
      gap: 10,
      padding: SPACING.md,
    }}>
      {entries.map((entry) => (
        <FileCard
          key={entry.path}
          entry={entry}
          onSelect={onSelect}
          onContextMenu={onContextMenu}
        />
      ))}
    </div>
  );
}
