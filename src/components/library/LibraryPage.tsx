'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import { FolderTree } from './FolderTree';
import { ItemList } from './ItemList';
import { ItemDetail } from './ItemDetail';
import { SearchFilterBar } from './SearchFilterBar';
import { BatchToolbar } from './BatchToolbar';
import { StatsPanel } from './StatsPanel';
import { TagPicker } from './TagPicker';
import ConfirmDialog from '@/components/ui/ConfirmDialog';
import Modal from '@/components/ui/Modal';
import { EmptyState, ErrorState } from '@/components/ui/EmptyState';
import { MathCurveLoader } from '@/components/ui/MathCurveLoader';
import { useLocale, t as tr } from '@/i18n';
import { classifyError } from '@/lib/error-classifier';
import { useToast } from '@/components/ui/Toast';
import { Trash2 } from 'lucide-react';
import {
  ITEM_TYPES,
  LIBRARY_PAGE_SIZE,
  TAG_COLOR_PRESETS,
  libraryApiOrNull,
  type LibraryFolder,
  type LibraryItem,
  type LibraryStats,
  type LibraryTag,
} from '@/lib/library-api';

type ViewMode = 'list' | 'stats';
type PendingDelete =
  | { type: 'item'; id: string }
  | { type: 'batch' }
  | { type: 'folder'; folder: LibraryFolder }
  | { type: 'tag'; tag: LibraryTag };

const SEARCH_DEBOUNCE_MS = 300;

