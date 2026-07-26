'use client';

import { useState } from 'react';
import { ArrowDown, ArrowUp, Trash2 } from 'lucide-react';
import { t, type Locale } from '@/i18n';
import type { AssistantGateway } from '@/lib/assistant-gateway';
import {
  createCapabilityTeam,
  updateCapabilityTeam,
} from '@/lib/assistant-workspace/capability-admin';
import { classifyError } from '@/lib/error-classifier';
import Modal from '@/components/ui/Modal';
import { useToast } from '@/components/ui/Toast';
import type { CapabilityExpert, CapabilityExpertTeam, CapabilityTeamMember } from '../shared/capability-types';

interface ExpertTeamEditDialogProps {
  locale: Locale;
  gateway: AssistantGateway;
  /** null → create */
  team: CapabilityExpertTeam | null;
  experts: CapabilityExpert[];
  onClose: () => void;
  onSaved: () => void;
}

const inputStyle = {
  borderColor: 'var(--border)',
  background: 'var(--surface)',
  color: 'var(--text)',
} as const;

// Frozen daemon vocab (experts.rs::validate_team_settings).
const STRATEGIES = ['parallel', 'sequential', 'coordinator'] as const;
const FAILURE_POLICIES = ['isolate', 'fail_fast', 'require_all'] as const;

type MemberDraft = Omit<CapabilityTeamMember, 'position'>;

