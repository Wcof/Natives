'use client';

import { useMemo, useState } from 'react';
import { useLocale, t as tr } from '@/i18n';
import { Folder as FolderIcon, FolderOpen, Pencil, Plus, Trash2, Check, X } from 'lucide-react';
import type { LibraryFolder } from '@/lib/library-api';

/**
 * 文件夹树 — 支持嵌套渲染（parentId 递归）、悬停重命名/删除。
 * 注意：这是 Library 域的逻辑文件夹，与文件浏览器的目录树无关。
 */
export function FolderTree({
  folders, selectedId, onSelect, onCreate, onRename, onDelete,
}: {
  folders: LibraryFolder[];
  selectedId: string | null;
  onSelect: (id: string | null) => void;
  onCreate: () => void;
  onRename: (id: string, name: string) => void;
  onDelete: (folder: LibraryFolder) => void;
}) {
  const locale = useLocale();
  const t = (key: string) => tr(locale, key);

  const childrenOf = useMemo(() => {
    const map = new Map<string | null, LibraryFolder[]>();
    for (const f of folders) {
      const key = f.parentId && folders.some((p) => p.id === f.parentId) ? f.parentId : null;
      const list = map.get(key) ?? [];
      list.push(f);
      map.set(key, list);
    }
    return map;
  }, [folders]);

  return (
    <div className="flex flex-col h-full">
      <div className="flex items-center justify-between border-b px-3 py-2" style={{ borderColor: 'var(--border)' }}>
        <span className="text-xs font-semibold uppercase tracking-wider" style={{ color: 'var(--text-secondary)' }}>{t('library.folders')}</span>
        <button
          type="button"
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
          type="button"
          onClick={() => onSelect(null)}
          aria-current={!selectedId ? 'true' : undefined}
          className="flex w-full items-center gap-1.5 rounded px-2 py-1.5 text-left text-sm"
          style={{
            background: !selectedId ? 'var(--accent-soft)' : 'transparent',
            color: !selectedId ? 'var(--accent)' : 'var(--text)',
          }}
        >
          <FolderOpen size={14} /> {t('library.allItems')}
        </button>
        {(childrenOf.get(null) ?? []).map((folder) => (
          <FolderNode
            key={folder.id}
            folder={folder}
            childrenOf={childrenOf}
            selectedId={selectedId}
            onSelect={onSelect}
            onRename={onRename}
            onDelete={onDelete}
            depth={0}
          />
        ))}
        {folders.length === 0 && (
          <p className="px-2 pt-2 text-xs" style={{ color: 'var(--text-disabled)' }}>{t('library.noFolders')}</p>
        )}
      </div>
    </div>
  );
}

function FolderNode({ folder, childrenOf, selectedId, onSelect, onRename, onDelete, depth }: {
  folder: LibraryFolder;
  childrenOf: Map<string | null, LibraryFolder[]>;
  selectedId: string | null;
  onSelect: (id: string | null) => void;
  onRename: (id: string, name: string) => void;
  onDelete: (folder: LibraryFolder) => void;
  depth: number;
}) {
  const locale = useLocale();
  const t = (key: string) => tr(locale, key);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(folder.name);
  const children = childrenOf.get(folder.id) ?? [];
  const selected = selectedId === folder.id;

  const commitRename = () => {
    const name = draft.trim();
    setEditing(false);
    if (name && name !== folder.name) onRename(folder.id, name);
    else setDraft(folder.name);
  };

  return (
    <>
      {editing ? (
        <div
          className="flex items-center gap-1 rounded px-2 py-1"
          style={{ paddingLeft: `${8 + depth * 14}px` }}
        >
          <FolderIcon size={14} style={{ color: 'var(--text-secondary)', flexShrink: 0 }} />
          <input
            value={draft}
            autoFocus
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') commitRename();
              if (e.key === 'Escape') { setEditing(false); setDraft(folder.name); }
            }}
            className="min-w-0 flex-1 rounded border px-1 py-0.5 text-sm"
            style={{ borderColor: 'var(--accent)', background: 'var(--surface)', color: 'var(--text)' }}
            aria-label={t('library.renameFolder')}
          />
          <button type="button" onClick={commitRename} title={t('common.save')} style={{ color: 'var(--accent)' }}>
            <Check size={13} />
          </button>
          <button
            type="button"
            onClick={() => { setEditing(false); setDraft(folder.name); }}
            title={t('common.cancel')}
            style={{ color: 'var(--text-secondary)' }}
          >
            <X size={13} />
          </button>
        </div>
      ) : (
        <div
          className="group flex w-full items-center rounded"
          style={{
            background: selected ? 'var(--accent-soft)' : 'transparent',
            color: selected ? 'var(--accent)' : 'var(--text)',
          }}
        >
          <button
            type="button"
            onClick={() => onSelect(folder.id)}
            aria-current={selected ? 'true' : undefined}
            className="flex min-w-0 flex-1 items-center gap-1.5 px-2 py-1.5 text-left text-sm"
            style={{ paddingLeft: `${8 + depth * 14}px` }}
          >
            <FolderIcon size={14} style={{ flexShrink: 0 }} />
            <span className="truncate">{folder.name}</span>
          </button>
          <span className="hidden shrink-0 items-center gap-0.5 pr-1.5 group-hover:flex">
            <button
              type="button"
              onClick={() => { setDraft(folder.name); setEditing(true); }}
              title={t('library.renameFolder')}
              aria-label={t('library.renameFolder')}
              style={{ color: 'var(--text-secondary)' }}
            >
              <Pencil size={12} />
            </button>
            <button
              type="button"
              onClick={() => onDelete(folder)}
              title={t('library.deleteFolder')}
              aria-label={t('library.deleteFolder')}
              style={{ color: 'var(--danger)' }}
            >
              <Trash2 size={12} />
            </button>
          </span>
        </div>
      )}
      {children.map((child) => (
        <FolderNode
          key={child.id}
          folder={child}
          childrenOf={childrenOf}
          selectedId={selectedId}
          onSelect={onSelect}
          onRename={onRename}
          onDelete={onDelete}
          depth={depth + 1}
        />
      ))}
    </>
  );
}
