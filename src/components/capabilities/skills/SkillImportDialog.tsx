'use client';

import { useState } from 'react';
import { t, type Locale } from '@/i18n';
import type { AssistantGateway } from '@/lib/assistant-gateway';
import { importCapabilitySkill } from '@/lib/assistant-workspace/capability-admin';
import { classifyError } from '@/lib/error-classifier';
import Modal from '@/components/ui/Modal';
import { useToast } from '@/components/ui/Toast';
import { SKILL_CATEGORIES } from '../shared/capability-types';

interface SkillImportDialogProps {
  locale: Locale;
  gateway: AssistantGateway;
  open: boolean;
  onClose: () => void;
  onImported: () => void;
}

/** Import a skill from a zip archive or a local directory path. */
export default function SkillImportDialog({ locale, gateway, open, onClose, onImported }: SkillImportDialogProps) {
  const { toast } = useToast();
  const [source, setSource] = useState<'zip' | 'dir'>('zip');
  const [path, setPath] = useState('');
  const [name, setName] = useState('');
  const [category, setCategory] = useState('');
  const [tags, setTags] = useState('');
  const [linkClaudeDir, setLinkClaudeDir] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);

  const inputStyle = {
    borderColor: 'var(--border)',
    background: 'var(--surface)',
    color: 'var(--text)',
  } as const;

  const handleImport = async () => {
    if (!path.trim()) {
      setFormError(t(locale, 'capabilities.skills.pathRequired'));
      return;
    }
    setFormError(null);
    setSubmitting(true);
    try {
      await importCapabilitySkill(gateway, {
        source,
        path: path.trim(),
        ...(name.trim() ? { name: name.trim() } : {}),
        ...(category ? { category } : {}),
        ...(tags.trim()
          ? { tags: tags.split(',').map((s) => s.trim()).filter(Boolean) }
          : {}),
        ...(linkClaudeDir ? { linkClaudeDir: true } : {}),
      });
      toast(t(locale, 'capabilities.skills.imported'), 'success');
      setPath('');
      setName('');
      setCategory('');
      setTags('');
      setLinkClaudeDir(false);
      onImported();
    } catch (e) {
      setFormError(classifyError(e).userMessage);
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Modal isOpen={open} onClose={onClose} title={t(locale, 'capabilities.skills.importTitle')} width={480}>
      <div className="space-y-3">
        <div className="flex flex-col gap-1">
          <span className="text-xs font-semibold uppercase" style={{ color: 'var(--text-secondary)' }}>
            {t(locale, 'capabilities.skills.importSource')}
          </span>
          <div className="flex gap-2" role="radiogroup" aria-label={t(locale, 'capabilities.skills.importSource')}>
            {(['zip', 'dir'] as const).map((option) => (
              <button
                key={option}
                type="button"
                role="radio"
                aria-checked={source === option}
                onClick={() => setSource(option)}
                className="rounded-lg border px-3 py-1.5 text-sm"
                style={{
                  borderColor: source === option ? 'var(--primary)' : 'var(--border-subtle)',
                  color: source === option ? 'var(--primary)' : 'var(--text-secondary)',
                }}
              >
                {t(locale, option === 'zip' ? 'capabilities.skills.importSourceZip' : 'capabilities.skills.importSourceDir')}
              </button>
            ))}
          </div>
        </div>

        <input
          value={path}
          onChange={(e) => setPath(e.target.value)}
          placeholder={t(locale, 'capabilities.skills.importPathPlaceholder')}
          aria-label={t(locale, 'capabilities.skills.importPath')}
          className="w-full rounded border px-3 py-2 text-sm"
          style={inputStyle}
          autoFocus
        />
        <input
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder={t(locale, 'capabilities.skills.importName')}
          aria-label={t(locale, 'capabilities.skills.importName')}
          className="w-full rounded border px-3 py-2 text-sm"
          style={inputStyle}
        />
        <select
          value={category}
          onChange={(e) => setCategory(e.target.value)}
          aria-label={t(locale, 'capabilities.skills.importCategory')}
          className="w-full rounded border px-3 py-2 text-sm"
          style={inputStyle}
        >
          <option value="">{t(locale, 'capabilities.skills.importCategory')}</option>
          {SKILL_CATEGORIES.map((c) => (
            <option key={c} value={c}>{t(locale, `capabilities.categories.${c}`)}</option>
          ))}
        </select>
        <input
          value={tags}
          onChange={(e) => setTags(e.target.value)}
          placeholder={t(locale, 'capabilities.skills.tagsPlaceholder')}
          aria-label={t(locale, 'capabilities.skills.tags')}
          className="w-full rounded border px-3 py-2 text-sm"
          style={inputStyle}
        />
        <label className="flex items-center gap-2 text-sm" style={{ color: 'var(--text)' }}>
          <input type="checkbox" checked={linkClaudeDir} onChange={(e) => setLinkClaudeDir(e.target.checked)} />
          {t(locale, 'capabilities.skills.importLinkClaude')}
        </label>

        {formError ? (
          <p className="text-sm" style={{ color: 'var(--danger)' }}>{formError}</p>
        ) : null}

        <div className="flex justify-end gap-2 pt-1">
          <button type="button" onClick={onClose} className="px-4 py-2 text-sm" style={{ color: 'var(--text-secondary)' }}>
            {t(locale, 'capabilities.common.cancel')}
          </button>
          <button
            type="button"
            onClick={() => void handleImport()}
            disabled={submitting}
            className="rounded px-4 py-2 text-sm disabled:opacity-50"
            style={{ background: 'var(--primary)', color: '#fff' }}
          >
            {t(locale, 'capabilities.common.import')}
          </button>
        </div>
      </div>
    </Modal>
  );
}
