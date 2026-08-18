//! Agent proposal approval card (batch 10 CR-1002, hardened T06).
//!
//! Surfaces a Host-validated agent proposal for explicit user approval before
//! it becomes an Application → Profile → Runtime → Window. The card shows the
//! proposal's intent in full — interpreter/executable, argv, cwd, env KEY
//! names, port, ownership, project root — so the user can see exactly what
//! would run. Approve/reject failures keep the proposal visible with the
//! error; a success toast is only ever shown for a real success.

import React, { useState } from 'react';
import { Check, Copy } from 'lucide-react';
import { useLocale, t, type Locale } from '@/i18n';
import { classifyError } from '@/lib/error-classifier';
import type { CreativeAppProposal } from '@/lib/tauri-adapter';

export interface ProposalApprovalCardProps {
  proposal: CreativeAppProposal;
  /**
   * Resolves `true` when the decision actually changed (approved). A duplicate
   * `already_decided` event resolves `false` — the card must not toast a fresh
   * success for a decision the host already settled. Failures reject.
   */
  onApprove: (proposal: CreativeAppProposal) => Promise<boolean>;
  onReject: (proposal: CreativeAppProposal) => Promise<boolean>;
  onToast: (message: string) => void;
}

/** Long values are visually truncated but always fully copyable. */
function CopyableValue({
  value,
  locale,
  onToast,
  dataAttr,
}: {
  value: string;
  locale: Locale;
  onToast: (message: string) => void;
  dataAttr?: string;
}) {
  const [copied, setCopied] = useState(false);
  const MAX = 160;
  const truncated = value.length > MAX ? `${value.slice(0, MAX)}…` : value;
  const copy = () => {
    void window.nativesAPI?.clipboard
      ?.write?.(value)
      .then(() => {
        setCopied(true);
        window.setTimeout(() => setCopied(false), 1200);
      })
      .catch((err) => onToast(classifyError(err).userMessage));
  };
  return (
    <span className="flex items-center gap-1 min-w-0" data-copyable-value>
      <span
        className="font-mono break-all text-right truncate"
        title={value}
        {...(dataAttr ? { [`data-${dataAttr}`]: true } : {})}
      >
        {truncated}
      </span>
      <button
        type="button"
        aria-label={`${t(locale, 'workshop.copyFullValue')} ${value}`}
        title={t(locale, 'workshop.copyFullValue')}
        onClick={copy}
        className="shrink-0 p-0.5 rounded text-[var(--text-secondary)] hover:text-[var(--text)] hover:bg-[var(--surface-hover)]"
      >
        {copied ? <Check size={10} /> : <Copy size={10} />}
      </button>
    </span>
  );
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

export function ProposalApprovalCardContent({
  proposal,
  onApprove,
  onReject,
  onToast,
  locale,
}: ProposalApprovalCardProps & { locale: Locale }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleApprove = async () => {
    setBusy(true);
    setError(null);
    try {
      const changed = await onApprove(proposal);
      if (changed) onToast(t(locale, 'workshop.proposalApproved'));
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
      const changed = await onReject(proposal);
      if (changed) onToast(t(locale, 'workshop.proposalRejected'));
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
          <CopyableValue value={proposal.projectRoot} locale={locale} onToast={onToast} dataAttr="proposal-root" />
        </div>
        {executable && (
          <div className="flex justify-between gap-4">
            <span className="text-[var(--text-secondary)] shrink-0">{t(locale, 'workshop.proposalExecutable')}</span>
            <CopyableValue value={executable} locale={locale} onToast={onToast} dataAttr="proposal-executable" />
          </div>
        )}
        {args && args.length > 0 && (
          <div className="flex justify-between gap-4">
            <span className="text-[var(--text-secondary)] shrink-0">{t(locale, 'workshop.proposalArgs')}</span>
            <CopyableValue value={args.join(' ')} locale={locale} onToast={onToast} dataAttr="proposal-args" />
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
        {(proposal.driver.kind === 'python' || proposal.driver.kind === 'binary') && (
          <div className="flex justify-between gap-4">
            <span className="text-[var(--text-secondary)] shrink-0">{t(locale, 'workshop.proposalPort')}</span>
            <span className="text-[var(--text)] font-mono" data-proposal-port>
              {proposal.driver.port.mode === 'fixed' && proposal.driver.port.value != null
                ? `${t(locale, 'workshop.proposalPortFixed')}: ${proposal.driver.port.value}`
                : t(locale, 'workshop.proposalPortAuto')}
            </span>
          </div>
        )}
        {proposal.environmentKeys.length > 0 && (
          <div className="flex justify-between gap-4">
            <span className="text-[var(--text-secondary)] shrink-0">{t(locale, 'workshop.proposalEnvKeys')}</span>
            <CopyableValue
              value={proposal.environmentKeys.join(', ')}
              locale={locale}
              onToast={onToast}
              dataAttr="proposal-env-keys"
            />
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

export default function ProposalApprovalCard(props: ProposalApprovalCardProps) {
  const locale = useLocale();
  return <ProposalApprovalCardContent {...props} locale={locale} />;
}
