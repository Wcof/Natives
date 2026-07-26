'use client';

import { useLocale, t as tr } from '@/i18n';
import { BarChart3, List, Plus, Search, Tags } from 'lucide-react';
import type { LibraryTag } from '@/lib/library-api';

export function SearchFilterBar({
  keyword, onKeywordChange, viewMode, onViewModeChange,
  tags, selectedTagId, onTagChange, onCreateItem, onManageTags,
}: {
  keyword: string; onKeywordChange: (v: string) => void;
  viewMode: 'list' | 'stats'; onViewModeChange: (v: 'list' | 'stats') => void;
  tags: LibraryTag[]; selectedTagId: string | null; onTagChange: (id: string | null) => void;
  onCreateItem: () => void; onManageTags: () => void;
}) {
  const locale = useLocale();
  const t = (key: string) => tr(locale, key);

  return (
    <div className="flex flex-wrap items-center gap-2 border-b px-3 py-2" style={{ borderColor: 'var(--border)' }}>
      {/* Search */}
      <div className="relative min-w-[160px] flex-1">
        <input
          type="text"
          value={keyword}
          onChange={(e) => onKeywordChange(e.target.value)}
          placeholder={t('library.search')}
          aria-label={t('library.search')}
          className="w-full rounded border px-3 py-1.5 pl-8 text-sm"
          style={{ borderColor: 'var(--border)', background: 'var(--surface)', color: 'var(--text)' }}
        />
        <Search
          size={14}
          className="absolute left-2.5 top-1/2 -translate-y-1/2"
          style={{ color: 'var(--text-disabled)' }}
        />
      </div>

      {/* Tag filter */}
      <select
        value={selectedTagId ?? ''}
        onChange={(e) => onTagChange(e.target.value || null)}
        aria-label={t('library.allTags')}
        className="rounded border px-2 py-1.5 text-xs"
        style={{ borderColor: 'var(--border)', background: 'var(--surface)', color: 'var(--text)' }}
      >
        <option value="">{t('library.allTags')}</option>
        {tags.map((tag) => (
          <option key={tag.id} value={tag.id}>{tag.name}</option>
        ))}
      </select>

      {/* Tag management */}
      <button
        type="button"
        onClick={onManageTags}
        title={t('library.manageTags')}
        aria-label={t('library.manageTags')}
        className="inline-flex items-center gap-1 rounded border px-2 py-1.5 text-xs"
        style={{ borderColor: 'var(--border)', background: 'var(--surface)', color: 'var(--text-secondary)' }}
      >
        <Tags size={13} />
      </button>

      {/* View mode toggle */}
      <div className="flex overflow-hidden rounded border" style={{ borderColor: 'var(--border)' }}>
        <button
          type="button"
          onClick={() => onViewModeChange('list')}
          title={t('library.viewList')}
          aria-label={t('library.viewList')}
          aria-pressed={viewMode === 'list'}
          className="px-2 py-1 text-xs"
          style={{
            background: viewMode === 'list' ? 'var(--accent)' : 'var(--surface)',
            color: viewMode === 'list' ? 'var(--accent-ink)' : 'var(--text-secondary)',
          }}
        >
          <List size={13} />
        </button>
        <button
          type="button"
          onClick={() => onViewModeChange('stats')}
          title={t('library.statistics')}
          aria-label={t('library.statistics')}
          aria-pressed={viewMode === 'stats'}
          className="px-2 py-1 text-xs"
          style={{
            background: viewMode === 'stats' ? 'var(--accent)' : 'var(--surface)',
            color: viewMode === 'stats' ? 'var(--accent-ink)' : 'var(--text-secondary)',
          }}
        >
          <BarChart3 size={13} />
        </button>
      </div>

      {/* Create item button */}
      <button
        type="button"
        onClick={onCreateItem}
        className="inline-flex items-center gap-1 rounded px-3 py-1.5 text-xs font-medium"
        style={{ background: 'var(--primary)', color: 'var(--primary-foreground, var(--accent-ink))' }}
      >
        <Plus size={13} /> {t('library.newItem')}
      </button>
    </div>
  );
}
