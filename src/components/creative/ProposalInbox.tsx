//! Agent proposal inbox (batch 10 CR-1002, hardened T06).
//!
//! Hosts the Host-persisted list of validated agent proposals, each rendered as
//! a ProposalApprovalCard keyed by the stable Daemon `proposalId`. Approve
//! routes through the Host gate by id (creative_app_proposal_approve → register
//! + start); reject journals a rejection and removes the card. A failure is
//! rethrown to the card — the proposal stays visible with the error, and a
//! success toast is only ever shown for a real success. Repeat clicks on an
//! already-decided proposal are an idempotent `already_decided` no-op.

import React, { useState } from 'react';
import ProposalApprovalCard from './ProposalApprovalCard';
import { useLocale, t } from '@/i18n';
import type { CreativeAppProposal } from '@/lib/tauri-adapter';

export interface ProposalInboxProps {
  proposals: CreativeAppProposal[];
  onDismissed: (proposal: CreativeAppProposal) => void;
  onToast: (message: string) => void;
  onRegistered: (proposal: CreativeAppProposal) => void;
}

export default function ProposalInbox({
  proposals,
  onDismissed,
  onToast,
  onRegistered,
}: ProposalInboxProps) {
  const locale = useLocale();
  // Track which proposal ids are being processed (disables double-submit).
  const [processing, setProcessing] = useState<Set<string>>(new Set());

  if (proposals.length === 0) return null;

  const approve = async (proposal: CreativeAppProposal) => {
    const key = proposal.proposalId;
    setProcessing((prev) => new Set(prev).add(key));
    try {
      const result = await window.nativesAPI?.creativeApp?.proposalApprove?.(key);
      if (result?.status === 'approved') {
        onRegistered(proposal);
        onDismissed(proposal);
      }
      // `already_decided` keeps the card; the host already made the decision.
    } catch (err) {
      // Registration/verification failed — rethrow so the card shows the error
      // and stays visible. Never swallow into a silent success.
      throw err;
    } finally {
      setProcessing((prev) => {
        const next = new Set(prev);
        next.delete(key);
        return next;
      });
    }
  };

  const reject = async (proposal: CreativeAppProposal) => {
    const key = proposal.proposalId;
    setProcessing((prev) => new Set(prev).add(key));
    try {
      const result = await window.nativesAPI?.creativeApp?.proposalReject?.(key);
      if (result?.status === 'rejected') {
        onDismissed(proposal);
      }
    } catch (err) {
      throw err;
    } finally {
      setProcessing((prev) => {
        const next = new Set(prev);
        next.delete(key);
        return next;
      });
    }
  };

  return (
    <div className="space-y-3" data-proposal-inbox>
      <h2 className="text-sm font-medium text-[var(--text)] px-1">
        {t(locale, 'workshop.proposalInbox')}
      </h2>
      {proposals.map((p) => (
        <ProposalApprovalCard
          key={p.proposalId}
          proposal={p}
          onApprove={approve}
          onReject={reject}
          onToast={onToast}
        />
      ))}
    </div>
  );
}