export function LibraryPage() {
  const locale = useLocale();
  const t = useCallback(
    (key: string, params?: Record<string, string | number>) => tr(locale, key, params),
    [locale],
  );
  const { toast } = useToast();

  const [folders, setFolders] = useState<LibraryFolder[]>([]);
  const [tags, setTags] = useState<LibraryTag[]>([]);
  const [items, setItems] = useState<LibraryItem[]>([]);
  const [hasMore, setHasMore] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);
  const [stats, setStats] = useState<LibraryStats | null>(null);
  const [selectedItem, setSelectedItem] = useState<LibraryItem | null>(null);
  const [viewMode, setViewMode] = useState<ViewMode>('list');

  const [selectedFolderId, setSelectedFolderId] = useState<string | null>(null);
  const [selectedTagId, setSelectedTagId] = useState<string | null>(null);
  const [keyword, setKeyword] = useState('');
  const [debouncedKeyword, setDebouncedKeyword] = useState('');
  const [selectedIds, setSelectedIds] = useState<string[]>([]);

  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<Error | null>(null);
  const [pendingDelete, setPendingDelete] = useState<PendingDelete | null>(null);
  const [showCreateItem, setShowCreateItem] = useState(false);
  const [showCreateFolder, setShowCreateFolder] = useState(false);
  const [showTagManager, setShowTagManager] = useState(false);

  // 请求竞态守卫：过滤器快速切换时旧响应不得覆盖新状态
  const listRequestRef = useRef(0);

  // 关键字防抖 — 每敲一键就打后端的旧行为是性能缺陷
  useEffect(() => {
    const timer = window.setTimeout(() => setDebouncedKeyword(keyword), SEARCH_DEBOUNCE_MS);
    return () => window.clearTimeout(timer);
  }, [keyword]);

  /** folders/tags/stats — 仅初始与增删改后刷新，不随过滤器抖动 */
  const loadMeta = useCallback(async () => {
    const api = libraryApiOrNull();
    if (!api) return;
    const [folderRes, tagRes, statsRes] = await Promise.all([
      api.listFolders(),
      api.listTags(),
      api.getStats(),
    ]);
    setFolders(folderRes);
    setTags(tagRes);
    setStats(statsRes);
  }, []);

  /** items — 随过滤器变化；append=true 时是「加载更多」 */
  const loadItems = useCallback(async (offset: number, append: boolean) => {
    const api = libraryApiOrNull();
    if (!api) return;
    const requestId = ++listRequestRef.current;
    const page = await api.listItems({
      folderId: selectedFolderId ?? undefined,
      tagId: selectedTagId ?? undefined,
      keyword: debouncedKeyword || undefined,
      limit: LIBRARY_PAGE_SIZE,
      offset,
    });
    if (requestId !== listRequestRef.current) return;
    setItems((prev) => (append ? [...prev, ...page] : page));
    setHasMore(page.length === LIBRARY_PAGE_SIZE);
  }, [selectedFolderId, selectedTagId, debouncedKeyword]);

  // loadItems 随过滤器重建；reloadAll 经 ref 引用最新版以保持自身稳定，
  // 否则过滤器每次变化都会连带 meta（folders/tags/stats）整体重拉。
  const loadItemsRef = useRef(loadItems);
  useEffect(() => { loadItemsRef.current = loadItems; }, [loadItems]);

  const reloadAll = useCallback(async () => {
    const api = libraryApiOrNull();
    if (!api) {
      setError(new Error(classifyError(new Error('Library API unavailable')).userMessage));
      setLoading(false);
      return;
    }
    setError(null);
    try {
      await Promise.all([loadMeta(), loadItemsRef.current(0, false)]);
    } catch (e) {
      setError(new Error(classifyError(e).userMessage));
    } finally {
      setLoading(false);
    }
  }, [loadMeta]);

  // 初始加载（reloadAll 稳定，仅执行一次）
  useEffect(() => { reloadAll(); }, [reloadAll]);

  // 过滤器变化只刷新条目（不重挂全页 loading）；跳过首帧，避免与初始加载重复请求
  const filterEffectPrimed = useRef(false);
  useEffect(() => {
    if (!filterEffectPrimed.current) {
      filterEffectPrimed.current = true;
      return;
    }
    setSelectedIds([]);
    loadItems(0, false).catch((e) => {
      toast(classifyError(e).userMessage, 'error');
    });
  }, [loadItems, toast]);

  const handleLoadMore = async () => {
    setLoadingMore(true);
    try {
      await loadItems(items.length, true);
    } catch (e) {
      toast(classifyError(e).userMessage, 'error');
    } finally {
      setLoadingMore(false);
    }
  };

  const runMutation = async (fn: () => Promise<void>, successMessage: string) => {
    const api = libraryApiOrNull();
    if (!api) {
      toast(classifyError(new Error('Library API unavailable')).userMessage, 'error');
      return false;
    }
    try {
      await fn();
      if (successMessage) toast(successMessage, 'success');
      await reloadAll();
      return true;
    } catch (e) {
      toast(classifyError(e).userMessage, 'error');
      return false;
    }
  };

  const handleConfirmDelete = async () => {
    if (!pendingDelete) return;
    const api = libraryApiOrNull();
    if (!api) {
      toast(classifyError(new Error('Library API unavailable')).userMessage, 'error');
      setPendingDelete(null);
      return;
    }
    const target = pendingDelete;
    setPendingDelete(null);
    if (target.type === 'batch') {
      const count = selectedIds.length;
      await runMutation(async () => {
        await api.batchDelete({ itemIds: selectedIds });
        setSelectedIds([]);
        setSelectedItem((prev) => (prev && selectedIds.includes(prev.id) ? null : prev));
      }, t('library.deleteBatchSuccess', { count }));
    } else if (target.type === 'item') {
      await runMutation(async () => {
        await api.deleteItem(target.id);
        setSelectedItem((prev) => (prev?.id === target.id ? null : prev));
        setSelectedIds((prev) => prev.filter((id) => id !== target.id));
      }, t('library.deleteSuccess'));
    } else if (target.type === 'folder') {
      await runMutation(async () => {
        await api.deleteFolder(target.folder.id, true);
        setSelectedFolderId((prev) => (prev === target.folder.id ? null : prev));
      }, t('library.deleteFolderSuccess'));
    } else {
      await runMutation(async () => {
        await api.deleteTag(target.tag.id);
        setSelectedTagId((prev) => (prev === target.tag.id ? null : prev));
      }, t('library.deleteTagSuccess'));
    }
  };

  const handleRenameFolder = (id: string, name: string) => {
    void runMutation(async () => {
      await libraryApiOrNull()!.updateFolder({ id, name });
    }, t('library.renameFolderSuccess'));
  };

  const handleBatchTag = (tagIds: string[]) => {
    void runMutation(async () => {
      await libraryApiOrNull()!.batchTag({ itemIds: selectedIds, tagIds });
    }, t('library.batchTagSuccess'));
  };

  const handleBatchMove = (folderId: string | null) => {
    void runMutation(async () => {
      await libraryApiOrNull()!.batchMove({ itemIds: selectedIds, folderId: folderId ?? undefined });
    }, t('library.batchMoveSuccess'));
  };

  // 选中条目切换后，用最新列表数据同步详情面板（编辑保存后内容才会刷新）
  useEffect(() => {
    setSelectedItem((prev) => {
      if (!prev) return prev;
      return items.find((i) => i.id === prev.id) ?? prev;
    });
  }, [items]);

  const confirmMessage = (() => {
    if (!pendingDelete) return '';
    switch (pendingDelete.type) {
      case 'batch': return t('library.deleteBatchMessage', { count: selectedIds.length });
      case 'item': return t('library.deleteMessage');
      case 'folder': return t('library.deleteFolderMessage', { name: pendingDelete.folder.name });
      case 'tag': return t('library.deleteTagMessage', { name: pendingDelete.tag.name });
    }
  })();

  if (loading) {
    return <div className="flex flex-1 items-center justify-center"><MathCurveLoader /></div>;
  }

  if (error) {
    return <ErrorState message={error.message} onRetry={reloadAll} />;
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
          onRename={handleRenameFolder}
          onDelete={(folder) => setPendingDelete({ type: 'folder', folder })}
        />
      </div>

      {/* Center: items list */}
      <div className="flex flex-1 flex-col overflow-hidden">
        <SearchFilterBar
          keyword={keyword}
          onKeywordChange={setKeyword}
          viewMode={viewMode}
          onViewModeChange={setViewMode}
          tags={tags}
          selectedTagId={selectedTagId}
          onTagChange={setSelectedTagId}
          onCreateItem={() => setShowCreateItem(true)}
          onManageTags={() => setShowTagManager(true)}
        />

        {selectedIds.length > 0 && (
          <BatchToolbar
            selectedCount={selectedIds.length}
            tags={tags}
            folders={folders}
            onTag={handleBatchTag}
            onMove={handleBatchMove}
            onDelete={() => setPendingDelete({ type: 'batch' })}
            onClear={() => setSelectedIds([])}
          />
        )}

        {viewMode === 'list' ? (
          <div className="flex-1 overflow-auto">
            {items.length === 0 ? (
              <EmptyState
                title={debouncedKeyword || selectedTagId || selectedFolderId ? t('library.noResults') : t('library.empty')}
                description={debouncedKeyword || selectedTagId || selectedFolderId ? undefined : t('library.emptyDesc')}
                action={{ label: t('library.createItem'), onClick: () => setShowCreateItem(true) }}
              />
            ) : (
              <>
                <ItemList
                  items={items}
                  selectedIds={selectedIds}
                  onSelectionChange={setSelectedIds}
                  onSelectItem={setSelectedItem}
                  selectedItemId={selectedItem?.id ?? null}
                />
                {hasMore && (
                  <div className="flex justify-center py-3">
                    <button
                      type="button"
                      onClick={handleLoadMore}
                      disabled={loadingMore}
                      className="rounded border px-4 py-1.5 text-xs disabled:opacity-50"
                      style={{ borderColor: 'var(--border)', background: 'var(--surface)', color: 'var(--text-secondary)' }}
                    >
                      {loadingMore ? t('common.loading') : t('library.loadMore')}
                    </button>
                  </div>
                )}
              </>
            )}
          </div>
        ) : (
          <div className="flex-1 overflow-auto p-4">
            <StatsPanel stats={stats} />
          </div>
        )}
      </div>

      {/* Right: item detail — key 强制随条目重建，防编辑态残留（数据破坏防线） */}
      {selectedItem && (
        <div className="w-80 overflow-auto border-l" style={{ borderColor: 'var(--border)' }}>
          <ItemDetail
            key={selectedItem.id}
            item={selectedItem}
            folders={folders}
            tags={tags}
            onClose={() => setSelectedItem(null)}
            onDelete={(id) => setPendingDelete({ type: 'item', id })}
            onSaved={reloadAll}
          />
        </div>
      )}

      {/* Delete confirmation (item / batch / folder / tag) */}
      <ConfirmDialog
        open={pendingDelete !== null}
        title={t('library.deleteConfirm')}
        message={confirmMessage}
        confirmLabel={t('common.delete')}
        cancelLabel={t('common.cancel')}
        danger
        onConfirm={handleConfirmDelete}
        onCancel={() => setPendingDelete(null)}
      />

      <CreateItemDialog
        open={showCreateItem}
        folders={folders}
        tags={tags}
        onClose={() => setShowCreateItem(false)}
        onSaved={() => { setShowCreateItem(false); void reloadAll(); }}
      />

      <CreateFolderDialog
        open={showCreateFolder}
        onClose={() => setShowCreateFolder(false)}
        onSaved={() => { setShowCreateFolder(false); void reloadAll(); }}
      />

      <TagManagerDialog
        open={showTagManager}
        tags={tags}
        onClose={() => setShowTagManager(false)}
        onChanged={() => void reloadAll()}
        onDeleteTag={(tag) => {
          // 先收起管理弹窗再弹确认，避免双层 Esc 事件互相截断
          setShowTagManager(false);
          setPendingDelete({ type: 'tag', tag });
        }}
      />
    </div>
  );
}

