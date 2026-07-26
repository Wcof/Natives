'use client';

import { useLocale, t as tr } from '@/i18n';
import { TagBadge } from './TagPicker';
import type { LibraryItem } from '@/lib/library-api';

export function ItemList({ items, selectedIds, onSelectionChange, onSelectItem, selectedItemId }: {
  items: LibraryItem[]; selectedIds: string[]; onSelectionChange: (ids: string[]) => void;
  onSelectItem: (item: LibraryItem) => void; selectedItemId: string | null;
}) {
  const locale = useLocale();
  const t = (key: string) => tr(locale, key);

  const handleSelect = (id: string, checked: boolean) => {
    onSelectionChange(checked ? [...selectedIds, id] : selectedIds.filter(sid => sid !== id));
  };

  const formatDate = (dateStr: string) => {
    try { return new Date(dateStr).toLocaleDateString(locale === 'zh' ? 'zh-CN' : 'en-US', { month: 'short', day: 'numeric' }); }
    catch { return dateStr; }
  };

  const allChecked = selectedIds.length === items.length && items.length > 0;

  return (
    <table className="w-full text-sm">
      <thead className="sticky top-0 border-b" style={{ background: 'var(--surface)', borderColor: 'var(--border)' }}>
        <tr>
          <th className="w-8 px-2 py-2">
            <input type="checkbox"
              checked={allChecked}
              aria-label={t('library.selectAll')}
              onChange={(e) => onSelectionChange(e.target.checked ? items.map(i => i.id) : [])}
              className="rounded"
            />
          </th>
          <th className="px-2 py-2 text-left text-xs font-medium uppercase" style={{ color: 'var(--text-secondary)' }}>{t('library.title')}</th>
          <th className="hidden px-2 py-2 text-left text-xs font-medium uppercase md:table-cell" style={{ color: 'var(--text-secondary)' }}>{t('library.tags')}</th>
          <th className="hidden px-2 py-2 text-left text-xs font-medium uppercase sm:table-cell" style={{ color: 'var(--text-secondary)' }}>{t('library.updated')}</th>
        </tr>
      </thead>
      <tbody>
        {items.map((item) => (
          <tr
            key={item.id}
            onClick={() => onSelectItem(item)}
            className="cursor-pointer border-b"
            style={{
              borderColor: 'var(--border)',
              background: selectedItemId === item.id ? 'var(--accent-soft)' : 'transparent',
            }}
          >
            <td className="px-2 py-2" onClick={(e) => e.stopPropagation()}>
              <input type="checkbox"
                checked={selectedIds.includes(item.id)}
                aria-label={`${t('library.selectItem')}: ${item.title || t('common.untitled')}`}
                onChange={(e) => handleSelect(item.id, e.target.checked)}
                className="rounded"
              />
            </td>
            <td className="px-2 py-2">
              <div className="font-medium" style={{ color: 'var(--text)' }}>{item.title || t('common.untitled')}</div>
              {item.description && <div className="max-w-xs truncate text-xs" style={{ color: 'var(--text-secondary)' }}>{item.description}</div>}
            </td>
            <td className="hidden px-2 py-2 md:table-cell">
              <div className="flex flex-wrap gap-1">
                {item.tags.map(tag => <TagBadge key={tag.id} tag={tag} />)}
              </div>
            </td>
            <td className="hidden whitespace-nowrap px-2 py-2 text-xs sm:table-cell" style={{ color: 'var(--text-secondary)' }}>
              {formatDate(item.updatedAt)}
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}
