'use client';

import { useCallback, useEffect, useState } from 'react';
import ProposalInbox from '@/components/creative/ProposalInbox';
import type { CreativeAppProposal } from '@/lib/tauri-adapter';

export interface ProposalInboxControllerProps {
  /** Called after a proposal is approved/registered (refreshes the catalog). */
  onRegistered: (proposal: CreativeAppProposal) => void;
  onToast: (message: string) => void;
}

/**
 * Host-persisted proposal inbox (T06). Loads the pending list from the Host on
 * mount and re-lists on every `creative-app` db-state-changed broadcast, so a
 * fresh proposal produced by any assistant run appears here and approved/
 * rejected ones disappear without renderer-side faking. The Host is the
 * authority: a failed list never clears an inbox the user already sees.
 */
export default function ProposalInboxController({
  onRegistered,
  onToast,
}: ProposalInboxControllerProps) {
  const [pendingProposals, setPendingProposals] = useState<CreativeAppProposal[]>([]);

  const reloadProposals = useCallback(() => {
    const api = window.nativesAPI?.creativeApp;
    if (!api?.proposalList) return;
    void api
      .proposalList()
      .then((rows) => setPendingProposals(Array.isArray(rows) ? rows : []))
      .catch(() => {
        // The Host is the authority; a failed list must not clear an inbox
        // the user already sees.
      });
  }, []);

  useEffect(() => {
    reloadProposals();
    const unsub = window.nativesAPI?.onDbStateChanged?.((_event, channel) => {
      if (channel !== 'creative-app') return;
      reloadProposals();
    });
    return () => {
      unsub?.();
    };
  }, [reloadProposals]);

  if (pendingProposals.length === 0) return null;

  return (
    <div className="mb-4">
      <ProposalInbox
        proposals={pendingProposals}
        onDismissed={(p) => setPendingProposals((prev) => prev.filter((x) => x !== p))}
        onToast={onToast}
        onRegistered={(p) => {
          setPendingProposals((prev) => prev.filter((x) => x !== p));
          onRegistered(p);
        }}
      />
    </div>
  );
}
