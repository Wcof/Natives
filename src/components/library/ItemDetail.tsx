'use client';

import { useState } from 'react';
import { useLocale, t as tr } from '@/i18n';
import { TagBadge, TagPicker } from './TagPicker';
import { Bookmark, Code2, ExternalLink, File, Image, Link, NotebookText, X } from 'lucide-react';
import { useToast } from '@/components/ui/Toast';
import { classifyError } from '@/lib/error-classifier';
import {
  ITEM_STATUSES,
  libraryApiOrNull,
  type LibraryFolder,
  type LibraryItem,
  type LibraryTag,
} from '@/lib/library-api';

/**
 * 条目详情/编辑面板。
 * 调用方必须用 key={item.id} 挂载，切换条目时整体重建，避免编辑态残留
 * 导致 A 条目的草稿覆盖 B 条目（数据破坏）。
 */
export function ItemDetail({ item, folders, tags, onClose, onDelete, onSaved }: {
  item: LibraryItem; folders: LibraryFolder[]; tags: LibraryTag[];
  onClose: () => void; onDelete: (id: string) => void; onSaved: () => void;
}) {
  const locale = useLocale();
  const t = (key: string) => tr(locale, key);
  const { toast } = useToast();

  const [editing, setEditing] = useState(false);
  const [title, setTitle] = useState(item.title);
  const [description, setDescription] = useState(item.description);
  const [content, setContent] = useState(item.content);
  const [sourceUrl, setSourceUrl] = useState(item.sourceUrl);
  const [status, setStatus] = useState(item.status);
  const [folderId, setFolderId] = useState(item.folderId);
  const [selectedTagIds, setSelectedTagIds] = useState(item.tags.map(tag => tag.id));
  const [saving, setSaving] = useState(false);

  const handleSave = async () => {
    const api = libraryApiOrNull();
    if (!api) {
      toast(classifyError(new Error('Library API unavailable')).userMessage, 'error');
      return;
    }
    setSaving(true);
    try {
      // 全字段显式下发（folderId null = 移出文件夹），后端做部分合并
      await api.updateItem({
        id: item.id,
        title: title.trim(),
        description,
        content,
        sourceUrl: sourceUrl.trim(),
        status,
        folderId: folderId ?? null,
        tagIds: selectedTagIds,
      });
      setEditing(false);
      onSaved();
    } catch (err) {
      toast(classifyError(err).userMessage, 'error');
    } finally { setSaving(false); }
  };

  const cancelEdit = () => {
    setEditing(false);
    setTitle(item.title);
    setDescription(item.description);
    setContent(item.content);
    setSourceUrl(item.sourceUrl);
    setStatus(item.status);
    setFolderId(item.folderId);
    setSelectedTagIds(item.tags.map(tag => tag.id));
  };

  const formatDate = (dateStr: string) => {
    try { return new Date(dateStr).toLocaleString(locale === 'zh' ? 'zh-CN' : 'en-US'); } catch { return dateStr; }
  };

  const typeLabel = (value: string) => {
    const label = t(`library.itemType.${value}`);
    return label.startsWith('library.') ? value : label;
  };
  const statusLabel = (value: string) => {
    const label = t(`library.statusValue.${value}`);
    return label.startsWith('library.') ? value : label;
  };

  const TypeIcon = ({
    note: NotebookText,
    link: Link,
    file: File,
    code: Code2,
    image: Image,
    bookmark: Bookmark,
  } as const)[item.itemType as 'note' | 'link' | 'file' | 'code' | 'image' | 'bookmark'] ?? File;

  const inputStyle = { borderColor: 'var(--border)', background: 'var(--surface)', color: 'var(--text)' } as const;

  return (
    <div className="p-4">
      {/* Header */}
      <div className="mb-3 flex items-center justify-between">
        <span style={{ color: 'var(--accent)' }} title={typeLabel(item.itemType)}><TypeIcon size={18} /></span>
        <div className="flex gap-1">
          <button
            type="button"
            onClick={() => (editing ? cancelEdit() : setEditing(true))}
            className="rounded px-2 py-1 text-xs"
            style={{ color: 'var(--accent)' }}
          >
            {editing ? t('common.cancel') : t('common.edit')}
          </button>
          <button
            type="button"
            onClick={() => onDelete(item.id)}
            className="rounded px-2 py-1 text-xs"
            style={{ color: 'var(--danger)' }}
          >
            {t('common.delete')}
          </button>
          <button
            type="button"
            onClick={onClose}
            className="rounded px-2 py-1 text-xs"
            style={{ color: 'var(--text-secondary)' }}
            title={t('common.close')}
            aria-label={t('common.close')}
          >
            <X size={14} />
          </button>
        </div>
      </div>

      {editing ? (
        /* Edit mode */
        <div className="space-y-3">
          <input
            value={title}
            onChange={(e) => setTitle(e.target.value)}
            aria-label={t('library.title')}
            placeholder={t('library.title')}
            className="w-full rounded border px-3 py-2 text-sm font-medium"
            style={inputStyle}
          />
          <textarea
            value={description}
            onChange={(e) => setDescription(e.target.value)}
            aria-label={t('library.description')}
            placeholder={t('library.description')}
            className="h-16 w-full resize-none rounded border px-3 py-2 text-sm"
            style={inputStyle}
          />
          <textarea
            value={content}
            onChange={(e) => setContent(e.target.value)}
            aria-label={t('library.content')}
            placeholder={t('library.content')}
            className="h-28 w-full resize-none rounded border px-3 py-2 font-mono text-xs"
            style={inputStyle}
          />
          <input
            value={sourceUrl}
            onChange={(e) => setSourceUrl(e.target.value)}
            aria-label={t('library.sourceUrl')}
            placeholder={t('library.sourceUrl')}
            className="w-full rounded border px-3 py-2 text-sm"
            style={inputStyle}
          />
          <select
            value={folderId ?? ''}
            onChange={(e) => setFolderId(e.target.value || null)}
            aria-label={t('library.folders')}
            className="w-full rounded border px-3 py-2 text-sm"
            style={inputStyle}
          >
            <option value="">{t('library.noFolder')}</option>
            {folders.map(f => <option key={f.id} value={f.id}>{f.name}</option>)}
          </select>
          <select
            value={status}
            onChange={(e) => setStatus(e.target.value)}
            aria-label={t('library.status')}
            className="w-full rounded border px-3 py-2 text-sm"
            style={inputStyle}
          >
            {ITEM_STATUSES.map((s) => <option key={s} value={s}>{statusLabel(s)}</option>)}
          </select>
          <div>
            <p className="mb-1 text-xs" style={{ color: 'var(--text-secondary)' }}>{t('library.tags')}</p>
            <TagPicker tags={tags} selectedIds={selectedTagIds} onChange={setSelectedTagIds} />
          </div>
          <button
            type="button"
            onClick={handleSave}
            disabled={!title.trim() || saving}
            className="w-full rounded px-4 py-2 text-sm font-medium disabled:opacity-50"
            style={{ background: 'var(--primary)', color: 'var(--primary-foreground, var(--accent-ink))' }}
          >
            {saving ? t('common.saving') : t('common.save')}
          </button>
        </div>
      ) : (
        /* View mode */
        <div className="space-y-3">
          <h3 className="text-base font-semibold" style={{ color: 'var(--text)' }}>{item.title || t('common.untitled')}</h3>
          {item.description && <p className="text-sm" style={{ color: 'var(--text-secondary)' }}>{item.description}</p>}
          {item.content && (
            <div
              className="max-h-40 overflow-auto whitespace-pre-wrap rounded p-3 font-mono text-xs"
              style={{ background: 'var(--surface)', color: 'var(--text)' }}
            >
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
          <div className="space-y-0.5 border-t pt-2 text-xs" style={{ borderColor: 'var(--border)', color: 'var(--text-disabled)' }}>
            <div>{t('library.type')}: {typeLabel(item.itemType)}</div>
            <div>{t('library.status')}: {statusLabel(item.status)}</div>
            <div>{t('library.created')}: {formatDate(item.createdAt)}</div>
            <div>{t('library.updated')}: {formatDate(item.updatedAt)}</div>
          </div>
        </div>
      )}
    </div>
  );
}
