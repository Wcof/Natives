'use client';

import { useCallback, useEffect, useState } from 'react';
import { FileText, Plus, Users } from 'lucide-react';
import { t, type Locale } from '@/i18n';
import type { AssistantGateway } from '@/lib/assistant-gateway';
import {
  deleteCapabilityExpert,
  deleteCapabilityTeam,
  importCapabilityExpertMd,
  listCapabilityExperts,
  listCapabilitySkills,
  listCapabilityTeams,
} from '@/lib/assistant-workspace/capability-admin';
import { classifyError } from '@/lib/error-classifier';
import Modal from '@/components/ui/Modal';
import ConfirmDialog from '@/components/ui/ConfirmDialog';
import { EmptyState, ErrorState, LoadingState } from '@/components/ui/EmptyState';
import { useToast } from '@/components/ui/Toast';
import type { CapabilityExpert, CapabilityExpertTeam, CapabilitySkill } from '../shared/capability-types';
import ExpertList from './ExpertList';
import ExpertEditForm from './ExpertEditForm';
import ExpertTeamEditDialog from './ExpertTeamEditDialog';

interface ExpertsTabProps {
  locale: Locale;
  gateway: AssistantGateway;
}

export default function ExpertsTab({ locale, gateway }: ExpertsTabProps) {
  const { toast } = useToast();
  const [experts, setExperts] = useState<CapabilityExpert[]>([]);
  const [teams, setTeams] = useState<CapabilityExpertTeam[]>([]);
  const [skills, setSkills] = useState<CapabilitySkill[]>([]);
  const [phase, setPhase] = useState<'loading' | 'error' | 'ready'>('loading');
  const [error, setError] = useState<string | null>(null);
  const [editingExpert, setEditingExpert] = useState<CapabilityExpert | null>(null);
  const [creatingExpert, setCreatingExpert] = useState(false);
  const [editingTeam, setEditingTeam] = useState<CapabilityExpertTeam | null>(null);
  const [creatingTeam, setCreatingTeam] = useState(false);
  const [importOpen, setImportOpen] = useState(false);
  const [importText, setImportText] = useState('');
  const [importError, setImportError] = useState<string | null>(null);
  const [confirmDeleteExpert, setConfirmDeleteExpert] = useState<CapabilityExpert | null>(null);
  const [forceDeleteExpert, setForceDeleteExpert] = useState<CapabilityExpert | null>(null);
  const [confirmDeleteTeam, setConfirmDeleteTeam] = useState<CapabilityExpertTeam | null>(null);

  const load = useCallback(async () => {
    setPhase('loading');
    setError(null);
    try {
      const [expertList, teamList, skillList] = await Promise.all([
        listCapabilityExperts(gateway),
        listCapabilityTeams(gateway),
        listCapabilitySkills(gateway).catch(() => [] as CapabilitySkill[]),
      ]);
      setExperts(expertList);
      setTeams(teamList);
      setSkills(skillList);
      setPhase('ready');
    } catch (e) {
      setError(classifyError(e).userMessage);
      setPhase('error');
    }
  }, [gateway]);

  useEffect(() => {
    void load();
  }, [load]);

  const handleDeleteExpert = useCallback(
    async (expert: CapabilityExpert, force: boolean) => {
      try {
        await deleteCapabilityExpert(gateway, expert.id, force);
        toast(t(locale, 'capabilities.experts.deleted'), 'success');
        void load();
      } catch (e) {
        if (!force) {
          // Referenced by a team → daemon rejects; offer force delete.
          setForceDeleteExpert(expert);
          return;
        }
        toast(classifyError(e).userMessage, 'error');
      }
    },
    [gateway, load, locale, toast],
  );

  const handleDeleteTeam = useCallback(async () => {
    if (!confirmDeleteTeam) return;
    try {
      await deleteCapabilityTeam(gateway, confirmDeleteTeam.id);
      toast(t(locale, 'capabilities.experts.teamDeleted'), 'success');
      void load();
    } catch (e) {
      toast(classifyError(e).userMessage, 'error');
    } finally {
      setConfirmDeleteTeam(null);
    }
  }, [confirmDeleteTeam, gateway, load, locale, toast]);

  const handleImportMd = useCallback(async () => {
    if (!importText.trim()) {
      setImportError(t(locale, 'capabilities.experts.importContentRequired'));
      return;
    }
    setImportError(null);
    try {
      await importCapabilityExpertMd(gateway, { content: importText });
      toast(t(locale, 'capabilities.experts.imported'), 'success');
      setImportOpen(false);
      setImportText('');
      void load();
    } catch (e) {
      setImportError(classifyError(e).userMessage);
    }
  }, [gateway, importText, load, locale, toast]);

  return (
    <div className="flex h-full flex-col gap-3 overflow-hidden">
      <div className="flex flex-wrap items-center gap-2">
        <button
          type="button"
          onClick={() => setCreatingExpert(true)}
          className="flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-sm"
          style={{ background: 'var(--primary)', color: '#fff' }}
        >
          <Plus size={14} aria-hidden />
          {t(locale, 'capabilities.experts.create')}
        </button>
        <button
          type="button"
          onClick={() => setCreatingTeam(true)}
          className="flex items-center gap-1.5 rounded-lg border px-3 py-1.5 text-sm"
          style={{ borderColor: 'var(--border-subtle)', color: 'var(--text-secondary)' }}
        >
          <Users size={14} aria-hidden />
          {t(locale, 'capabilities.experts.createTeam')}
        </button>
        <button
          type="button"
          onClick={() => setImportOpen(true)}
          className="flex items-center gap-1.5 rounded-lg border px-3 py-1.5 text-sm"
          style={{ borderColor: 'var(--border-subtle)', color: 'var(--text-secondary)' }}
        >
          <FileText size={14} aria-hidden />
          {t(locale, 'capabilities.experts.importMd')}
        </button>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto">
        {phase === 'loading' ? (
          <LoadingState message={t(locale, 'capabilities.common.loading')} />
        ) : phase === 'error' ? (
          <ErrorState message={error ?? t(locale, 'capabilities.experts.loadFailed')} onRetry={() => void load()} />
        ) : experts.length === 0 && teams.length === 0 ? (
          <EmptyState
            title={t(locale, 'capabilities.experts.empty')}
            description={t(locale, 'capabilities.experts.emptyDesc')}
            action={{ label: t(locale, 'capabilities.experts.create'), onClick: () => setCreatingExpert(true) }}
          />
        ) : (
          <ExpertList
            locale={locale}
            gateway={gateway}
            experts={experts}
            teams={teams}
            onEditExpert={setEditingExpert}
            onDeleteExpert={setConfirmDeleteExpert}
            onEditTeam={setEditingTeam}
            onDeleteTeam={setConfirmDeleteTeam}
          />
        )}
      </div>

      {(creatingExpert || editingExpert) && (
        <ExpertEditForm
          locale={locale}
          gateway={gateway}
          expert={editingExpert}
          skills={skills}
          onClose={() => {
            setCreatingExpert(false);
            setEditingExpert(null);
          }}
          onSaved={() => {
            setCreatingExpert(false);
            setEditingExpert(null);
            void load();
          }}
        />
      )}
      {(creatingTeam || editingTeam) && (
        <ExpertTeamEditDialog
          locale={locale}
          gateway={gateway}
          team={editingTeam}
          experts={experts}
          onClose={() => {
            setCreatingTeam(false);
            setEditingTeam(null);
          }}
          onSaved={() => {
            setCreatingTeam(false);
            setEditingTeam(null);
            void load();
          }}
        />
      )}

      <Modal
        isOpen={importOpen}
        onClose={() => setImportOpen(false)}
        title={t(locale, 'capabilities.experts.importTitle')}
        width={520}
      >
        <div className="space-y-3">
          <textarea
            value={importText}
            onChange={(e) => setImportText(e.target.value)}
            placeholder={t(locale, 'capabilities.experts.importPlaceholder')}
            aria-label={t(locale, 'capabilities.experts.importTitle')}
            className="h-48 w-full resize-none rounded border p-3 font-mono text-xs"
            style={{ borderColor: 'var(--border)', background: 'var(--surface)', color: 'var(--text)' }}
            autoFocus
          />
          {importError ? <p className="text-sm" style={{ color: 'var(--danger)' }}>{importError}</p> : null}
          <div className="flex justify-end gap-2">
            <button
              type="button"
              onClick={() => setImportOpen(false)}
              className="px-4 py-2 text-sm"
              style={{ color: 'var(--text-secondary)' }}
            >
              {t(locale, 'capabilities.common.cancel')}
            </button>
            <button
              type="button"
              onClick={() => void handleImportMd()}
              className="rounded px-4 py-2 text-sm"
              style={{ background: 'var(--primary)', color: '#fff' }}
            >
              {t(locale, 'capabilities.common.import')}
            </button>
          </div>
        </div>
      </Modal>

      <ConfirmDialog
        open={confirmDeleteExpert !== null}
        title={t(locale, 'capabilities.experts.deleteTitle')}
        message={t(locale, 'capabilities.experts.deleteMessage')}
        confirmLabel={t(locale, 'capabilities.common.delete')}
        cancelLabel={t(locale, 'capabilities.common.cancel')}
        danger
        onConfirm={() => {
          const expert = confirmDeleteExpert;
          setConfirmDeleteExpert(null);
          if (expert) void handleDeleteExpert(expert, false);
        }}
        onCancel={() => setConfirmDeleteExpert(null)}
      />
      <ConfirmDialog
        open={forceDeleteExpert !== null}
        title={t(locale, 'capabilities.experts.deleteTitle')}
        message={t(locale, 'capabilities.experts.deleteReferenced')}
        confirmLabel={t(locale, 'capabilities.experts.forceDelete')}
        cancelLabel={t(locale, 'capabilities.common.cancel')}
        danger
        onConfirm={() => {
          const expert = forceDeleteExpert;
          setForceDeleteExpert(null);
          if (expert) void handleDeleteExpert(expert, true);
        }}
        onCancel={() => setForceDeleteExpert(null)}
      />
      <ConfirmDialog
        open={confirmDeleteTeam !== null}
        title={t(locale, 'capabilities.experts.teamDeleteTitle')}
        message={t(locale, 'capabilities.experts.teamDeleteMessage')}
        confirmLabel={t(locale, 'capabilities.common.delete')}
        cancelLabel={t(locale, 'capabilities.common.cancel')}
        danger
        onConfirm={() => void handleDeleteTeam()}
        onCancel={() => setConfirmDeleteTeam(null)}
      />
    </div>
  );
}
