'use client';

import { useState } from 'react';
import { useLocale, t as tr } from '@/i18n';
import { TagBadge } from './TagPicker';
import { Bookmark, Code2, ExternalLink, File, Image, Link, NotebookText, X } from 'lucide-react';
import { useToast } from '@/components/ui/Toast';
import { classifyError } from '@/lib/error-classifier';

interface Tag { id: string; name: string; color: string; createdAt: string; }
interface Folder { id: string; name: string; parentId: string | null; sortOrder: number; createdAt: string; updatedAt: string; }
interface LibraryItem { id: string; folderId: string | null; title: string; description: string; content: string; sourceUrl: string; itemType: string; status: string; createdAt: string; updatedAt: string; tags: Tag[]; }

export function ItemDetail({ item, folders, tags, onClose, onDelete, onSaved }: {
  item: LibraryItem; folders: Folder[]; tags: Tag[];
  onClose: () => void; onDelete: (id: string) => void; onSaved: () => void;
}) {
  const locale = useLocale();
  const t = (key: string) => tr(locale, key);
  const { toast } = useToast();

  const [editing, setEditing] = useState(false);
  const [title, setTitle] = useState(item.title);
  const [description, setDescription] = useState(item.description);
  const [folderId, setFolderId] = useState(item.folderId);
  const [selectedTagIds, setSelectedTagIds] = useState(item.tags.map(t => t.id));
  const [saving, setSaving] = useState(false);

  const api = typeof window !== 'undefined' ? window.nativesAPI : undefined;

  const handleSave = async () => {
    setSaving(true);
    try {
      await api!.library!.updateItem({ id: item.id, title, description, folderId: folderId ?? undefined, tagIds: selectedTagIds });
      setEditing(false);
      onSaved();
    } catch (err) {
      toast(classifyError(err).userMessage, 'error');
    } finally { setSaving(false); }
  };

  const formatDate = (dateStr: string) => {
    try { return new Date(dateStr).toLocaleString(locale === 'zh' ? 'zh-CN' : 'en-US'); } catch { return dateStr; }
  };

  const toggleTag = (tagId: string) => {
    setSelectedTagIds(prev => prev.includes(tagId) ? prev.filter(id => id !== tagId) : [...prev, tagId]);
  };

  const TypeIcon = ({
    note: NotebookText,
    link: Link,
    file: File,
    code: Code2,
    image: Image,
    bookmark: Bookmark,
  } as const)[item.itemType as 'note' | 'link' | 'file' | 'code' | 'image' | 'bookmark'] ?? File;

  return (
    <div className="p-4">
      {/* Header */}
      <div className="flex items-center justify-between mb-3">
        <span style={{ color: 'var(--accent)' }}><TypeIcon size={18} /></span>
        <div className="flex gap-1">
          <button onClick={() => setEditing(!editing)} className="text-xs text-blue-500 hover:text-blue-600 px-2 py-1 rounded hover:bg-blue-50 dark:hover:bg-blue-900/20">
            {editing ? t('common.cancel') : t('common.edit')}
          </button>
          <button onClick={() => onDelete(item.id)} className="text-xs text-red-500 hover:text-red-600 px-2 py-1 rounded hover:bg-red-50 dark:hover:bg-red-900/20">
            {t('common.delete')}
          </button>
          <button onClick={onClose} className="rounded px-2 py-1 text-xs" style={{ color: 'var(--text-secondary)' }} title={t('common.close')}>
            <X size={14} />
          </button>
        </div>
      </div>

      {editing ? (
        /* Edit mode */
        <div className="space-y-3">
          <input value={title} onChange={(e) => setTitle(e.target.value)}
            className="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded bg-white dark:bg-gray-700 text-sm font-medium" />
          <textarea value={description} onChange={(e) => setDescription(e.target.value)}
            className="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded bg-white dark:bg-gray-700 text-sm h-20 resize-none" />
          <select value={folderId ?? ''} onChange={(e) => setFolderId(e.target.value || null)}
            className="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded bg-white dark:bg-gray-700 text-sm">
            <option value="">{t('library.noFolder')}</option>
            {folders.map(f => <option key={f.id} value={f.id}>{f.name}</option>)}
          </select>
          <div>
            <p className="text-xs text-gray-500 mb-1">{t('library.tags')}</p>
            <div className="flex flex-wrap gap-1">
              {tags.map(tag => (
                <button key={tag.id} onClick={() => toggleTag(tag.id)}
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
          </div>
          <button onClick={handleSave} disabled={!title.trim() || saving}
            className="w-full rounded px-4 py-2 text-sm font-medium disabled:opacity-50"
            style={{ background: 'var(--primary)', color: 'var(--primary-foreground, #fff)' }}>
            {saving ? t('common.saving') : t('common.save')}
          </button>
        </div>
      ) : (
        /* View mode */
        <div className="space-y-3">
          <h3 className="text-base font-semibold">{item.title || t('common.untitled')}</h3>
          {item.description && <p className="text-sm text-gray-600 dark:text-gray-400">{item.description}</p>}
          {item.content && (
            <div className="text-sm bg-gray-50 dark:bg-gray-900 p-3 rounded max-h-40 overflow-auto whitespace-pre-wrap font-mono text-xs">
              {item.content}
            </div>
          )}
          {item.sourceUrl && (
            <a href={item.sourceUrl} target="_blank" rel="noopener noreferrer"
              className="flex items-center gap-1 truncate text-xs hover:underline"
              style={{ color: 'var(--accent)' }}>
              <ExternalLink size={12} /> {item.sourceUrl}
            </a>
          )}
          <div className="flex flex-wrap gap-1">
            {item.tags.map(tag => <TagBadge key={tag.id} tag={tag} />)}
          </div>
          <div className="text-xs text-gray-400 space-y-0.5 pt-2 border-t border-gray-200 dark:border-gray-700">
            <div>{t('library.type')}: {item.itemType}</div>
            <div>{t('library.status')}: {item.status}</div>
            <div>{t('library.created')}: {formatDate(item.createdAt)}</div>
            <div>{t('library.updated')}: {formatDate(item.updatedAt)}</div>
          </div>
        </div>
      )}
    </div>
  );
}