/** Team = lead + ordered member allowlist + failure policy (ADR-0016 决策 3). */
export default function ExpertTeamEditDialog({ locale, gateway, team, experts, onClose, onSaved }: ExpertTeamEditDialogProps) {
  const { toast } = useToast();
  const isEdit = team !== null;
  const [name, setName] = useState(team?.name ?? '');
  const [description, setDescription] = useState(team?.description ?? '');
  const [strategy, setStrategy] = useState(team?.strategy ?? 'parallel');
  const [failurePolicy, setFailurePolicy] = useState(team?.failurePolicy ?? 'isolate');
  const [maxConcurrent, setMaxConcurrent] = useState(team?.maxConcurrent ?? 2);
  const [coordinatorId, setCoordinatorId] = useState(team?.coordinatorExpertId ?? '');
  const [members, setMembers] = useState<MemberDraft[]>(
    (team?.members ?? [])
      .slice()
      .sort((a, b) => a.position - b.position)
      .map(({ expertId, roleHint, taskTemplate }) => ({ expertId, roleHint, taskTemplate })),
  );
  const [enabled, setEnabled] = useState(team?.enabled ?? true);
  const [addSelect, setAddSelect] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);

  const memberIds = new Set(members.map((m) => m.expertId));
  const addable = experts.filter((e) => !memberIds.has(e.id));
  const expertName = (id: string) => experts.find((e) => e.id === id)?.name ?? id;

  const move = (index: number, delta: number) => {
    const target = index + delta;
    if (target < 0 || target >= members.length) return;
    const next = members.slice();
    const [row] = next.splice(index, 1);
    next.splice(target, 0, row!);
    setMembers(next);
  };

  const patchMember = (index: number, patch: Partial<MemberDraft>) => {
    setMembers(members.map((m, i) => (i === index ? { ...m, ...patch } : m)));
  };

  const handleSave = async () => {
    if (!name.trim()) {
      setFormError(t(locale, 'capabilities.experts.nameRequired'));
      return;
    }
    if (members.length === 0) {
      setFormError(t(locale, 'capabilities.experts.memberRequired'));
      return;
    }
    setFormError(null);
    setSubmitting(true);
    const payload: Partial<CapabilityExpertTeam> = {
      name: name.trim(),
      description: description.trim(),
      strategy,
      failurePolicy,
      maxConcurrent,
      coordinatorExpertId: coordinatorId || null,
      enabled,
      members: members.map((m, index) => ({ ...m, position: index })),
    };
    try {
      if (isEdit && team) {
        await updateCapabilityTeam(gateway, team.id, payload);
        toast(t(locale, 'capabilities.experts.teamSaved'), 'success');
      } else {
        await createCapabilityTeam(gateway, payload);
        toast(t(locale, 'capabilities.experts.teamCreated'), 'success');
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
      title={t(locale, isEdit ? 'capabilities.experts.teamEditTitle' : 'capabilities.experts.teamCreateTitle')}
      width={600}
    >
      <div className="space-y-3">
        <input
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder={t(locale, 'capabilities.experts.teamName')}
          aria-label={t(locale, 'capabilities.experts.teamName')}
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
        <div className="grid grid-cols-3 gap-2">
          <label className="flex flex-col gap-1 text-xs" style={{ color: 'var(--text-secondary)' }}>
            {t(locale, 'capabilities.experts.strategy')}
            <select value={strategy} onChange={(e) => setStrategy(e.target.value)} className="rounded border px-2 py-1.5 text-sm" style={inputStyle}>
              {STRATEGIES.map((s) => (
                <option key={s} value={s}>{s}</option>
              ))}
            </select>
          </label>
          <label className="flex flex-col gap-1 text-xs" style={{ color: 'var(--text-secondary)' }}>
            {t(locale, 'capabilities.experts.failurePolicy')}
            <select value={failurePolicy} onChange={(e) => setFailurePolicy(e.target.value)} className="rounded border px-2 py-1.5 text-sm" style={inputStyle}>
              {FAILURE_POLICIES.map((p) => (
                <option key={p} value={p}>{p}</option>
              ))}
            </select>
          </label>
          <label className="flex flex-col gap-1 text-xs" style={{ color: 'var(--text-secondary)' }}>
            {t(locale, 'capabilities.experts.maxConcurrent')}
            <input
              type="number"
              min={1}
              max={16}
              value={maxConcurrent}
              onChange={(e) => setMaxConcurrent(Math.max(1, Number(e.target.value) || 1))}
              className="rounded border px-2 py-1.5 text-sm"
              style={inputStyle}
            />
          </label>
        </div>
        <label className="flex flex-col gap-1 text-xs" style={{ color: 'var(--text-secondary)' }}>
          {t(locale, 'capabilities.experts.coordinator')}
          <select value={coordinatorId} onChange={(e) => setCoordinatorId(e.target.value)} className="rounded border px-2 py-1.5 text-sm" style={inputStyle}>
            <option value="">{t(locale, 'capabilities.experts.coordinatorNone')}</option>
            {experts.map((e) => (
              <option key={e.id} value={e.id}>{e.name}</option>
            ))}
          </select>
        </label>

        {/* Members: pick, order, annotate */}
        <div className="rounded-lg border p-2.5" style={{ borderColor: 'var(--border-subtle)' }}>
          <span className="mb-1.5 block text-xs font-semibold uppercase" style={{ color: 'var(--text-secondary)' }}>
            {t(locale, 'capabilities.experts.members')}
          </span>
          <div className="space-y-1.5">
            {members.map((member, index) => (
              <div key={member.expertId} className="flex items-center gap-1.5">
                <span className="w-6 text-center font-mono text-xs" style={{ color: 'var(--text-disabled)' }}>{index + 1}</span>
                <span className="min-w-0 flex-1 truncate text-sm" style={{ color: 'var(--text)' }}>{expertName(member.expertId)}</span>
                <input
                  value={member.roleHint ?? ''}
                  onChange={(e) => patchMember(index, { roleHint: e.target.value || null })}
                  placeholder={t(locale, 'capabilities.experts.roleHint')}
                  aria-label={t(locale, 'capabilities.experts.roleHint')}
                  className="w-28 rounded border px-2 py-1 text-xs"
                  style={inputStyle}
                />
                <input
                  value={member.taskTemplate ?? ''}
                  onChange={(e) => patchMember(index, { taskTemplate: e.target.value || null })}
                  placeholder={t(locale, 'capabilities.experts.taskTemplate')}
                  aria-label={t(locale, 'capabilities.experts.taskTemplate')}
                  className="w-36 rounded border px-2 py-1 text-xs"
                  style={inputStyle}
                />
                <button type="button" onClick={() => move(index, -1)} disabled={index === 0} aria-label={t(locale, 'capabilities.experts.moveUp')} title={t(locale, 'capabilities.experts.moveUp')} className="rounded p-1 hover:bg-[var(--surface-hover)] disabled:opacity-30" style={{ color: 'var(--text-secondary)' }}>
                  <ArrowUp size={12} />
                </button>
                <button type="button" onClick={() => move(index, 1)} disabled={index === members.length - 1} aria-label={t(locale, 'capabilities.experts.moveDown')} title={t(locale, 'capabilities.experts.moveDown')} className="rounded p-1 hover:bg-[var(--surface-hover)] disabled:opacity-30" style={{ color: 'var(--text-secondary)' }}>
                  <ArrowDown size={12} />
                </button>
                <button type="button" onClick={() => setMembers(members.filter((_, i) => i !== index))} aria-label={t(locale, 'capabilities.experts.removeMember')} title={t(locale, 'capabilities.experts.removeMember')} className="rounded p-1 hover:bg-[var(--surface-hover)]" style={{ color: 'var(--danger)' }}>
                  <Trash2 size={12} />
                </button>
              </div>
            ))}
          </div>
          <div className="mt-2 flex items-center gap-2">
            <select
              value={addSelect}
              onChange={(e) => setAddSelect(e.target.value)}
              aria-label={t(locale, 'capabilities.experts.addMember')}
              className="flex-1 rounded border px-2 py-1.5 text-sm"
              style={inputStyle}
            >
              <option value="">{t(locale, 'capabilities.experts.addMember')}</option>
              {addable.map((e) => (
                <option key={e.id} value={e.id}>{e.name}</option>
              ))}
            </select>
            <button
              type="button"
              onClick={() => {
                if (!addSelect) return;
                setMembers([...members, { expertId: addSelect, roleHint: null, taskTemplate: null }]);
                setAddSelect('');
              }}
              disabled={!addSelect}
              className="rounded px-3 py-1.5 text-sm disabled:opacity-40"
              style={{ background: 'var(--surface-hover)', color: 'var(--text)' }}
            >
              {t(locale, 'capabilities.experts.addMember')}
            </button>
          </div>
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
