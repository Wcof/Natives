//! Agent proposal inbox (batch 10 CR-1002).
//!
//! Hosts a list of Host-validated agent proposals, each rendered as a
//! ProposalApprovalCard. Approve routes through the Host gate
//! (creative_app_proposal_approve → register + start); reject journals a
//! rejection and removes the card. A proposal that fails to register stays
//! visible — never faked as success.

import React, { useState } from 'react';
import ProposalApprovalCard from './ProposalApprovalCard';
import { useLocale, t } from '@/i18n';
import { classifyError } from '@/lib/error-classifier';
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
  // Track which proposal ids are being processed.
  const [processing, setProcessing] = useState<Set<string>>(new Set());

  if (proposals.length === 0) return null;

  const keyOf = (p: CreativeAppProposal) => `${p.kind}-${p.title}-${p.projectRoot}`;

  const approve = async (proposal: CreativeAppProposal) => {
    const key = keyOf(proposal);
    setProcessing((prev) => new Set(prev).add(key));
    try {
      const summary = await window.nativesAPI?.creativeApp?.proposalApprove?.(proposal);
      if (summary) {
        onRegistered(proposal);
        onDismissed(proposal);
      }
    } catch (err) {
      // Registration failed — keep the proposal visible with the error.
      onToast(classifyError(err).userMessage);
    } finally {
      setProcessing((prev) => {
        const next = new Set(prev);
        next.delete(key);
        return next;
      });
    }
  };

  const reject = async (proposal: CreativeAppProposal) => {
    const key = keyOf(proposal);
    setProcessing((prev) => new Set(prev).add(key));
    try {
      await window.nativesAPI?.creativeApp?.proposalReject?.(proposal);
      onDismissed(proposal);
    } catch (err) {
      onToast(classifyError(err).userMessage);
    } finally {
      setProcessing((prev) => {
        const next = new Set(prev);
        next.delete(key);
        return next;
      });
    }
  };

  return (
    <div className="space-y-3">
      <h2 className="text-sm font-medium text-[var(--text)] px-1">
        {t(locale, 'workshop.proposalInbox')}
      </h2>
      {proposals.map((p) => {
        const key = keyOf(p);
        return (
          <ProposalApprovalCard
            key={key}
            proposal={p}
            onApprove={approve}
            onReject={reject}
            onToast={onToast}
          />
        );
      })}
    </div>
  );
}
