'use client';

import { useMemo } from 'react';
import { useLocale, t as tr } from '@/i18n';
import { Folder, FolderOpen, Plus } from 'lucide-react';

interface Folder { id: string; name: string; parentId: string | null; sortOrder: number; createdAt: string; updatedAt: string; }

export function FolderTree({
  folders, selectedId, onSelect, onCreate,
}: {
  folders: Folder[]; selectedId: string | null; onSelect: (id: string | null) => void; onCreate: () => void;
}) {
  const locale = useLocale();
  const t = (key: string) => tr(locale, key);

  // Build tree structure
  const tree = useMemo(() => {
    const root = folders.filter(f => !f.parentId);
    const childrenOf = (parentId: string) => folders.filter(f => f.parentId === parentId);
    return root;
  }, [folders]);

  return (
    <div className="flex flex-col h-full">
      <div className="flex items-center justify-between border-b px-3 py-2" style={{ borderColor: 'var(--border)' }}>
        <span className="text-xs font-semibold uppercase tracking-wider" style={{ color: 'var(--text-secondary)' }}>{t('library.folders')}</span>
        <button
          onClick={onCreate}
          className="inline-flex items-center gap-1 text-xs"
          style={{ color: 'var(--accent)' }}
          title={t('library.createFolder')}
        >
          <Plus size={12} /> {t('library.new')}
        </button>
      </div>
      <div className="flex-1 overflow-auto p-1 space-y-0.5">
        <button
          onClick={() => onSelect(null)}
          className="flex w-full items-center gap-1.5 rounded px-2 py-1.5 text-left text-sm"
          style={{
            background: !selectedId ? 'var(--accent-soft)' : 'transparent',
            color: !selectedId ? 'var(--accent)' : 'var(--text)',
          }}
        >
          <FolderOpen size={14} /> {t('library.allItems')}
        </button>
        {tree.map((folder) => (
          <FolderNode key={folder.id} folder={folder} selectedId={selectedId} onSelect={onSelect} depth={0} />
        ))}
        {folders.length === 0 && (
          <p className="px-2 pt-2 text-xs" style={{ color: 'var(--text-disabled)' }}>{t('library.noFolders')}</p>
        )}
      </div>
    </div>
  );
}

function FolderNode({ folder, selectedId, onSelect, depth }: {
  folder: Folder; selectedId: string | null; onSelect: (id: string | null) => void; depth: number;
}) {
  return (
    <button
      onClick={() => onSelect(folder.id)}
      className="flex w-full items-center gap-1.5 rounded px-2 py-1.5 text-left text-sm"
      style={{
        paddingLeft: `${8 + depth * 16}px`,
        background: selectedId === folder.id ? 'var(--accent-soft)' : 'transparent',
        color: selectedId === folder.id ? 'var(--accent)' : 'var(--text)',
      }}
    >
      <Folder size={14} /> {folder.name}
    </button>
  );
}
