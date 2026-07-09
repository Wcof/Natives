'use client';

import { useCallback, useEffect, useState } from 'react';
import { FolderTree } from './FolderTree';
import { TagPicker } from './TagPicker';
import { ItemList } from './ItemList';
import { ItemDetail } from './ItemDetail';
import { SearchFilterBar } from './SearchFilterBar';
import { BatchToolbar } from './BatchToolbar';
import { StatsPanel } from './StatsPanel';
import ConfirmDialog from '@/components/ui/ConfirmDialog';
import { EmptyState, ErrorState } from '@/components/ui/EmptyState';
import { MathCurveLoader } from '@/components/ui/MathCurveLoader';
import { useLocale, t as tr } from '@/i18n';
import { classifyError } from '@/lib/error-classifier';
import { useToast } from '@/components/ui/Toast';

interface Folder { id: string; name: string; parentId: string | null; sortOrder: number; createdAt: string; updatedAt: string; }
interface Tag { id: string; name: string; color: string; createdAt: string; }
interface LibraryItem { id: string; folderId: string | null; title: string; description: string; content: string; sourceUrl: string; itemType: string; status: string; createdAt: string; updatedAt: string; tags: Tag[]; }
interface LibraryStats { totalItems: number; totalFolders: number; totalTags: number; itemsByFolder: { folderId: string | null; folderName: string; count: number }[]; recentItems: number; }

type ViewMode = 'list' | 'stats';

