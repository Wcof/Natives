'use client';

import { useLocale, t as tr } from '@/i18n';

interface Tag { id: string; name: string; color: string; createdAt: string; }

export function TagPicker({ tags, selectedIds, onChange }: {
  tags: Tag[]; selectedIds: string[]; onChange: (ids: string[]) => void;
}) {
  const handleToggle = (tagId: string) => {
    onChange(selectedIds.includes(tagId) ? selectedIds.filter(id => id !== tagId) : [...selectedIds, tagId]);
  };

  return (
    <div className="flex flex-wrap gap-1">
      {tags.map((tag) => (
        <button
          key={tag.id}
          onClick={() => handleToggle(tag.id)}
          className={`inline-flex items-center gap-1 px-2 py-0.5 rounded text-xs border ${
            selectedIds.includes(tag.id)
              ? 'border-blue-500 bg-blue-50 dark:bg-blue-900/30 text-blue-700 dark:text-blue-300'
              : 'border-gray-300 dark:border-gray-600 hover:bg-gray-50 dark:hover:bg-gray-700'
          }`}
          style={{ borderLeftColor: tag.color, borderLeftWidth: 3 }}
        >
          {tag.name}
        </button>
      ))}
    </div>
  );
}

export function TagBadge({ tag }: { tag: Tag }) {
  return (
    <span
      className="inline-flex items-center px-1.5 py-0.5 rounded text-xs border border-gray-300 dark:border-gray-600"
      style={{ borderLeftColor: tag.color, borderLeftWidth: 3 }}
    >
      {tag.name}
    </span>
  );
}
