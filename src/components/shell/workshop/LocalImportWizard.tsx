'use client';

import React from 'react';
import { t, useLocale } from '@/i18n';
import Modal from '@/components/ui/Modal';
import { canProceedFromScan } from '@/lib/local-creative';
import { useLocalWizardState } from '@/hooks/useLocalWizardState';
import type { LocalWizardStep } from '@/lib/local-creative';
import LocalLaunchStep from './LocalLaunchStep';
import LocalConfirmStep from './LocalConfirmStep';

export interface LocalImportWizardProps {
  open: boolean;
  onClose: () => void;
  onToast: (message: string) => void;
  /** Called after a successful save so the page can refresh the catalog. */
  onSaved: () => void;
}

const STEPS: LocalWizardStep[] = ['basic', 'scan', 'launch', 'confirm'];

/**
 * Four-step local project import wizard (basic → scan → launch → confirm).
 * All state lives in `useLocalWizardState`; this component only wires steps.
 */
export default function LocalImportWizard({ open, onClose, onToast, onSaved }: LocalImportWizardProps) {
  const locale = useLocale();
  const w = useLocalWizardState({ onToast, onSaved });

  if (!open) return null;

  const close = () => {
    w.reset();
    onClose();
  };

  return (
    <Modal isOpen onClose={close} title={t(locale, 'workshop.localWizardTitle')} width={560}>
      <div className="flex flex-col gap-4 py-1">
        <div className="flex gap-2 text-[11px] text-[var(--text-secondary)]">
          {STEPS.map((s, i) => (
            <span
              key={s}
              className={w.step === s ? 'text-[var(--primary)] font-semibold' : ''}
            >
              {i + 1}. {t(locale, `workshop.localStep.${s}` as 'workshop.localStep.basic')}
            </span>
          ))}
        </div>

        {w.step === 'basic' && (
          <div className="flex flex-col gap-3">
            <div>
              <label className="block text-xs font-medium text-[var(--text-secondary)] mb-1.5">
                {t(locale, 'workshop.localFolder')}
              </label>
              <div className="flex gap-2">
                <input
                  value={w.root}
                  onChange={(e) => w.setRoot(e.target.value)}
                  className="flex-1 h-9 px-3 text-xs rounded-lg border border-[var(--border)] bg-[var(--surface-subtle)]"
                  placeholder={t(locale, 'workshop.localRootPlaceholder')}
                />
                <button
                  type="button"
                  className="h-9 px-3 text-xs rounded-lg border border-[var(--border)]"
                  onClick={() => void w.pickFolder()}
                >
                  {t(locale, 'workshop.browseFiles')}
                </button>
              </div>
            </div>
            <div>
              <label className="block text-xs font-medium text-[var(--text-secondary)] mb-1.5">
                {t(locale, 'workshop.templateName')}
              </label>
              <input
                value={w.title}
                onChange={(e) => w.setTitle(e.target.value)}
                className="w-full h-9 px-3 text-xs rounded-lg border border-[var(--border)] bg-[var(--surface-subtle)]"
              />
            </div>
            <div>
              <label className="block text-xs font-medium text-[var(--text-secondary)] mb-1.5">
                {t(locale, 'workshop.description')}
              </label>
              <textarea
                value={w.desc}
                onChange={(e) => w.setDesc(e.target.value)}
                className="w-full min-h-[64px] px-3 py-2 text-xs rounded-lg border border-[var(--border)] bg-[var(--surface-subtle)]"
              />
            </div>
            {w.scanError && <div className="text-xs text-[var(--danger)]">{w.scanError}</div>}
            <div className="flex justify-end gap-2">
              <button
                type="button"
                className="h-9 px-4 text-xs rounded-lg border border-[var(--border)]"
                onClick={close}
              >
                {t(locale, 'common.cancel')}
              </button>
              <button
                type="button"
                className="h-9 px-4 text-xs rounded-lg bg-[var(--primary)] text-[var(--accent-ink)] disabled:opacity-50"
                disabled={!w.root.trim() || w.scanning}
                onClick={() => void w.runScan(w.root.trim())}
              >
                {w.scanning ? t(locale, 'workshop.localScanning') : t(locale, 'common.next')}
              </button>
            </div>
          </div>
        )}

        {w.step === 'scan' && w.scan && (
          <div className="flex flex-col gap-3 text-xs">
            <div className="grid grid-cols-2 gap-2">
              <div>
                {t(locale, 'workshop.localKind')}: <strong>{w.scan.projectKind}</strong>
              </div>
              <div>
                {t(locale, 'workshop.localPm')}:{' '}
                <strong>{w.scan.packageManager || '—'}</strong>
              </div>
              <div>
                {t(locale, 'workshop.localNodeModules')}:{' '}
                <strong>{w.scan.hasNodeModules ? t(locale, 'workshop.yes') : t(locale, 'workshop.missing')}</strong>
              </div>
              <div>
                {t(locale, 'workshop.localScripts')}:{' '}
                <strong>{w.scan.scripts.slice(0, 5).join(', ') || '—'}</strong>
              </div>
            </div>
            {w.scan.risks.length > 0 && (
              <ul className="list-disc list-inside text-[var(--warning)]">
                {w.scan.risks.map((r) => (
                  <li key={r}>{r}</li>
                ))}
              </ul>
            )}
            {w.scan.blockers.length > 0 && (
              <ul className="list-disc list-inside text-[var(--danger)]">
                {w.scan.blockers.map((r) => (
                  <li key={r}>{r}</li>
                ))}
              </ul>
            )}
            <div className="flex justify-between gap-2">
              <button
                type="button"
                className="h-9 px-4 text-xs rounded-lg border border-[var(--border)]"
                onClick={() => w.setStep('basic')}
              >
                {t(locale, 'common.back')}
              </button>
              <button
                type="button"
                className="h-9 px-4 text-xs rounded-lg bg-[var(--primary)] text-[var(--accent-ink)] disabled:opacity-50"
                disabled={!canProceedFromScan(w.scan)}
                onClick={() => w.setStep('launch')}
              >
                {t(locale, 'common.next')}
              </button>
            </div>
          </div>
        )}

        {w.step === 'launch' && <LocalLaunchStep w={w} locale={locale} />}
        {w.step === 'confirm' && <LocalConfirmStep w={w} locale={locale} />}
      </div>
    </Modal>
  );
}