export function LibraryPage() {
  const locale = useLocale();
  const t = (key: string, params?: Record<string, string | number>) => tr(locale, key, params);
  const { toast } = useToast();

  const [folders, setFolders] = useState<Folder[]>([]);
  const [tags, setTags] = useState<Tag[]>([]);
  const [items, setItems] = useState<LibraryItem[]>([]);
  const [stats, setStats] = useState<LibraryStats | null>(null);
  const [selectedItem, setSelectedItem] = useState<LibraryItem | null>(null);
  const [viewMode, setViewMode] = useState<ViewMode>('list');

  const [selectedFolderId, setSelectedFolderId] = useState<string | null>(null);
  const [selectedTagId, setSelectedTagId] = useState<string | null>(null);
  const [keyword, setKeyword] = useState('');
  const [selectedIds, setSelectedIds] = useState<string[]>([]);

  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<Error | null>(null);
  const [confirmDelete, setConfirmDelete] = useState<{ type: 'item' | 'batch'; id?: string } | null>(null);
  const [showCreateItem, setShowCreateItem] = useState(false);
  const [showCreateFolder, setShowCreateFolder] = useState(false);

  const api = typeof window !== 'undefined' ? window.nativesAPI : undefined;

  const loadData = useCallback(async () => {
    if (!api?.library) {
      setError(new Error('Library API unavailable'));
      setLoading(false);
      return;
    }
    setLoading(true);
    setError(null);
    try {
      const [folderRes, tagRes, itemRes, statsRes] = await Promise.all([
        api.library.listFolders(),
        api.library.listTags(),
        api.library.listItems({
          folderId: selectedFolderId ?? undefined,
          tagId: selectedTagId ?? undefined,
          keyword: keyword || undefined,
        }),
        api.library.getStats(),
      ]);
      setFolders(folderRes as Folder[]);
      setTags(tagRes as Tag[]);
      setItems(itemRes as LibraryItem[]);
      setStats(statsRes as LibraryStats);
    } catch (e) {
      const classified = classifyError(e);
      setError(new Error(classified.userMessage));
    } finally {
      setLoading(false);
    }
  }, [api, selectedFolderId, selectedTagId, keyword]);

  useEffect(() => { loadData(); }, [loadData]);

  const handleConfirmDelete = async () => {
    if (!confirmDelete) return;
    try {
      if (confirmDelete.type === 'batch') {
        await api!.library!.batchDelete({ itemIds: selectedIds });
        setSelectedIds([]);
        toast(t('library.deleteBatchSuccess', { count: selectedIds.length }), 'success');
      } else if (confirmDelete.id) {
        await api!.library!.deleteItem(confirmDelete.id);
        if (selectedItem?.id === confirmDelete.id) setSelectedItem(null);
        toast(t('library.deleteSuccess'), 'success');
      }
      setConfirmDelete(null);
      loadData();
    } catch (e) {
      const classified = classifyError(e);
      toast(classified.userMessage, 'error');
    }
  };

  const handleBatchTag = async (tagIds: string[]) => {
    try {
      await api!.library!.batchTag({ itemIds: selectedIds, tagIds });
      toast(t('library.batchTagSuccess'), 'success');
      loadData();
    } catch (e) {
      const classified = classifyError(e);
      toast(classified.userMessage, 'error');
    }
  };

  const handleBatchMove = async (folderId: string | null) => {
    try {
      await api!.library!.batchMove({ itemIds: selectedIds, folderId: folderId ?? undefined });
      toast(t('library.batchMoveSuccess'), 'success');
      loadData();
    } catch (e) {
      const classified = classifyError(e);
      toast(classified.userMessage, 'error');
    }
  };

  if (loading) {
    return <div className="flex-1 flex items-center justify-center"><MathCurveLoader /></div>;
  }

  if (error) {
    return <ErrorState message={error.message} onRetry={loadData} />;
  }

  return (
    <div className="flex h-full overflow-hidden">
      {/* Left sidebar: folders */}
      <div className="w-56 border-r" style={{ borderColor: 'var(--border)' }}>
        <FolderTree
          folders={folders}
          selectedId={selectedFolderId}
          onSelect={setSelectedFolderId}
          onCreate={() => setShowCreateFolder(true)}
        />
      </div>

      {/* Center: items list */}
      <div className="flex-1 flex flex-col overflow-hidden">
        <SearchFilterBar
          keyword={keyword}
          onKeywordChange={setKeyword}
          viewMode={viewMode}
          onViewModeChange={setViewMode}
          tags={tags}
          selectedTagId={selectedTagId}
          onTagChange={setSelectedTagId}
          onCreateItem={() => setShowCreateItem(true)}
        />

        {selectedIds.length > 0 && (
          <BatchToolbar
            selectedCount={selectedIds.length}
            tags={tags}
            folders={folders}
            onTag={handleBatchTag}
            onMove={handleBatchMove}
            onDelete={() => setConfirmDelete({ type: 'batch' })}
            onClear={() => setSelectedIds([])}
          />
        )}

        {viewMode === 'list' ? (
          <div className="flex-1 overflow-auto">
            {items.length === 0 ? (
              <EmptyState
                title={t('library.empty')}
                description={t('library.emptyDesc')}
                action={{ label: t('library.createItem'), onClick: () => setShowCreateItem(true) }}
              />
            ) : (
              <ItemList
                items={items}
                selectedIds={selectedIds}
                onSelectionChange={setSelectedIds}
                onSelectItem={setSelectedItem}
                selectedItemId={selectedItem?.id ?? null}
              />
            )}
          </div>
        ) : (
          <div className="flex-1 overflow-auto p-4">
            <StatsPanel stats={stats} />
          </div>
        )}
      </div>

      {/* Right: item detail */}
      {selectedItem && (
        <div className="w-80 border-l overflow-auto" style={{ borderColor: 'var(--border)' }}>
          <ItemDetail
            item={selectedItem}
            folders={folders}
            tags={tags}
            onClose={() => setSelectedItem(null)}
            onDelete={(id) => setConfirmDelete({ type: 'item', id })}
            onSaved={loadData}
          />
        </div>
      )}

      {/* ConfirmDialog for delete */}
      <ConfirmDialog
        open={confirmDelete !== null}
        title={t('library.deleteConfirm')}
        message={confirmDelete?.type === 'batch' ? t('library.deleteBatchMessage', { count: selectedIds.length }) : t('library.deleteMessage')}
        confirmLabel={t('common.delete')}
        cancelLabel={t('common.cancel')}
        danger
        onConfirm={handleConfirmDelete}
        onCancel={() => setConfirmDelete(null)}
      />

      {/* Create item dialog */}
      {showCreateItem && (
        <CreateItemDialog
          folders={folders}
          tags={tags}
          locale={locale}
          onClose={() => setShowCreateItem(false)}
          onSaved={() => { setShowCreateItem(false); loadData(); }}
        />
      )}

      {/* Create folder dialog */}
      {showCreateFolder && (
        <CreateFolderDialog
          locale={locale}
          onClose={() => setShowCreateFolder(false)}
          onSaved={() => { setShowCreateFolder(false); loadData(); }}
        />
      )}
    </div>
  );
}

// ── Sub-dialogs ──

