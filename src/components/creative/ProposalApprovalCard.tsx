//! Agent proposal approval card (batch 10 CR-1002).
//!
//! Surfaces a Host-validated agent proposal for explicit user approval before
//! it becomes an Application → Profile → Runtime → Window. The card shows the
//! proposal's intent (create/start, ownership, driver, env keys) and the two
//! actions: approve (proceeds to register/start) or reject (keeps the proposal
//! unchanged — no fake success). Failure keeps the proposal visible.

import React, { useState } from 'react';
import { useLocale, t } from '@/i18n';
import { classifyError } from '@/lib/error-classifier';
import type { CreativeAppProposal } from '@/lib/tauri-adapter';

export interface ProposalApprovalCardProps {
  proposal: CreativeAppProposal;
  onApprove: (proposal: CreativeAppProposal) => Promise<void>;
  onReject: (proposal: CreativeAppProposal) => Promise<void>;
  onToast: (message: string) => void;
}

/** Human-readable driver summary for the card. */
function driverLabel(driver: CreativeAppProposal['driver']): string {
  switch (driver.kind) {
    case 'python':
      return `python:${driver.entry}`;
    case 'binary':
      return 'binary';
    case 'staticHttp':
      return 'static_http';
    case 'compose':
      return 'compose';
  }
}

export default function ProposalApprovalCard({
  proposal,
  onApprove,
  onReject,
  onToast,
}: ProposalApprovalCardProps) {
  const locale = useLocale();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleApprove = async () => {
    setBusy(true);
    setError(null);
    try {
      await onApprove(proposal);
      onToast(t(locale, 'workshop.proposalApproved'));
    } catch (err) {
      // Failure keeps the proposal visible — never fake success.
      setError(classifyError(err).userMessage);
      onToast(classifyError(err).userMessage);
    } finally {
      setBusy(false);
    }
  };

  const handleReject = async () => {
    setBusy(true);
    setError(null);
    try {
      await onReject(proposal);
      onToast(t(locale, 'workshop.proposalRejected'));
    } catch (err) {
      setError(classifyError(err).userMessage);
      onToast(classifyError(err).userMessage);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="border border-[var(--border)] rounded-xl bg-[var(--surface)] p-4">
      <div className="flex items-center justify-between mb-3">
        <h3 className="text-sm font-medium text-[var(--text)]">
          {t(locale, 'workshop.proposalTitle')}
        </h3>
        <span className="text-xs px-2 py-0.5 rounded-full bg-[var(--surface-subtle)] text-[var(--text-secondary)]">
          {proposal.kind === 'create' ? t(locale, 'workshop.proposalCreate') : t(locale, 'workshop.proposalStart')}
        </span>
      </div>

      {/* Proposal summary */}
      <div className="space-y-1.5 text-xs mb-4">
        <div className="flex justify-between">
          <span className="text-[var(--text-secondary)]">{t(locale, 'workshop.appTitle')}</span>
          <span className="text-[var(--text)] font-medium">{proposal.title}</span>
        </div>
        <div className="flex justify-between">
          <span className="text-[var(--text-secondary)]">{t(locale, 'workshop.proposalOwnership')}</span>
          <span className="text-[var(--text)]">{proposal.ownership}</span>
        </div>
        <div className="flex justify-between">
          <span className="text-[var(--text-secondary)]">{t(locale, 'workshop.proposalDriver')}</span>
          <span className="text-[var(--text)] font-mono">{driverLabel(proposal.driver)}</span>
        </div>
        <div className="flex justify-between gap-4">
          <span className="text-[var(--text-secondary)] shrink-0">{t(locale, 'workshop.projectRoot')}</span>
          <span className="text-[var(--text)] font-mono truncate">{proposal.projectRoot}</span>
        </div>
        {proposal.envKeys.length > 0 && (
          <div className="flex justify-between">
            <span className="text-[var(--text-secondary)]">{t(locale, 'workshop.proposalEnvKeys')}</span>
            <span className="text-[var(--text)] font-mono">{proposal.envKeys.join(', ')}</span>
          </div>
        )}
      </div>

      {error && (
        <p className="text-xs text-[var(--danger)] mb-3">{error}</p>
      )}

      {/* Actions */}
      <div className="flex justify-end gap-2">
        <button
          type="button"
          disabled={busy}
          onClick={handleReject}
          className="px-3 py-1.5 rounded-lg border border-[var(--border)] text-xs text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] disabled:opacity-50"
        >
          {t(locale, 'workshop.proposalReject')}
        </button>
        <button
          type="button"
          disabled={busy}
          onClick={handleApprove}
          className="px-3 py-1.5 rounded-lg bg-[var(--accent)] text-[var(--on-accent)] text-xs font-medium hover:opacity-90 disabled:opacity-50"
        >
          {busy ? t(locale, 'workshop.proposalApproving') : t(locale, 'workshop.proposalApprove')}
        </button>
      </div>
    </div>
  );
}
