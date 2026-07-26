'use client';

import { t, type Locale } from '@/i18n';
import { SKILL_CATEGORIES, type CategoryFilterValue } from './capability-types';

interface CategoryFilterBarProps {
  locale: Locale;
  value: CategoryFilterValue;
  onChange: (value: CategoryFilterValue) => void;
}

const FILTER_VALUES: CategoryFilterValue[] = ['all', ...SKILL_CATEGORIES, 'uncategorized'];

/** Category chips: 全部 / 办公 / 工具 / 投资 / 效率 / 其他 / 未分类. */
export default function CategoryFilterBar({ locale, value, onChange }: CategoryFilterBarProps) {
  return (
    <div
      className="flex flex-wrap items-center gap-1.5"
      role="radiogroup"
      aria-label={t(locale, 'capabilities.categories.filterLabel')}
    >
      {FILTER_VALUES.map((category) => {
        const active = value === category;
        return (
          <button
            key={category}
            type="button"
            role="radio"
            aria-checked={active}
            onClick={() => onChange(category)}
            className="rounded-full border px-2.5 py-1 text-xs transition"
            style={{
              borderColor: active ? 'var(--primary)' : 'var(--border-subtle)',
              background: active ? 'var(--primary)' : 'transparent',
              color: active ? '#fff' : 'var(--text-secondary)',
            }}
          >
            {t(locale, `capabilities.categories.${category}`)}
          </button>
        );
      })}
    </div>
  );
}

/** True when the skill's category matches the current filter chip. */
export function matchesCategory(category: string | null | undefined, filter: CategoryFilterValue): boolean {
  if (filter === 'all') return true;
  if (filter === 'uncategorized') return !category;
  return category === filter;
}

/** Localised label for a category; unknown backend values fall through verbatim. */
export function categoryLabel(locale: Locale, category: string | null | undefined): string {
  if (!category) return t(locale, 'capabilities.categories.uncategorized');
  if ((SKILL_CATEGORIES as readonly string[]).includes(category)) {
    return t(locale, `capabilities.categories.${category}`);
  }
  return category;
}
