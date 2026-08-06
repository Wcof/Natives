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

/**
 * Resolves `true` only when the decision actually changed (approved/rejected).
 * A duplicate `already_decided` event resolves `false` so the card skips its
 * success toast.
 */
export type ProposalDecision = (proposal: CreativeAppProposal) => Promise<boolean>;

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

  const approve = async (proposal: CreativeAppProposal): Promise<boolean> => {
    const key = proposal.proposalId;
    setProcessing((prev) => new Set(prev).add(key));
    try {
      const result = await window.nativesAPI?.creativeApp?.proposalApprove?.(key);
      if (result?.status === 'approved') {
        onRegistered(proposal);
        onDismissed(proposal);
        return true;
      }
      // `already_decided` (or a null result): the host already settled this
      // proposal. Keep the card until the next list reload, but report "no
      // state change" so the card never toasts a fresh success for a duplicate
      // event. Any error propagates (no catch here) so the card shows it and
      // stays visible — never a swallowed silent success.
      return false;
    } finally {
      setProcessing((prev) => {
        const next = new Set(prev);
        next.delete(key);
        return next;
      });
    }
  };

  const reject = async (proposal: CreativeAppProposal): Promise<boolean> => {
    const key = proposal.proposalId;
    setProcessing((prev) => new Set(prev).add(key));
    try {
      const result = await window.nativesAPI?.creativeApp?.proposalReject?.(key);
      if (result?.status === 'rejected') {
        onDismissed(proposal);
        return true;
      }
      return false;
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
