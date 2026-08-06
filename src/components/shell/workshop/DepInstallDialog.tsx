'use client';

import React from 'react';
import { t, useLocale } from '@/i18n';
import Modal from '@/components/ui/Modal';
import type { CreativeAppSummary } from '@/lib/tauri-adapter';

export interface DepInstallDialogProps {
  depInstallFor: CreativeAppSummary | null;
  depCommand: string;
  depInstalling: boolean;
  depConfirmChecked: boolean;
  onSetDepConfirmChecked: (v: boolean) => void;
  onClose: () => void;
  onRun: () => void;
}

/**
 * Dependency-install confirmation for a local app: shows the whitelist command
 * the Host will run and the network/node_modules risks before executing.
 */
export default function DepInstallDialog({
  depInstallFor,
  depCommand,
  depInstalling,
  depConfirmChecked,
  onSetDepConfirmChecked,
  onClose,
  onRun,
}: DepInstallDialogProps) {
  const locale = useLocale();
  if (!depInstallFor) return null;
  return (
    <Modal
      isOpen
      onClose={onClose}
      title={t(locale, 'workshop.installDeps')}
      width={480}
    >
      <div className="flex flex-col gap-3 py-1 text-xs">
        <p className="text-[var(--text-secondary)]">{t(locale, 'workshop.installDepsWarn')}</p>
        {depCommand ? (
          <pre className="p-2 rounded-lg border border-[var(--border)] bg-[var(--surface-subtle)] font-mono text-[11px] whitespace-pre-wrap">
            {depCommand}
          </pre>
        ) : null}
        <ul className="list-disc list-inside text-[var(--text-secondary)] space-y-1">
          <li>{t(locale, 'workshop.installDepsNetwork')}</li>
          <li>{t(locale, 'workshop.installDepsNodeModules')}</li>
          <li>{t(locale, 'workshop.installDepsLock')}</li>
          <li>{t(locale, 'workshop.installDepsUntrusted')}</li>
        </ul>
        <label className="flex items-center gap-2">
          <input
            type="checkbox"
            checked={depConfirmChecked}
            onChange={(e) => onSetDepConfirmChecked(e.target.checked)}
          />
          {t(locale, 'workshop.installDepsConfirm')}
        </label>
        <div className="flex justify-end gap-2">
          <button
            type="button"
            className="h-9 px-4 rounded-lg border border-[var(--border)]"
            onClick={onClose}
          >
            {t(locale, 'common.cancel')}
          </button>
          <button
            type="button"
            className="h-9 px-4 rounded-lg bg-[var(--primary)] text-[var(--accent-ink)] disabled:opacity-50"
            disabled={!depConfirmChecked || depInstalling}
            onClick={onRun}
          >
            {t(locale, 'workshop.installDepsRun')}
          </button>
        </div>
      </div>
    </Modal>
  );
}
