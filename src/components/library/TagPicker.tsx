'use client';

import type { LibraryTag } from '@/lib/library-api';

/** 标签多选器 — CreateItemDialog / ItemDetail 编辑态共用（消除三处重复的 toggle 实现） */
export function TagPicker({ tags, selectedIds, onChange }: {
  tags: LibraryTag[]; selectedIds: string[]; onChange: (ids: string[]) => void;
}) {
  const handleToggle = (tagId: string) => {
    onChange(selectedIds.includes(tagId) ? selectedIds.filter(id => id !== tagId) : [...selectedIds, tagId]);
  };

  return (
    <div className="flex flex-wrap gap-1">
      {tags.map((tag) => {
        const active = selectedIds.includes(tag.id);
        return (
          <button
            key={tag.id}
            type="button"
            onClick={() => handleToggle(tag.id)}
            aria-pressed={active}
            className="inline-flex items-center gap-1 rounded border px-2 py-0.5 text-xs"
            style={{
              borderColor: active ? 'var(--accent)' : 'var(--border)',
              background: active ? 'var(--accent-soft)' : 'var(--surface)',
              color: 'var(--text)',
              borderLeftColor: tag.color,
              borderLeftWidth: 3,
            }}
          >
            {tag.name}
          </button>
        );
      })}
    </div>
  );
}

export function TagBadge({ tag }: { tag: LibraryTag }) {
  return (
    <span
      className="inline-flex items-center rounded border px-1.5 py-0.5 text-xs"
      style={{
        borderColor: 'var(--border)',
        color: 'var(--text-secondary)',
        borderLeftColor: tag.color,
        borderLeftWidth: 3,
      }}
    >
      {tag.name}
    </span>
  );
}
