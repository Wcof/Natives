'use client';

import { type FileEntry } from '@/types/file';
import { t, useLocale } from '@/i18n';
import FileCard from './FileCard';

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
        height: '100%', color: 'var(--text-faint, #62655a)', fontSize: 13, gap: 8,
      }}>
        <span style={{ fontSize: 28 }}>📂</span>
        {t(locale, 'fileBrowser.empty')}
      </div>
    );
  }

  return (
    <div style={{
      display: 'grid',
      gridTemplateColumns: 'repeat(auto-fill, minmax(140px, 1fr))',
      gap: 10,
      padding: 12,
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
