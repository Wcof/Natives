'use client';

import { Bot, Copy, Pencil, Trash2, Users } from 'lucide-react';
import { t, type Locale } from '@/i18n';
import type { AssistantGateway } from '@/lib/assistant-gateway';
import { exportCapabilityExpertMd } from '@/lib/assistant-workspace/capability-admin';
import { classifyError } from '@/lib/error-classifier';
import { useToast } from '@/components/ui/Toast';
import type { CapabilityExpert, CapabilityExpertTeam } from '../shared/capability-types';

interface ExpertListProps {
  locale: Locale;
  gateway: AssistantGateway;
  experts: CapabilityExpert[];
  teams: CapabilityExpertTeam[];
  onEditExpert: (expert: CapabilityExpert) => void;
  onDeleteExpert: (expert: CapabilityExpert) => void;
  onEditTeam: (team: CapabilityExpertTeam) => void;
  onDeleteTeam: (team: CapabilityExpertTeam) => void;
}

function SectionHeader({ label }: { label: string }) {
  return (
    <div className="px-1 pb-1 pt-3 text-xs font-semibold uppercase" style={{ color: 'var(--text-secondary)' }}>
      {label}
    </div>
  );
}

function IconButton({ label, onClick, danger, children }: {
  label: string;
  onClick: () => void;
  danger?: boolean;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-label={label}
      title={label}
      className="rounded p-1.5 hover:bg-[var(--surface-hover)]"
      style={{ color: danger ? 'var(--danger)' : 'var(--text-secondary)' }}
    >
      {children}
    </button>
  );
}

/** Grouped list: 单专家 and 专家团. */
export default function ExpertList({
  locale, gateway, experts, teams, onEditExpert, onDeleteExpert, onEditTeam, onDeleteTeam,
}: ExpertListProps) {
  const { toast } = useToast();

  const handleExport = async (expert: CapabilityExpert) => {
    try {
      const content = await exportCapabilityExpertMd(gateway, expert.id);
      await navigator.clipboard.writeText(content);
      toast(t(locale, 'capabilities.experts.exportCopied'), 'success');
    } catch (e) {
      toast(classifyError(e).userMessage, 'error');
    }
  };

  const expertName = (id: string) => experts.find((e) => e.id === id)?.name ?? id;

  return (
    <div className="flex flex-col gap-1">
      {experts.length > 0 ? <SectionHeader label={t(locale, 'capabilities.experts.single')} /> : null}
      {experts.map((expert) => (
        <div
          key={expert.id}
          className="flex items-center gap-2.5 rounded-lg border px-3 py-2"
          style={{ borderColor: 'var(--border-subtle)', background: 'var(--surface)' }}
        >
          <Bot size={16} className="shrink-0" style={{ color: 'var(--primary)' }} aria-hidden />
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-2">
              <span className="truncate text-sm font-medium" style={{ color: 'var(--text)' }}>{expert.name}</span>
              <span
                className="rounded-full border px-1.5 py-0.5 text-[10px] leading-none"
                style={{
                  borderColor: 'var(--border-subtle)',
                  color: expert.enabled ? 'var(--success, #10b981)' : 'var(--text-disabled)',
                }}
              >
                {expert.enabled ? t(locale, 'capabilities.common.enabled') : t(locale, 'capabilities.common.disabled')}
              </span>
              {expert.modelId ? (
                <span className="truncate font-mono text-[10px]" style={{ color: 'var(--text-disabled)' }}>
                  {expert.modelId}
                </span>
              ) : null}
            </div>
            {expert.description ? (
              <p className="mt-0.5 truncate text-xs" style={{ color: 'var(--text-secondary)' }}>{expert.description}</p>
            ) : null}
          </div>
          <IconButton label={t(locale, 'capabilities.experts.exportMd')} onClick={() => void handleExport(expert)}>
            <Copy size={14} />
          </IconButton>
          <IconButton label={t(locale, 'capabilities.common.edit')} onClick={() => onEditExpert(expert)}>
            <Pencil size={14} />
          </IconButton>
          <IconButton label={t(locale, 'capabilities.common.delete')} onClick={() => onDeleteExpert(expert)} danger>
            <Trash2 size={14} />
          </IconButton>
        </div>
      ))}

      {teams.length > 0 ? <SectionHeader label={t(locale, 'capabilities.experts.teams')} /> : null}
      {teams.map((team) => (
        <div
          key={team.id}
          className="flex items-center gap-2.5 rounded-lg border px-3 py-2"
          style={{ borderColor: 'var(--border-subtle)', background: 'var(--surface)' }}
        >
          <Users size={16} className="shrink-0" style={{ color: 'var(--primary)' }} aria-hidden />
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-2">
              <span className="truncate text-sm font-medium" style={{ color: 'var(--text)' }}>{team.name}</span>
              <span
                className="rounded-full border px-1.5 py-0.5 text-[10px] leading-none"
                style={{
                  borderColor: 'var(--border-subtle)',
                  color: team.enabled ? 'var(--success, #10b981)' : 'var(--text-disabled)',
                }}
              >
                {team.enabled ? t(locale, 'capabilities.common.enabled') : t(locale, 'capabilities.common.disabled')}
              </span>
              <span className="text-[10px]" style={{ color: 'var(--text-disabled)' }}>{team.strategy}</span>
            </div>
            <p className="mt-0.5 truncate text-xs" style={{ color: 'var(--text-secondary)' }}>
              {team.members
                .slice()
                .sort((a, b) => a.position - b.position)
                .map((member) => expertName(member.expertId))
                .join(' · ')}
            </p>
          </div>
          <IconButton label={t(locale, 'capabilities.common.edit')} onClick={() => onEditTeam(team)}>
            <Pencil size={14} />
          </IconButton>
          <IconButton label={t(locale, 'capabilities.common.delete')} onClick={() => onDeleteTeam(team)} danger>
            <Trash2 size={14} />
          </IconButton>
        </div>
      ))}
    </div>
  );
}
