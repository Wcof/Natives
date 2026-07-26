'use client';

import { useState } from 'react';
import { t, type Locale } from '@/i18n';
import type { AssistantGateway } from '@/lib/assistant-gateway';
import {
  createCapabilityExpert,
  updateCapabilityExpert,
} from '@/lib/assistant-workspace/capability-admin';
import { classifyError } from '@/lib/error-classifier';
import Modal from '@/components/ui/Modal';
import { useToast } from '@/components/ui/Toast';
import type { CapabilityExpert, CapabilitySkill } from '../shared/capability-types';

interface ExpertEditFormProps {
  locale: Locale;
  gateway: AssistantGateway;
  /** null → create */
  expert: CapabilityExpert | null;
  /** From capability.skill.list — the data source for the skills binding. */
  skills: CapabilitySkill[];
  onClose: () => void;
  onSaved: () => void;
}

const inputStyle = {
  borderColor: 'var(--border)',
  background: 'var(--surface)',
  color: 'var(--text)',
} as const;

const PERMISSION_MODES = ['', 'readonly', 'ask', 'full_access'] as const;

const splitList = (text: string): string[] => text.split(',').map((s) => s.trim()).filter(Boolean);

/** Expert form incl. multi-select skill binding (data source: capability.skill.list). */
export default function ExpertEditForm({ locale, gateway, expert, skills, onClose, onSaved }: ExpertEditFormProps) {
  const { toast } = useToast();
  const isEdit = expert !== null;
  const [name, setName] = useState(expert?.name ?? '');
  const [description, setDescription] = useState(expert?.description ?? '');
  const [systemPrompt, setSystemPrompt] = useState(expert?.systemPrompt ?? '');
  const [toolsText, setToolsText] = useState((expert?.tools ?? []).join(', '));
  const [disallowedText, setDisallowedText] = useState((expert?.disallowedTools ?? []).join(', '));
  const [permissionMode, setPermissionMode] = useState(expert?.permissionMode ?? '');
  const [selectedSkills, setSelectedSkills] = useState<string[]>(expert?.skills ?? []);
  const [providerId, setProviderId] = useState(expert?.providerId ?? '');
  const [keyId, setKeyId] = useState(expert?.keyId ?? '');
  const [modelId, setModelId] = useState(expert?.modelId ?? '');
  const [enabled, setEnabled] = useState(expert?.enabled ?? true);
  const [submitting, setSubmitting] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);

  const toggleSkill = (id: string) => {
    setSelectedSkills((prev) => (prev.includes(id) ? prev.filter((s) => s !== id) : [...prev, id]));
  };

  const handleSave = async () => {
    if (!name.trim()) {
      setFormError(t(locale, 'capabilities.experts.nameRequired'));
      return;
    }
    setFormError(null);
    setSubmitting(true);
    const payload: Partial<CapabilityExpert> = {
      name: name.trim(),
      description: description.trim(),
      systemPrompt,
      tools: splitList(toolsText),
      disallowedTools: splitList(disallowedText),
      permissionMode: permissionMode || null,
      skills: selectedSkills,
      providerId: providerId.trim() || null,
      keyId: keyId.trim() || null,
      modelId: modelId.trim() || null,
      enabled,
    };
    try {
      if (isEdit && expert) {
        await updateCapabilityExpert(gateway, expert.id, payload);
        toast(t(locale, 'capabilities.experts.saved'), 'success');
      } else {
        await createCapabilityExpert(gateway, payload);
        toast(t(locale, 'capabilities.experts.created'), 'success');
      }
      onSaved();
    } catch (e) {
      setFormError(classifyError(e).userMessage);
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Modal
      isOpen
      onClose={onClose}
      title={t(locale, isEdit ? 'capabilities.experts.editTitle' : 'capabilities.experts.createTitle')}
      width={560}
    >
      <div className="space-y-3">
        <input
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder={t(locale, 'capabilities.experts.name')}
          aria-label={t(locale, 'capabilities.experts.name')}
          className="w-full rounded border px-3 py-2 text-sm"
          style={inputStyle}
          autoFocus
        />
        <input
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          placeholder={t(locale, 'capabilities.experts.description')}
          aria-label={t(locale, 'capabilities.experts.description')}
          className="w-full rounded border px-3 py-2 text-sm"
          style={inputStyle}
        />
        <textarea
          value={systemPrompt}
          onChange={(e) => setSystemPrompt(e.target.value)}
          placeholder={t(locale, 'capabilities.experts.systemPrompt')}
          aria-label={t(locale, 'capabilities.experts.systemPrompt')}
          className="h-28 w-full resize-none rounded border px-3 py-2 text-sm"
          style={inputStyle}
        />
        <input
          value={toolsText}
          onChange={(e) => setToolsText(e.target.value)}
          placeholder={t(locale, 'capabilities.experts.tools')}
          aria-label={t(locale, 'capabilities.experts.tools')}
          className="w-full rounded border px-3 py-2 font-mono text-xs"
          style={inputStyle}
        />
        <input
          value={disallowedText}
          onChange={(e) => setDisallowedText(e.target.value)}
          placeholder={t(locale, 'capabilities.experts.disallowedTools')}
          aria-label={t(locale, 'capabilities.experts.disallowedTools')}
          className="w-full rounded border px-3 py-2 font-mono text-xs"
          style={inputStyle}
        />
        <select
          value={permissionMode ?? ''}
          onChange={(e) => setPermissionMode(e.target.value)}
          aria-label={t(locale, 'capabilities.experts.permissionMode')}
          className="w-full rounded border px-3 py-2 text-sm"
          style={inputStyle}
        >
          {PERMISSION_MODES.map((mode) => (
            <option key={mode} value={mode}>
              {mode === '' ? t(locale, 'capabilities.experts.permissionMode') : mode}
            </option>
          ))}
        </select>

        {/* Skills binding — multi-select from the capability skill store */}
        <div className="rounded-lg border p-2.5" style={{ borderColor: 'var(--border-subtle)' }}>
          <span className="mb-1.5 block text-xs font-semibold uppercase" style={{ color: 'var(--text-secondary)' }}>
            {t(locale, 'capabilities.experts.skillsBinding')}
          </span>
          {skills.length === 0 ? (
            <p className="text-xs" style={{ color: 'var(--text-disabled)' }}>
              {t(locale, 'capabilities.experts.noSkillsAvailable')}
            </p>
          ) : (
            <div className="flex max-h-32 flex-wrap gap-1.5 overflow-y-auto">
              {skills.map((skill) => {
                const active = selectedSkills.includes(skill.id);
                return (
                  <button
                    key={skill.id}
                    type="button"
                    role="checkbox"
                    aria-checked={active}
                    onClick={() => toggleSkill(skill.id)}
                    className="rounded-full border px-2.5 py-1 text-xs"
                    style={{
                      borderColor: active ? 'var(--primary)' : 'var(--border-subtle)',
                      background: active ? 'var(--primary)' : 'transparent',
                      color: active ? '#fff' : 'var(--text-secondary)',
                    }}
                  >
                    {skill.name}
                  </button>
                );
              })}
            </div>
          )}
        </div>

        <div className="grid grid-cols-3 gap-2">
          <input
            value={providerId}
            onChange={(e) => setProviderId(e.target.value)}
            placeholder={t(locale, 'capabilities.experts.provider')}
            aria-label={t(locale, 'capabilities.experts.provider')}
            className="rounded border px-3 py-2 font-mono text-xs"
            style={inputStyle}
          />
          <input
            value={keyId}
            onChange={(e) => setKeyId(e.target.value)}
            placeholder={t(locale, 'capabilities.experts.keyId')}
            aria-label={t(locale, 'capabilities.experts.keyId')}
            className="rounded border px-3 py-2 font-mono text-xs"
            style={inputStyle}
          />
          <input
            value={modelId}
            onChange={(e) => setModelId(e.target.value)}
            placeholder={t(locale, 'capabilities.experts.model')}
            aria-label={t(locale, 'capabilities.experts.model')}
            className="rounded border px-3 py-2 font-mono text-xs"
            style={inputStyle}
          />
        </div>

        <label className="flex items-center justify-between gap-2 text-sm" style={{ color: 'var(--text)' }}>
          {t(locale, 'capabilities.experts.enabledLabel')}
          <input type="checkbox" checked={enabled} onChange={(e) => setEnabled(e.target.checked)} />
        </label>

        {formError ? <p className="text-sm" style={{ color: 'var(--danger)' }}>{formError}</p> : null}

        <div className="flex justify-end gap-2 pt-1">
          <button type="button" onClick={onClose} className="px-4 py-2 text-sm" style={{ color: 'var(--text-secondary)' }}>
            {t(locale, 'capabilities.common.cancel')}
          </button>
          <button
            type="button"
            onClick={() => void handleSave()}
            disabled={submitting}
            className="rounded px-4 py-2 text-sm disabled:opacity-50"
            style={{ background: 'var(--primary)', color: '#fff' }}
          >
            {t(locale, 'capabilities.common.save')}
          </button>
        </div>
      </div>
    </Modal>
  );
}
