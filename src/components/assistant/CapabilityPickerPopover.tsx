'use client';

import { useEffect, useRef, useState } from 'react';
import { Check, X } from 'lucide-react';
import { t, type Locale } from '@/i18n';
import type { AssistantGateway } from '@/lib/assistant-gateway';
import type { CapabilitySelection } from '@/lib/assistant-protocol';
import {
  listCapabilityExperts,
  listCapabilityMcpServers,
  listCapabilitySkills,
  listCapabilityTeams,
} from '@/lib/assistant-workspace/capability-admin';
import { classifyError } from '@/lib/error-classifier';
import { LoadingState } from '@/components/ui/EmptyState';

interface CapabilityPickerPopoverProps {
  locale: Locale;
  gateway: AssistantGateway;
  selection: CapabilitySelection | null;
  onChange: (selection: CapabilitySelection | null) => void;
  onClose: () => void;
}

interface Option {
  id: string;
  name: string;
  description?: string;
}

function normalize(selection: CapabilitySelection): CapabilitySelection | null {
  const empty =
    (selection.skills?.length ?? 0) === 0 &&
    (selection.mcp_servers?.length ?? 0) === 0 &&
    !selection.expert_id &&
    !selection.team_id;
  return empty ? null : selection;
}

/**
 * Composer capability picker (ADR-0016 会话选用): skills multi-select,
 * trusted+enabled connectors multi-select, expert OR team single-select.
 * Rendered lazily from AssistantWorkbench — keep this module out of the
 * initial bundle.
 */
