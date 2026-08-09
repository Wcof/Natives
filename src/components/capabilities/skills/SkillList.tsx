'use client';

import { Sparkles } from 'lucide-react';
import { t, type Locale } from '@/i18n';
import { categoryLabel } from '../shared/CategoryFilterBar';
import type { CapabilitySkill } from '@/types/capability';

interface SkillListProps {
  locale: Locale;
  skills: CapabilitySkill[];
  selectedId: string | null;
  onSelect: (id: string) => void;
}

function Badge({ text, tone }: { text: string; tone: 'ok' | 'muted' | 'warn' }) {
  const color =
    tone === 'ok' ? 'var(--success)' : tone === 'warn' ? 'var(--warning)' : 'var(--text-disabled)';
  return (
    <span
      className="rounded-full border px-1.5 py-0.5 text-[10px] leading-none"
      style={{ borderColor: 'var(--border-subtle)', color }}
    >
      {text}
    </span>
  );
}

/** Presentational skill list — selection + state badges only. */
export default function SkillList({ locale, skills, selectedId, onSelect }: SkillListProps) {
  return (
    <div className="flex flex-col gap-1" role="listbox" aria-label={t(locale, 'capabilities.tabs.skills')}>
      {skills.map((skill) => {
        const active = selectedId === skill.id;
        return (
          <button
            key={skill.id}
            type="button"
            role="option"
            aria-selected={active}
            onClick={() => onSelect(skill.id)}
            className="flex w-full items-start gap-2.5 rounded-lg border px-3 py-2 text-left transition"
            style={{
              borderColor: active ? 'var(--primary)' : 'var(--border-subtle)',
              background: active ? 'var(--surface-hover)' : 'var(--surface)',
            }}
          >
            <Sparkles size={16} className="mt-0.5 shrink-0" style={{ color: 'var(--primary)' }} aria-hidden />
            <span className="min-w-0 flex-1">
              <span className="flex items-center gap-2">
                <span className="truncate text-sm font-medium" style={{ color: 'var(--text)' }}>
                  {skill.name}
                </span>
                <Badge
                  text={
                    skill.enabled
                      ? t(locale, 'capabilities.common.enabled')
                      : t(locale, 'capabilities.common.disabled')
                  }
                  tone={skill.enabled ? 'ok' : 'muted'}
                />
                <Badge
                  text={
                    skill.trusted
                      ? t(locale, 'capabilities.common.trusted')
                      : t(locale, 'capabilities.common.untrusted')
                  }
                  tone={skill.trusted ? 'ok' : 'warn'}
                />
                {skill.category ? <Badge text={categoryLabel(locale, skill.category)} tone="muted" /> : null}
              </span>
              {skill.description ? (
                <span className="mt-0.5 block truncate text-xs" style={{ color: 'var(--text-secondary)' }}>
                  {skill.description}
                </span>
              ) : null}
              {skill.tags.length > 0 ? (
                <span className="mt-1 flex flex-wrap gap-1">
                  {skill.tags.map((tag) => (
                    <span
                      key={tag}
                      className="rounded px-1.5 py-0.5 text-[10px]"
                      style={{ background: 'var(--surface-hover)', color: 'var(--text-disabled)' }}
                    >
                      {tag}
                    </span>
                  ))}
                </span>
              ) : null}
            </span>
          </button>
        );
      })}
    </div>
  );
}