// ── Sub-dialogs（统一走 ui/Modal：焦点圈闭 / Esc / 背景点击关闭 / aria） ──

function CreateItemDialog({ open, folders, tags, onClose, onSaved }: {
  open: boolean; folders: LibraryFolder[]; tags: LibraryTag[]; onClose: () => void; onSaved: () => void;
}) {
  const locale = useLocale();
  const t = (key: string) => tr(locale, key);
  const { toast } = useToast();
  const [title, setTitle] = useState('');
  const [description, setDescription] = useState('');
  const [content, setContent] = useState('');
  const [sourceUrl, setSourceUrl] = useState('');
  const [itemType, setItemType] = useState<string>('note');
  const [folderId, setFolderId] = useState<string | undefined>(undefined);
  const [selectedTagIds, setSelectedTagIds] = useState<string[]>([]);
  const [saving, setSaving] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const reset = () => {
    setTitle(''); setDescription(''); setContent(''); setSourceUrl('');
    setItemType('note'); setFolderId(undefined); setSelectedTagIds([]); setErr(null);
  };

  const handleClose = () => { reset(); onClose(); };

  const handleSave = async () => {
    if (!title.trim() || saving) return;
    const api = libraryApiOrNull();
    if (!api) {
      setErr(classifyError(new Error('Library API unavailable')).userMessage);
      return;
    }
    setSaving(true);
    setErr(null);
    try {
      await api.createItem({
        folderId,
        title: title.trim(),
        description,
        content,
        sourceUrl: sourceUrl.trim(),
        itemType,
        tagIds: selectedTagIds,
      });
      toast(t('library.createSuccess'), 'success');
      reset();
      onSaved();
    } catch (e) {
      setErr(classifyError(e).userMessage);
    } finally {
      setSaving(false);
    }
  };

  const typeLabel = (value: string) => {
    const label = t(`library.itemType.${value}`);
    return label.startsWith('library.') ? value : label;
  };

  const inputStyle = { borderColor: 'var(--border)', background: 'var(--surface)', color: 'var(--text)' } as const;

  return (
    <Modal isOpen={open} onClose={handleClose} title={t('library.createItem')} width={420}>
      <div className="space-y-3">
        <input
          placeholder={t('library.title')} value={title} onChange={(e) => setTitle(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && handleSave()}
          className="w-full rounded border px-3 py-2 text-sm" autoFocus
          aria-label={t('library.title')}
          style={inputStyle} />
        <textarea
          placeholder={t('library.description')} value={description} onChange={(e) => setDescription(e.target.value)}
          className="h-16 w-full resize-none rounded border px-3 py-2 text-sm"
          aria-label={t('library.description')}
          style={inputStyle} />
        <textarea
          placeholder={t('library.content')} value={content} onChange={(e) => setContent(e.target.value)}
          className="h-24 w-full resize-none rounded border px-3 py-2 font-mono text-xs"
          aria-label={t('library.content')}
          style={inputStyle} />
        <input
          placeholder={t('library.sourceUrl')} value={sourceUrl} onChange={(e) => setSourceUrl(e.target.value)}
          className="w-full rounded border px-3 py-2 text-sm"
          aria-label={t('library.sourceUrl')}
          style={inputStyle} />
        <div className="flex gap-2">
          <select value={itemType} onChange={(e) => setItemType(e.target.value)}
            className="flex-1 rounded border px-3 py-2 text-sm"
            aria-label={t('library.type')}
            style={inputStyle}>
            {ITEM_TYPES.map((tp) => <option key={tp} value={tp}>{typeLabel(tp)}</option>)}
          </select>
          <select value={folderId ?? ''} onChange={(e) => setFolderId(e.target.value || undefined)}
            className="flex-1 rounded border px-3 py-2 text-sm"
            aria-label={t('library.folders')}
            style={inputStyle}>
            <option value="">{t('library.noFolder')}</option>
            {folders.map((f) => <option key={f.id} value={f.id}>{f.name}</option>)}
          </select>
        </div>
        <div>
          <p className="mb-1 text-sm" style={{ color: 'var(--text-secondary)' }}>{t('library.tags')}</p>
          {tags.length === 0
            ? <p className="text-xs" style={{ color: 'var(--text-disabled)' }}>{t('library.noTags')}</p>
            : <TagPicker tags={tags} selectedIds={selectedTagIds} onChange={setSelectedTagIds} />}
        </div>
        {err && <p className="text-sm" style={{ color: 'var(--danger)' }}>{err}</p>}
      </div>
      <div className="mt-4 flex justify-end gap-2">
        <button type="button" onClick={handleClose} className="px-4 py-2 text-sm" disabled={saving}
          style={{ color: 'var(--text-secondary)' }}>{t('common.cancel')}</button>
        <button type="button" onClick={handleSave} disabled={!title.trim() || saving}
          className="rounded px-4 py-2 text-sm disabled:opacity-50"
          style={{ background: 'var(--primary)', color: 'var(--primary-foreground, var(--accent-ink))' }}>
          {saving ? t('common.saving') : t('common.save')}
        </button>
      </div>
    </Modal>
  );
}

function CreateFolderDialog({ open, onClose, onSaved }: { open: boolean; onClose: () => void; onSaved: () => void }) {
  const locale = useLocale();
  const t = (key: string) => tr(locale, key);
  const { toast } = useToast();
  const [name, setName] = useState('');
  const [saving, setSaving] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const handleClose = () => { setName(''); setErr(null); onClose(); };

  const handleSave = async () => {
    if (!name.trim() || saving) return;
    const api = libraryApiOrNull();
    if (!api) {
      setErr(classifyError(new Error('Library API unavailable')).userMessage);
      return;
    }
    setSaving(true);
    setErr(null);
    try {
      await api.createFolder({ name: name.trim() });
      toast(t('library.createFolderSuccess'), 'success');
      setName('');
      onSaved();
    } catch (e) {
      setErr(classifyError(e).userMessage);
    } finally {
      setSaving(false);
    }
  };

  return (
    <Modal isOpen={open} onClose={handleClose} title={t('library.createFolder')} width={340}>
      <input
        placeholder={t('library.folderName')} value={name} onChange={(e) => setName(e.target.value)}
        className="w-full rounded border px-3 py-2 text-sm" autoFocus
        aria-label={t('library.folderName')}
        style={{ borderColor: 'var(--border)', background: 'var(--surface)', color: 'var(--text)' }}
        onKeyDown={(e) => e.key === 'Enter' && handleSave()} />
      {err && <p className="mt-2 text-sm" style={{ color: 'var(--danger)' }}>{err}</p>}
      <div className="mt-4 flex justify-end gap-2">
        <button type="button" onClick={handleClose} className="px-4 py-2 text-sm" disabled={saving}
          style={{ color: 'var(--text-secondary)' }}>{t('common.cancel')}</button>
        <button type="button" onClick={handleSave} disabled={!name.trim() || saving}
          className="rounded px-4 py-2 text-sm disabled:opacity-50"
          style={{ background: 'var(--primary)', color: 'var(--primary-foreground, var(--accent-ink))' }}>
          {saving ? t('common.saving') : t('common.save')}
        </button>
      </div>
    </Modal>
  );
}

/** 标签管理：创建（名称 + 预置色板）与删除。此前 createTag/deleteTag 后端命令无任何 UI 入口。 */
function TagManagerDialog({ open, tags, onClose, onChanged, onDeleteTag }: {
  open: boolean; tags: LibraryTag[]; onClose: () => void;
  onChanged: () => void; onDeleteTag: (tag: LibraryTag) => void;
}) {
  const locale = useLocale();
  const t = (key: string) => tr(locale, key);
  const { toast } = useToast();
  const [name, setName] = useState('');
  const [color, setColor] = useState<string>(TAG_COLOR_PRESETS[0]);
  const [saving, setSaving] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const handleCreate = async () => {
    if (!name.trim() || saving) return;
    const api = libraryApiOrNull();
    if (!api) {
      setErr(classifyError(new Error('Library API unavailable')).userMessage);
      return;
    }
    setSaving(true);
    setErr(null);
    try {
      await api.createTag({ name: name.trim(), color });
      toast(t('library.createTagSuccess'), 'success');
      setName('');
      onChanged();
    } catch (e) {
      setErr(classifyError(e).userMessage);
    } finally {
      setSaving(false);
    }
  };

  return (
    <Modal isOpen={open} onClose={onClose} title={t('library.manageTags')} width={380}>
      {/* Create row */}
      <div className="space-y-2">
        <div className="flex gap-2">
          <input
            placeholder={t('library.tagName')} value={name} onChange={(e) => setName(e.target.value)}
            className="min-w-0 flex-1 rounded border px-3 py-2 text-sm"
            aria-label={t('library.tagName')}
            style={{ borderColor: 'var(--border)', background: 'var(--surface)', color: 'var(--text)' }}
            onKeyDown={(e) => e.key === 'Enter' && handleCreate()} />
          <button type="button" onClick={handleCreate} disabled={!name.trim() || saving}
            className="rounded px-3 py-2 text-sm disabled:opacity-50"
            style={{ background: 'var(--primary)', color: 'var(--primary-foreground, var(--accent-ink))' }}>
            {saving ? t('common.saving') : t('library.newTag')}
          </button>
        </div>
        <div className="flex items-center gap-1.5" role="radiogroup" aria-label={t('library.tagColor')}>
          {TAG_COLOR_PRESETS.map((preset) => (
            <button
              key={preset}
              type="button"
              role="radio"
              aria-checked={color === preset}
              aria-label={preset}
              onClick={() => setColor(preset)}
              className="h-5 w-5 rounded-full border-2"
              style={{
                background: preset,
                borderColor: color === preset ? 'var(--text)' : 'transparent',
              }}
            />
          ))}
        </div>
        {err && <p className="text-sm" style={{ color: 'var(--danger)' }}>{err}</p>}
      </div>

      {/* Existing tags */}
      <div className="mt-4 space-y-1 border-t pt-3" style={{ borderColor: 'var(--border)' }}>
        {tags.length === 0 ? (
          <p className="text-xs" style={{ color: 'var(--text-disabled)' }}>{t('library.noTags')}</p>
        ) : tags.map((tag) => (
          <div key={tag.id} className="flex items-center justify-between rounded px-2 py-1.5">
            <span className="inline-flex items-center gap-2 text-sm" style={{ color: 'var(--text)' }}>
              <span className="h-3 w-3 rounded-full" style={{ background: tag.color }} aria-hidden />
              {tag.name}
            </span>
            <button
              type="button"
              onClick={() => onDeleteTag(tag)}
              title={t('library.deleteTag')}
              aria-label={`${t('library.deleteTag')}: ${tag.name}`}
              style={{ color: 'var(--danger)' }}
            >
              <Trash2 size={13} />
            </button>
          </div>
        ))}
      </div>
    </Modal>
  );
}
