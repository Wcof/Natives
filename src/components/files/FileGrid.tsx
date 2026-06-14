'use client';

import { type FileEntry } from '@/types/file';
import FileCard from './FileCard';

interface FileGridProps {
  entries: FileEntry[];
  onSelect: (entry: FileEntry) => void;
  onContextMenu?: (e: React.MouseEvent, entry: FileEntry) => void;
}

export default function FileGrid({ entries, onSelect, onContextMenu }: FileGridProps) {
  if (entries.length === 0) {
    return (
      <div style={{
        display: 'flex', alignItems: 'center', justifyContent: 'center',
        height: '100%', color: 'var(--text-faint, #62655a)', fontSize: 13,
      }}>
        This folder is empty
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
