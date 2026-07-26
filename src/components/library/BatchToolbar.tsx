'use client';

import { useEffect, useRef, useState } from 'react';
import { useLocale, t as tr } from '@/i18n';
import { FolderInput, Tags, Trash2, X } from 'lucide-react';
import type { LibraryFolder, LibraryTag } from '@/lib/library-api';
import { TagPicker } from './TagPicker';

export function BatchToolbar({
  selectedCount, tags, folders,
  onTag, onMove, onDelete, onClear,
}: {
  selectedCount: number; tags: LibraryTag[]; folders: LibraryFolder[];
  onTag: (tagIds: string[]) => void; onMove: (folderId: string | null) => void;
  onDelete: () => void; onClear: () => void;
}) {
  const locale = useLocale();
  const t = (key: string) => tr(locale, key);
  const [openPicker, setOpenPicker] = useState<'tag' | 'folder' | null>(null);
  const [selectedTagIds, setSelectedTagIds] = useState<string[]>([]);
  const rootRef = useRef<HTMLDivElement>(null);

  // 弹层：点击外部 / Esc 关闭
  useEffect(() => {
    if (!openPicker) return;
    const onPointerDown = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) setOpenPicker(null);
    };
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setOpenPicker(null);
    };
    document.addEventListener('mousedown', onPointerDown);
    document.addEventListener('keydown', onKeyDown);
    return () => {
      document.removeEventListener('mousedown', onPointerDown);
      document.removeEventListener('keydown', onKeyDown);
    };
  }, [openPicker]);

  return (
    <div
      ref={rootRef}
      className="flex items-center gap-2 border-b px-3 py-1.5 text-xs"
      style={{
        background: 'var(--accent-soft)',
        borderColor: 'var(--border)',
        color: 'var(--text)',
      }}
    >
      <span className="font-medium" style={{ color: 'var(--accent)' }}>{selectedCount} {t('library.selected')}</span>

      <div className="relative ml-2 flex gap-1">
        {/* Tag button */}
        <div className="relative">
          <button
            type="button"
            onClick={() => setOpenPicker(openPicker === 'tag' ? null : 'tag')}
            aria-expanded={openPicker === 'tag'}
            className="inline-flex items-center gap-1 rounded border px-2 py-1"
            style={{ background: 'var(--surface)', borderColor: 'var(--border)', color: 'var(--text)' }}>
            <Tags size={13} /> {t('library.tag')}
          </button>
          {openPicker === 'tag' && (
            <div className="absolute left-0 top-full z-10 mt-1 w-48 rounded border p-2 shadow-lg" style={{ background: 'var(--panel, var(--surface))', borderColor: 'var(--border)' }}>
              <p className="mb-1 text-xs" style={{ color: 'var(--text-secondary)' }}>{t('library.selectTags')}</p>
              {tags.length === 0 ? (
                <p className="mb-2 text-xs" style={{ color: 'var(--text-disabled)' }}>{t('library.noTags')}</p>
              ) : (
                <div className="mb-2">
                  <TagPicker tags={tags} selectedIds={selectedTagIds} onChange={setSelectedTagIds} />
                </div>
              )}
              <button
                type="button"
                disabled={selectedTagIds.length === 0}
                onClick={() => { onTag(selectedTagIds); setSelectedTagIds([]); setOpenPicker(null); }}
                className="w-full rounded px-2 py-1 text-xs font-medium disabled:opacity-50"
                style={{ background: 'var(--primary)', color: 'var(--primary-foreground, var(--accent-ink))' }}>
                {t('common.apply')}
              </button>
            </div>
          )}
        </div>

        {/* Move button */}
        <div className="relative">
          <button
            type="button"
            onClick={() => setOpenPicker(openPicker === 'folder' ? null : 'folder')}
            aria-expanded={openPicker === 'folder'}
            className="inline-flex items-center gap-1 rounded border px-2 py-1"
            style={{ background: 'var(--surface)', borderColor: 'var(--border)', color: 'var(--text)' }}>
            <FolderInput size={13} /> {t('library.move')}
          </button>
          {openPicker === 'folder' && (
            <div className="absolute left-0 top-full z-10 mt-1 max-h-64 w-48 overflow-auto rounded border p-2 shadow-lg" style={{ background: 'var(--panel, var(--surface))', borderColor: 'var(--border)' }}>
              <button
                type="button"
                onClick={() => { onMove(null); setOpenPicker(null); }}
                className="w-full rounded px-2 py-1 text-left text-xs"
                style={{ color: 'var(--text)' }}>
                {t('library.noFolder')}
              </button>
              {folders.map(f => (
                <button
                  type="button"
                  key={f.id}
                  onClick={() => { onMove(f.id); setOpenPicker(null); }}
                  className="flex w-full items-center gap-1 rounded px-2 py-1 text-left text-xs"
                  style={{ color: 'var(--text)' }}>
                  <FolderInput size={12} /> {f.name}
                </button>
              ))}
            </div>
          )}
        </div>

        {/* Delete button */}
        <button
          type="button"
          onClick={onDelete}
          className="inline-flex items-center gap-1 rounded border px-2 py-1"
          style={{ background: 'var(--danger-soft)', borderColor: 'var(--danger)', color: 'var(--danger)' }}>
          <Trash2 size={13} /> {t('common.delete')}
        </button>

        {/* Clear button */}
        <button
          type="button"
          onClick={onClear}
          className="inline-flex items-center gap-1 px-2 py-1"
          style={{ color: 'var(--text-secondary)' }}>
          <X size={13} /> {t('common.clear')}
        </button>
      </div>
    </div>
  );
}
