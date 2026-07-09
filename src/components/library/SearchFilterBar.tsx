'use client';

import { useLocale, t as tr } from '@/i18n';

interface Tag { id: string; name: string; color: string; createdAt: string; }

export function SearchFilterBar({
  keyword, onKeywordChange, viewMode, onViewModeChange,
  tags, selectedTagId, onTagChange, onCreateItem,
}: {
  keyword: string; onKeywordChange: (v: string) => void;
  viewMode: 'list' | 'stats'; onViewModeChange: (v: 'list' | 'stats') => void;
  tags: Tag[]; selectedTagId: string | null; onTagChange: (id: string | null) => void;
  onCreateItem: () => void;
}) {
  const locale = useLocale();
  const t = (key: string) => tr(locale, key);

  return (
    <div className="px-3 py-2 border-b border-gray-200 dark:border-gray-700 flex items-center gap-2 flex-wrap">
      {/* Search */}
      <div className="relative flex-1 min-w-[160px]">
        <input
          type="text"
          value={keyword}
          onChange={(e) => onKeywordChange(e.target.value)}
          placeholder={t('library.search')}
          className="w-full px-3 py-1.5 text-sm border border-gray-300 dark:border-gray-600 rounded bg-white dark:bg-gray-700 pl-8"
        />
        <span className="absolute left-2.5 top-1/2 -translate-y-1/2 text-gray-400 text-sm">🔍</span>
      </div>

      {/* Tag filter */}
      <select
        value={selectedTagId ?? ''}
        onChange={(e) => onTagChange(e.target.value || null)}
        className="px-2 py-1.5 text-xs border border-gray-300 dark:border-gray-600 rounded bg-white dark:bg-gray-700"
      >
        <option value="">{t('library.allTags')}</option>
        {tags.map((tag) => (
          <option key={tag.id} value={tag.id}>{tag.name}</option>
        ))}
      </select>

      {/* View mode toggle */}
      <div className="flex border border-gray-300 dark:border-gray-600 rounded overflow-hidden">
        <button
          onClick={() => onViewModeChange('list')}
          className={`px-2 py-1 text-xs ${viewMode === 'list' ? 'bg-blue-500 text-white' : 'bg-white dark:bg-gray-700'}`}
        >
          📋
        </button>
        <button
          onClick={() => onViewModeChange('stats')}
          className={`px-2 py-1 text-xs ${viewMode === 'stats' ? 'bg-blue-500 text-white' : 'bg-white dark:bg-gray-700'}`}
        >
          📊
        </button>
      </div>

      {/* Create item button */}
      <button
        onClick={onCreateItem}
        className="px-3 py-1.5 text-xs bg-blue-500 text-white rounded hover:bg-blue-600 flex items-center gap-1"
      >
        + {t('library.newItem')}
      </button>
    </div>
  );
}