export default function CapabilityPickerPopover({ locale, gateway, selection, onChange, onClose }: CapabilityPickerPopoverProps) {
  const [skills, setSkills] = useState<Option[]>([]);
  const [connectors, setConnectors] = useState<Option[]>([]);
  const [experts, setExperts] = useState<Option[]>([]);
  const [teams, setTeams] = useState<Option[]>([]);
  const [phase, setPhase] = useState<'loading' | 'error' | 'ready'>('loading');
  const [error, setError] = useState<string | null>(null);
  const rootRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const [skillList, mcpList, expertList, teamList] = await Promise.all([
          listCapabilitySkills(gateway, { enabledOnly: true }),
          listCapabilityMcpServers(gateway),
          listCapabilityExperts(gateway, { enabledOnly: true }),
          listCapabilityTeams(gateway),
        ]);
        if (cancelled) return;
        setSkills(skillList.map((s) => ({ id: s.id, name: s.name, description: s.description })));
        // Selection contract: only trusted AND enabled connectors are selectable.
        setConnectors(
          mcpList.filter((m) => m.trusted && m.enabled).map((m) => ({ id: m.id, name: m.name })),
        );
        setExperts(expertList.map((e) => ({ id: e.id, name: e.name, description: e.description })));
        setTeams(teamList.filter((team) => team.enabled).map((team) => ({ id: team.id, name: team.name })));
        setPhase('ready');
      } catch (e) {
        if (cancelled) return;
        setError(classifyError(e).userMessage);
        setPhase('error');
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [gateway]);

  // Click outside → close (the popover lives inside the composer container).
  useEffect(() => {
    const handler = (event: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(event.target as Node)) onClose();
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, [onClose]);

  const current: CapabilitySelection = selection ?? {};
  const selectedSkills = current.skills ?? [];
  const selectedConnectors = current.mcp_servers ?? [];
  const count =
    selectedSkills.length + selectedConnectors.length + (current.expert_id ? 1 : 0) + (current.team_id ? 1 : 0);

  const toggleSkill = (id: string) => {
    const next = selectedSkills.includes(id)
      ? selectedSkills.filter((s) => s !== id)
      : [...selectedSkills, id];
    onChange(normalize({ ...current, skills: next }));
  };

  const toggleConnector = (id: string) => {
    const next = selectedConnectors.includes(id)
      ? selectedConnectors.filter((s) => s !== id)
      : [...selectedConnectors, id];
    onChange(normalize({ ...current, mcp_servers: next }));
  };

  // Expert and team are mutually exclusive.
  const selectExpert = (id: string | null) => {
    onChange(normalize({ ...current, expert_id: id, team_id: null }));
  };
  const selectTeam = (id: string | null) => {
    onChange(normalize({ ...current, expert_id: null, team_id: id }));
  };

  const chip = (active: boolean, label: string, onClick: () => void, key: string) => (
    <button
      key={key}
      type="button"
      role="checkbox"
      aria-checked={active}
      onClick={onClick}
      className="flex items-center gap-1 rounded-full border px-2.5 py-1 text-xs"
      style={{
        borderColor: active ? 'var(--primary)' : 'var(--border-subtle)',
        background: active ? 'var(--primary)' : 'transparent',
        color: active ? 'var(--accent-ink)' : 'var(--text-secondary)',
      }}
    >
      {active ? <Check size={11} aria-hidden /> : null}
      {label}
    </button>
  );

  return (
    <div
      ref={rootRef}
      role="dialog"
      aria-label={t(locale, 'capabilities.picker.title')}
      className="absolute bottom-full left-0 z-50 mb-2 w-[420px] max-w-[92vw] rounded-xl border p-3 shadow-popup"
      style={{ borderColor: 'var(--border)', background: 'var(--surface)' }}
    >
      <div className="mb-2 flex items-center justify-between">
        <span className="text-sm font-semibold" style={{ color: 'var(--text)' }}>
          {t(locale, 'capabilities.picker.title')}
        </span>
        <div className="flex items-center gap-2">
          <span className="text-xs" style={{ color: 'var(--text-disabled)' }}>
            {t(locale, 'capabilities.picker.selectedCount', { count })}
          </span>
          {count > 0 ? (
            <button type="button" onClick={() => onChange(null)} className="text-xs" style={{ color: 'var(--primary)' }}>
              {t(locale, 'capabilities.picker.clear')}
            </button>
          ) : null}
          <button
            type="button"
            onClick={onClose}
            aria-label={t(locale, 'capabilities.common.close')}
            className="rounded p-0.5 hover:bg-[var(--surface-hover)]"
            style={{ color: 'var(--text-secondary)' }}
          >
            <X size={14} />
          </button>
        </div>
      </div>

      {phase === 'loading' ? (
        <LoadingState message={t(locale, 'capabilities.common.loading')} />
      ) : phase === 'error' ? (
        <p className="py-2 text-sm" style={{ color: 'var(--danger)' }}>
          {t(locale, 'capabilities.picker.loadFailed')}
          {error ? ` — ${error}` : ''}
        </p>
      ) : (
        <div className="max-h-[320px] space-y-3 overflow-y-auto">
          <section>
            <h3 className="mb-1.5 text-xs font-semibold uppercase" style={{ color: 'var(--text-secondary)' }}>
              {t(locale, 'capabilities.picker.skillsSection')}
            </h3>
            {skills.length === 0 ? (
              <p className="text-xs" style={{ color: 'var(--text-disabled)' }}>{t(locale, 'capabilities.picker.noSkills')}</p>
            ) : (
              <div className="flex flex-wrap gap-1.5">
                {skills.map((s) => chip(selectedSkills.includes(s.id), s.name, () => toggleSkill(s.id), `skill-${s.id}`))}
              </div>
            )}
          </section>

          <section>
            <h3 className="mb-1.5 text-xs font-semibold uppercase" style={{ color: 'var(--text-secondary)' }}>
              {t(locale, 'capabilities.picker.connectorsSection')}
            </h3>
            {connectors.length === 0 ? (
              <p className="text-xs" style={{ color: 'var(--text-disabled)' }}>{t(locale, 'capabilities.picker.noConnectors')}</p>
            ) : (
              <div className="flex flex-wrap gap-1.5">
                {connectors.map((c) =>
                  chip(selectedConnectors.includes(c.id), c.name, () => toggleConnector(c.id), `mcp-${c.id}`),
                )}
              </div>
            )}
          </section>

          <section>
            <h3 className="mb-1.5 text-xs font-semibold uppercase" style={{ color: 'var(--text-secondary)' }}>
              {t(locale, 'capabilities.picker.expertSection')}
            </h3>
            {experts.length === 0 && teams.length === 0 ? (
              <p className="text-xs" style={{ color: 'var(--text-disabled)' }}>{t(locale, 'capabilities.picker.noExperts')}</p>
            ) : (
              <div className="flex flex-wrap gap-1.5">
                {chip(!current.expert_id && !current.team_id, t(locale, 'capabilities.picker.expertNone'), () => selectExpert(null), 'expert-none')}
                {experts.map((e) =>
                  chip(current.expert_id === e.id, e.name, () => selectExpert(current.expert_id === e.id ? null : e.id), `expert-${e.id}`),
                )}
                {teams.map((team) =>
                  chip(
                    current.team_id === team.id,
                    `${t(locale, 'capabilities.picker.teamTag')} · ${team.name}`,
                    () => selectTeam(current.team_id === team.id ? null : team.id),
                    `team-${team.id}`,
                  ),
                )}
              </div>
            )}
          </section>
        </div>
      )}
    </div>
  );
}
