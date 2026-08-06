//! Agent proposal approval card (batch 10 CR-1002, hardened T06).
//!
//! Surfaces a Host-validated agent proposal for explicit user approval before
//! it becomes an Application → Profile → Runtime → Window. The card shows the
//! proposal's intent in full — interpreter/executable, argv, cwd, env KEY
//! names, port, ownership, project root — so the user can see exactly what
//! would run. Approve/reject failures keep the proposal visible with the
//! error; a success toast is only ever shown for a real success.

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

/** The executable/interpreter line for the driver, if any. */
function executableOf(driver: CreativeAppProposal['driver']): string | null {
  switch (driver.kind) {
    case 'python':
      return driver.interpreter;
    case 'binary':
      return driver.executablePath;
    default:
      return null;
  }
}

/** The argv/args line for the driver, if any. */
function argsOf(driver: CreativeAppProposal['driver']): string[] | null {
  switch (driver.kind) {
    case 'python':
      return [driver.entry, ...driver.args];
    case 'binary':
      return driver.args;
    default:
      return null;
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
      const classified = classifyError(err);
      setError(classified.userMessage);
      onToast(classified.userMessage);
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
      const classified = classifyError(err);
      setError(classified.userMessage);
      onToast(classified.userMessage);
    } finally {
      setBusy(false);
    }
  };

  const executable = executableOf(proposal.driver);
  const args = argsOf(proposal.driver);

  return (
    <div className="border border-[var(--border)] rounded-xl bg-[var(--surface)] p-4" data-proposal-card>
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
        <div className="flex justify-between gap-4">
          <span className="text-[var(--text-secondary)] shrink-0">{t(locale, 'workshop.appTitle')}</span>
          <span className="text-[var(--text)] font-medium text-right">{proposal.title}</span>
        </div>
        <div className="flex justify-between gap-4">
          <span className="text-[var(--text-secondary)] shrink-0">{t(locale, 'workshop.proposalOwnership')}</span>
          <span className="text-[var(--text)]" data-proposal-ownership>{proposal.ownership}</span>
        </div>
        <div className="flex justify-between gap-4">
          <span className="text-[var(--text-secondary)] shrink-0">{t(locale, 'workshop.proposalDriver')}</span>
          <span className="text-[var(--text)] font-mono">{driverLabel(proposal.driver)}</span>
        </div>
        <div className="flex justify-between gap-4">
          <span className="text-[var(--text-secondary)] shrink-0">{t(locale, 'workshop.projectRoot')}</span>
          <span className="text-[var(--text)] font-mono truncate" data-proposal-root>{proposal.projectRoot}</span>
        </div>
        {executable && (
          <div className="flex justify-between gap-4">
            <span className="text-[var(--text-secondary)] shrink-0">{t(locale, 'workshop.proposalExecutable')}</span>
            <span className="text-[var(--text)] font-mono break-all text-right" data-proposal-executable>
              {executable}
            </span>
          </div>
        )}
        {args && args.length > 0 && (
          <div className="flex justify-between gap-4">
            <span className="text-[var(--text-secondary)] shrink-0">{t(locale, 'workshop.proposalArgs')}</span>
            <span className="text-[var(--text)] font-mono break-all text-right" data-proposal-args>
              {args.join(' ')}
            </span>
          </div>
        )}
        {proposal.driver.kind !== 'staticHttp' && (
          <div className="flex justify-between gap-4">
            <span className="text-[var(--text-secondary)] shrink-0">{t(locale, 'workshop.proposalCwd')}</span>
            <span className="text-[var(--text)] font-mono text-right">
              {proposal.driver.kind === 'python' || proposal.driver.kind === 'binary'
                ? proposal.driver.cwdRelative
                : proposal.driver.kind}
            </span>
          </div>
        )}
        {proposal.environmentKeys.length > 0 && (
          <div className="flex justify-between gap-4">
            <span className="text-[var(--text-secondary)] shrink-0">{t(locale, 'workshop.proposalEnvKeys')}</span>
            <span className="text-[var(--text)] font-mono" data-proposal-env-keys>
              {proposal.environmentKeys.join(', ')}
            </span>
          </div>
        )}
        {proposal.driver.kind === 'compose' && proposal.driver.privileged && (
          <p className="text-xs text-[var(--danger)]" data-proposal-privileged>
            {t(locale, 'workshop.proposalPrivilegedRisk')}
          </p>
        )}
      </div>

      {error && (
        <p className="text-xs text-[var(--danger)] mb-3" data-proposal-error>
          {error}
        </p>
      )}

      {/* Actions */}
      <div className="flex justify-end gap-2">
        <button
          type="button"
          disabled={busy}
          onClick={handleReject}
          className="px-3 py-1.5 rounded-lg border border-[var(--border)] text-xs text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] disabled:opacity-50"
          data-proposal-reject
        >
          {t(locale, 'workshop.proposalReject')}
        </button>
        <button
          type="button"
          disabled={busy}
          onClick={handleApprove}
          className="px-3 py-1.5 rounded-lg bg-[var(--accent)] text-[var(--on-accent)] text-xs font-medium hover:opacity-90 disabled:opacity-50"
          data-proposal-approve
        >
          {busy ? t(locale, 'workshop.proposalApproving') : t(locale, 'workshop.proposalApprove')}
        </button>
      </div>
    </div>
  );
}