function CreateItemDialog({ folders, tags, locale, onClose, onSaved }: {
  folders: Folder[]; tags: Tag[]; locale: string; onClose: () => void; onSaved: () => void;
}) {
  const t = (key: string) => tr(locale, key);
  const { toast } = useToast();
  const [title, setTitle] = useState('');
  const [description, setDescription] = useState('');
  const [folderId, setFolderId] = useState<string | undefined>(undefined);
  const [selectedTagIds, setSelectedTagIds] = useState<string[]>([]);
  const [saving, setSaving] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const api = typeof window !== 'undefined' ? window.nativesAPI : undefined;

  const handleSave = async () => {
    if (!title.trim()) return;
    if (!api?.library?.createItem) {
      const classified = classifyError(new Error('Library API unavailable'));
      setErr(classified.userMessage);
      return;
    }
    setSaving(true);
    setErr(null);
    try {
      await api.library.createItem({ folderId, title: title.trim(), description, tagIds: selectedTagIds });
      toast(t('library.createSuccess'), 'success');
      onSaved();
    } catch (e) {
      const classified = classifyError(e);
      setErr(classified.userMessage);
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center" style={{ backgroundColor: 'rgba(0,0,0,0.4)' }}>
      <div className="rounded-lg p-6 w-96 max-h-[80vh] overflow-auto shadow-xl"
        style={{ background: 'var(--surface)', borderColor: 'var(--border)', borderWidth: 1 }}>
        <h2 className="text-lg font-semibold mb-4" style={{ color: 'var(--text)' }}>{t('library.createItem')}</h2>
        <div className="space-y-3">
          <input placeholder={t('library.title')} value={title} onChange={(e) => setTitle(e.target.value)}
            className="w-full px-3 py-2 border rounded" autoFocus
            style={{ borderColor: 'var(--border)', background: 'var(--surface)', color: 'var(--text)' }} />
          <textarea placeholder={t('library.description')} value={description} onChange={(e) => setDescription(e.target.value)}
            className="w-full px-3 py-2 border rounded h-20 resize-none"
            style={{ borderColor: 'var(--border)', background: 'var(--surface)', color: 'var(--text)' }} />
          <select value={folderId ?? ''} onChange={(e) => setFolderId(e.target.value || undefined)}
            className="w-full px-3 py-2 border rounded"
            style={{ borderColor: 'var(--border)', background: 'var(--surface)', color: 'var(--text)' }}>
            <option value="">{t('library.noFolder')}</option>
            {folders.map((f) => <option key={f.id} value={f.id}>{f.name}</option>)}
          </select>
          <div>
            <p className="text-sm mb-1" style={{ color: 'var(--text-secondary)' }}>{t('library.tags')}</p>
            <div className="flex flex-wrap gap-1">
              {tags.map((tag) => (
                <button key={tag.id} onClick={() => setSelectedTagIds(prev =>
                  prev.includes(tag.id) ? prev.filter(id => id !== tag.id) : [...prev, tag.id])}
                  className="px-2 py-0.5 rounded text-xs border"
                  style={{
                    borderColor: selectedTagIds.includes(tag.id) ? 'var(--primary)' : 'var(--border)',
                    borderLeftColor: tag.color, borderLeftWidth: 3,
                    background: selectedTagIds.includes(tag.id) ? 'var(--surface-hover)' : 'transparent',
                  }}>
                  {tag.name}
                </button>
              ))}
            </div>
          </div>
          {err && <p className="text-sm" style={{ color: 'var(--danger)' }}>{err}</p>}
        </div>
        <div className="flex justify-end gap-2 mt-4">
          <button onClick={onClose} className="px-4 py-2 text-sm" disabled={saving}
            style={{ color: 'var(--text-secondary)' }}>{t('common.cancel')}</button>
          <button onClick={handleSave} disabled={!title.trim() || saving}
            className="px-4 py-2 text-sm rounded disabled:opacity-50"
            style={{ background: 'var(--primary)', color: '#fff' }}>
            {saving ? t('common.saving') : t('common.save')}
          </button>
        </div>
      </div>
    </div>
  );
}

function CreateFolderDialog({ locale, onClose, onSaved }: { locale: string; onClose: () => void; onSaved: () => void }) {
  const t = (key: string) => tr(locale, key);
  const { toast } = useToast();
  const [name, setName] = useState('');
  const [saving, setSaving] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const api = typeof window !== 'undefined' ? window.nativesAPI : undefined;

  const handleSave = async () => {
    if (!name.trim()) return;
    if (!api?.library?.createFolder) {
      const classified = classifyError(new Error('Library API unavailable'));
      setErr(classified.userMessage);
      return;
    }
    setSaving(true);
    setErr(null);
    try {
      await api.library.createFolder({ name: name.trim() });
      toast(t('library.createFolderSuccess'), 'success');
      onSaved();
    } catch (e) {
      const classified = classifyError(e);
      setErr(classified.userMessage);
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center" style={{ backgroundColor: 'rgba(0,0,0,0.4)' }}>
      <div className="rounded-lg p-6 w-80 shadow-xl"
        style={{ background: 'var(--surface)', borderColor: 'var(--border)', borderWidth: 1 }}>
        <h2 className="text-lg font-semibold mb-4" style={{ color: 'var(--text)' }}>{t('library.createFolder')}</h2>
        <input placeholder={t('library.folderName')} value={name} onChange={(e) => setName(e.target.value)}
          className="w-full px-3 py-2 border rounded" autoFocus
          style={{ borderColor: 'var(--border)', background: 'var(--surface)', color: 'var(--text)' }}
          onKeyDown={(e) => e.key === 'Enter' && handleSave()} />
        {err && <p className="text-sm mt-2" style={{ color: 'var(--danger)' }}>{err}</p>}
        <div className="flex justify-end gap-2 mt-4">
          <button onClick={onClose} className="px-4 py-2 text-sm" disabled={saving}
            style={{ color: 'var(--text-secondary)' }}>{t('common.cancel')}</button>
          <button onClick={handleSave} disabled={!name.trim() || saving}
            className="px-4 py-2 text-sm rounded disabled:opacity-50"
            style={{ background: 'var(--primary)', color: '#fff' }}>
            {saving ? t('common.saving') : t('common.save')}
          </button>
        </div>
      </div>
    </div>
  );
}
