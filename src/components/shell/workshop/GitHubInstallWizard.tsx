'use client';

import { useEffect } from 'react';
import { Loader } from 'lucide-react';
import { t, useLocale } from '@/i18n';
import Modal from '@/components/ui/Modal';
import { useGithubWizardState } from '@/hooks/useGithubWizardState';
import GithubWizardUrlStep from './GithubWizardUrlStep';
import GithubWizardManualStep from './GithubWizardManualStep';

export interface GitHubInstallWizardProps {
  open: boolean;
  onClose: () => void;
  onToast: (message: string) => void;
  /** Called after a successful install so the page can refresh the catalog. */
  onInstalled: () => void;
}

/**
 * GitHub install wizard (url → manual → installing). Progress events stream
 * from the Host while installing; the step map must match the Rust
 * ProgressStage snake_case names exactly.
 */
export default function GitHubInstallWizard({
  open,
  onClose,
  onToast,
  onInstalled,
}: GitHubInstallWizardProps) {
  const locale = useLocale();
  const w = useGithubWizardState({ onToast, onInstalled });

  useEffect(() => {
    const api = window.nativesAPI?.creativeApp;
    if (!api?.onProgress) return;
    return api.onProgress((ev) => w.onProgress(ev));
  }, [w.onProgress]);

  if (!open) return null;

  const close = () => {
    w.reset();
    onClose();
  };

  return (
    <Modal
      isOpen
      onClose={close}
      title={t(locale, 'workshop.githubWizardTitle')}
      width={520}
    >
      {w.step === 'url' && <GithubWizardUrlStep w={w} locale={locale} />}
      {w.step === 'manual' && <GithubWizardManualStep w={w} locale={locale} />}
      {w.step === 'installing' && (
        <div className="py-6 flex flex-col items-center justify-center text-center space-y-3">
          <Loader size={28} className="animate-spin text-[var(--primary)]" />
          <div className="text-sm font-semibold text-[var(--text)]">
            {w.progress ? w.stageLabel(w.progress.stage) : '…'}
          </div>
          <div className="text-xs text-[var(--text-secondary)] max-w-sm">
            {w.progress?.message}
          </div>
        </div>
      )}
    </Modal>
  );
}
