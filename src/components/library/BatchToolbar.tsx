'use client';

import { useState } from 'react';
import { useLocale, t as tr } from '@/i18n';
import { FolderInput, Tags, Trash2, X } from 'lucide-react';

interface Tag { id: string; name: string; color: string; createdAt: string; }
interface Folder { id: string; name: string; parentId: string | null; sortOrder: number; createdAt: string; updatedAt: string; }

export function BatchToolbar({
  selectedCount, tags, folders,
  onTag, onMove, onDelete, onClear,
}: {
  selectedCount: number; tags: Tag[]; folders: Folder[];
  onTag: (tagIds: string[]) => void; onMove: (folderId: string | null) => void;
  onDelete: () => void; onClear: () => void;
}) {
  const locale = useLocale();
  const t = (key: string) => tr(locale, key);
  const [showTagPicker, setShowTagPicker] = useState(false);
  const [showFolderPicker, setShowFolderPicker] = useState(false);
  const [selectedTagIds, setSelectedTagIds] = useState<string[]>([]);

  return (
    <div
      className="flex items-center gap-2 border-b px-3 py-1.5 text-xs"
      style={{
        background: 'var(--accent-soft)',
        borderColor: 'var(--accent-border)',
        color: 'var(--text)',
      }}
    >
      <span className="font-medium" style={{ color: 'var(--accent)' }}>{selectedCount} {t('library.selected')}</span>

      <div className="flex gap-1 ml-2 relative">
        {/* Tag button */}
        <div className="relative">
          <button onClick={() => { setShowTagPicker(!showTagPicker); setShowFolderPicker(false); }}
            className="inline-flex items-center gap-1 rounded border px-2 py-1"
            style={{ background: 'var(--surface)', borderColor: 'var(--border)', color: 'var(--text)' }}>
            <Tags size={13} /> {t('library.tag')}
          </button>
          {showTagPicker && (
            <div className="absolute top-full left-0 mt-1 z-10 w-48 rounded border p-2 shadow-lg" style={{ background: 'var(--panel)', borderColor: 'var(--border)' }}>
              <p className="mb-1 text-xs" style={{ color: 'var(--text-secondary)' }}>{t('library.selectTags')}</p>
              <div className="flex flex-wrap gap-1 mb-2">
                {tags.map(tag => (
                  <button key={tag.id} onClick={() => setSelectedTagIds(prev =>
                    prev.includes(tag.id) ? prev.filter(id => id !== tag.id) : [...prev, tag.id]
                  )}
                    className="rounded border px-2 py-0.5 text-xs"
                    style={{
                      borderColor: selectedTagIds.includes(tag.id) ? 'var(--accent)' : 'var(--border)',
                      background: selectedTagIds.includes(tag.id) ? 'var(--accent-soft)' : 'var(--surface)',
                      borderLeftColor: tag.color,
                      borderLeftWidth: 3,
                      color: 'var(--text)',
                    }}>
                    {tag.name}
                  </button>
                ))}
              </div>
              <button onClick={() => { onTag(selectedTagIds); setShowTagPicker(false); }}
                className="w-full rounded px-2 py-1 text-xs font-medium"
                style={{ background: 'var(--primary)', color: 'var(--primary-foreground, #fff)' }}>
                {t('common.apply')}
              </button>
            </div>
          )}
        </div>

        {/* Move button */}
        <div className="relative">
          <button onClick={() => { setShowFolderPicker(!showFolderPicker); setShowTagPicker(false); }}
            className="inline-flex items-center gap-1 rounded border px-2 py-1"
            style={{ background: 'var(--surface)', borderColor: 'var(--border)', color: 'var(--text)' }}>
            <FolderInput size={13} /> {t('library.move')}
          </button>
          {showFolderPicker && (
            <div className="absolute top-full left-0 mt-1 z-10 w-48 rounded border p-2 shadow-lg" style={{ background: 'var(--panel)', borderColor: 'var(--border)' }}>
              <button onClick={() => { onMove(null); setShowFolderPicker(false); }}
                className="w-full rounded px-2 py-1 text-left text-xs"
                style={{ color: 'var(--text)' }}>
                {t('library.noFolder')}
              </button>
              {folders.map(f => (
                <button key={f.id} onClick={() => { onMove(f.id); setShowFolderPicker(false); }}
                  className="flex w-full items-center gap-1 rounded px-2 py-1 text-left text-xs"
                  style={{ color: 'var(--text)' }}>
                  <FolderInput size={12} /> {f.name}
                </button>
              ))}
            </div>
          )}
        </div>

        {/* Delete button */}
        <button onClick={onDelete}
          className="inline-flex items-center gap-1 rounded border px-2 py-1"
          style={{ background: 'var(--danger-soft)', borderColor: 'var(--danger)', color: 'var(--danger)' }}>
          <Trash2 size={13} /> {t('common.delete')}
        </button>

        {/* Clear button */}
        <button onClick={onClear}
          className="inline-flex items-center gap-1 px-2 py-1"
          style={{ color: 'var(--text-secondary)' }}>
          <X size={13} /> {t('common.clear')}
        </button>
      </div>
    </div>
  );
}
