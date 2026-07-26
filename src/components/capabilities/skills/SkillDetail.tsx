'use client';

import { useCallback, useEffect, useState } from 'react';
import { X } from 'lucide-react';
import { t, type Locale } from '@/i18n';
import type { AssistantGateway } from '@/lib/assistant-gateway';
import {
  deleteCapabilitySkill,
  getCapabilitySkill,
  updateCapabilitySkill,
} from '@/lib/assistant-workspace/capability-admin';
import { classifyError } from '@/lib/error-classifier';
import { useToast } from '@/components/ui/Toast';
import { Skeleton } from '@/components/ui/EmptyState';
import { SKILL_CATEGORIES, type CapabilitySkill, type CapabilitySkillDetail } from '../shared/capability-types';

interface SkillDetailProps {
  locale: Locale;
  gateway: AssistantGateway;
  skill: CapabilitySkill;
  onClose: () => void;
  onChanged: () => void;
  onDeleted: () => void;
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-col gap-1">
      <span className="text-xs font-semibold uppercase" style={{ color: 'var(--text-secondary)' }}>
        {label}
      </span>
      {children}
    </div>
  );
}

/** Right-hand drawer: category / tags editing, enable + trust toggles, uninstall. */
export default function SkillDetail({ locale, gateway, skill, onClose, onChanged, onDeleted }: SkillDetailProps) {
  const { toast } = useToast();
  const [detail, setDetail] = useState<CapabilitySkillDetail | null>(null);
  const [tagsText, setTagsText] = useState(skill.tags.join(', '));
  const [category, setCategory] = useState(skill.category ?? '');
  const [saving, setSaving] = useState(false);
  const [deleteMode, setDeleteMode] = useState<'unregister' | 'remove_dir' | null>(null);

  useEffect(() => {
    let cancelled = false;
    setDetail(null);
    setTagsText(skill.tags.join(', '));
    setCategory(skill.category ?? '');
    getCapabilitySkill(gateway, skill.id)
      .then((d) => {
        if (!cancelled) setDetail(d);
      })
      .catch((e) => {
        if (!cancelled) toast(classifyError(e).userMessage, 'error');
      });
    return () => {
      cancelled = true;
    };
  }, [gateway, skill.id, skill.tags, skill.category, toast]);

  const patch = useCallback(
    async (change: Parameters<typeof updateCapabilitySkill>[2]) => {
      setSaving(true);
      try {
        await updateCapabilitySkill(gateway, skill.id, change);
        toast(t(locale, 'capabilities.skills.updated'), 'success');
        onChanged();
      } catch (e) {
        toast(classifyError(e).userMessage, 'error');
      } finally {
        setSaving(false);
      }
    },
    [gateway, skill.id, locale, onChanged, toast],
  );

  const handleDelete = useCallback(
    async (mode: 'unregister' | 'remove_dir') => {
      try {
        await deleteCapabilitySkill(gateway, skill.id, mode);
        toast(t(locale, 'capabilities.skills.deleted'), 'success');
        onDeleted();
      } catch (e) {
        toast(classifyError(e).userMessage, 'error');
      } finally {
        setDeleteMode(null);
      }
    },
    [gateway, skill.id, locale, onDeleted, toast],
  );

  const inputStyle = {
    borderColor: 'var(--border)',
    background: 'var(--surface)',
    color: 'var(--text)',
  } as const;

  return (
    <div className="fixed inset-0 z-40" role="dialog" aria-modal="true" aria-label={t(locale, 'capabilities.skills.detail')}>
      <div className="absolute inset-0" style={{ background: 'rgba(0,0,0,0.3)' }} onClick={onClose} aria-hidden />
      <aside
        className="absolute right-0 top-0 flex h-full w-[400px] max-w-[92vw] flex-col overflow-hidden"
        style={{ background: 'var(--surface)', borderLeft: '1px solid var(--border)' }}
      >
        <div className="flex items-center justify-between px-4 py-3" style={{ borderBottom: '1px solid var(--border)' }}>
          <h2 className="truncate text-base font-semibold" style={{ color: 'var(--text)' }}>
            {skill.name}
          </h2>
          <button
            type="button"
            onClick={onClose}
            aria-label={t(locale, 'capabilities.common.close')}
            className="rounded p-1 hover:bg-[var(--surface-hover)]"
            style={{ color: 'var(--text-secondary)' }}
          >
            <X size={16} />
          </button>
        </div>

        <div className="flex-1 space-y-4 overflow-y-auto p-4">
          {skill.description ? (
            <p className="text-sm" style={{ color: 'var(--text-secondary)' }}>{skill.description}</p>
          ) : null}

          <Row label={t(locale, 'capabilities.skills.category')}>
            <select
              value={category}
              onChange={(e) => {
                setCategory(e.target.value);
                void patch({ category: e.target.value || null });
              }}
              disabled={saving}
              className="w-full rounded border px-2.5 py-1.5 text-sm"
              style={inputStyle}
              aria-label={t(locale, 'capabilities.skills.category')}
            >
              <option value="">{t(locale, 'capabilities.categories.uncategorized')}</option>
              {SKILL_CATEGORIES.map((c) => (
                <option key={c} value={c}>{t(locale, `capabilities.categories.${c}`)}</option>
              ))}
            </select>
          </Row>

          <Row label={t(locale, 'capabilities.skills.tags')}>
            <input
              value={tagsText}
              onChange={(e) => setTagsText(e.target.value)}
              onBlur={() => {
                const tags = tagsText.split(',').map((s) => s.trim()).filter(Boolean);
                if (tags.join(',') !== skill.tags.join(',')) void patch({ tags });
              }}
              placeholder={t(locale, 'capabilities.skills.tagsPlaceholder')}
              className="w-full rounded border px-2.5 py-1.5 text-sm"
              style={inputStyle}
              aria-label={t(locale, 'capabilities.skills.tags')}
            />
          </Row>

          <label className="flex items-center justify-between gap-2 text-sm" style={{ color: 'var(--text)' }}>
            {t(locale, 'capabilities.skills.enabledLabel')}
            <input
              type="checkbox"
              checked={skill.enabled}
              disabled={saving}
              onChange={(e) => void patch({ enabled: e.target.checked })}
            />
          </label>

          <div className="rounded-lg border p-3" style={{ borderColor: 'var(--warning)' }}>
            <label className="flex items-center justify-between gap-2 text-sm" style={{ color: 'var(--text)' }}>
              {t(locale, 'capabilities.skills.trustedLabel')}
              <input
                type="checkbox"
                checked={skill.trusted}
                disabled={saving}
                onChange={(e) => void patch({ trusted: e.target.checked })}
              />
            </label>
            <p className="mt-1 text-xs" style={{ color: 'var(--text-secondary)' }}>
              {t(locale, 'capabilities.skills.trustedHint')}
            </p>
          </div>

          <Row label={t(locale, 'capabilities.skills.engineTargets')}>
            <span className="text-sm" style={{ color: 'var(--text-secondary)' }}>
              {skill.engineTargets.length > 0 ? skill.engineTargets.join(', ') : t(locale, 'capabilities.common.none')}
            </span>
          </Row>
          <Row label={t(locale, 'capabilities.skills.scope')}>
            <span className="text-sm" style={{ color: 'var(--text-secondary)' }}>{skill.scope}</span>
          </Row>
          <Row label={t(locale, 'capabilities.skills.source')}>
            <span className="text-sm" style={{ color: 'var(--text-secondary)' }}>{skill.source}</span>
          </Row>
          <Row label={t(locale, 'capabilities.skills.path')}>
            <span className="break-all font-mono text-xs" style={{ color: 'var(--text-disabled)' }}>{skill.dirPath}</span>
          </Row>

          <Row label={t(locale, 'capabilities.skills.preview')}>
            {detail ? (
              <pre
                className="max-h-64 overflow-auto whitespace-pre-wrap rounded border p-2 text-xs"
                style={{ borderColor: 'var(--border-subtle)', color: 'var(--text-secondary)' }}
              >
                {detail.bodyPreview || t(locale, 'capabilities.common.none')}
              </pre>
            ) : (
              <Skeleton count={3} />
            )}
          </Row>
        </div>

        <div className="px-4 py-3" style={{ borderTop: '1px solid var(--border)' }}>
          {deleteMode === null ? (
            <button
              type="button"
              onClick={() => setDeleteMode('unregister')}
              className="w-full rounded-lg border px-3 py-2 text-sm"
              style={{ borderColor: 'var(--danger)', color: 'var(--danger)' }}
            >
              {t(locale, 'capabilities.skills.deleteTitle')}
            </button>
          ) : (
            <div className="space-y-2">
              <p className="text-xs" style={{ color: 'var(--text-secondary)' }}>
                {t(locale, 'capabilities.skills.deleteMessage')}
              </p>
              <div className="flex gap-2">
                <button
                  type="button"
                  onClick={() => void handleDelete('unregister')}
                  className="flex-1 rounded-lg border px-3 py-2 text-sm"
                  style={{ borderColor: 'var(--danger)', color: 'var(--danger)' }}
                >
                  {t(locale, 'capabilities.skills.deleteUnregister')}
                </button>
                <button
                  type="button"
                  onClick={() => void handleDelete('remove_dir')}
                  className="flex-1 rounded-lg px-3 py-2 text-sm"
                  style={{ background: 'var(--danger)', color: '#fff' }}
                >
                  {t(locale, 'capabilities.skills.deleteRemoveDir')}
                </button>
                <button
                  type="button"
                  onClick={() => setDeleteMode(null)}
                  className="rounded-lg px-3 py-2 text-sm"
                  style={{ color: 'var(--text-secondary)' }}
                >
                  {t(locale, 'capabilities.common.cancel')}
                </button>
              </div>
            </div>
          )}
        </div>
      </aside>
    </div>
  );
}
