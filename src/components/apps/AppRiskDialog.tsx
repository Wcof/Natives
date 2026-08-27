'use client';

import React from 'react';
import { AlertTriangle, AlertCircle } from 'lucide-react';
import { t, useLocale } from '@/i18n';

interface AppRiskDialogProps {
  isOpen: boolean;
  title?: string;
  description?: string;
  note?: string;
  riskLevel?: number;
  confirmLabel?: string;
  loading?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}

export function AppRiskDialog({
  isOpen,
  title,
  description,
  note,
  riskLevel = 1,
  confirmLabel,
  loading = false,
  onConfirm,
  onCancel,
}: AppRiskDialogProps) {
  const locale = useLocale();
  if (!isOpen) return null;

  const isLevel2 = riskLevel >= 2;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-[var(--overlay-medium)] backdrop-blur-sm p-4 animate-in fade-in duration-150"
      role="dialog"
      aria-modal="true"
      aria-labelledby="risk-dialog-title"
    >
      <div className="relative w-full max-w-md rounded-2xl border border-[var(--border)] bg-[var(--surface)] p-6 shadow-2xl space-y-4">
        <div className="flex items-start gap-3">
          <div
            className={`flex h-10 w-10 shrink-0 items-center justify-center rounded-xl ${
              isLevel2
                ? 'bg-[var(--danger-soft)] text-[var(--danger)] border border-[var(--danger)]/20'
                : 'bg-[var(--warning-soft)] text-[var(--warning)] border border-[var(--warning)]/20'
            }`}
          >
            {isLevel2 ? <AlertTriangle className="h-5 w-5" /> : <AlertCircle className="h-5 w-5" />}
          </div>
          <div className="space-y-1">
            <h3 id="risk-dialog-title" className="text-base font-semibold text-[var(--text)]">
              {title || t(locale, 'appsPage.riskModalTitle')}
            </h3>
            <p className="text-sm text-[var(--text-secondary)]">
              {description || (isLevel2 ? t(locale, 'appsPage.riskLevel2Desc') : t(locale, 'appsPage.riskLevel1Desc'))}
            </p>
          </div>
        </div>

        {note && (
          <div className="rounded-xl border border-[var(--border-subtle)] bg-[var(--surface-hover)] p-3 text-xs leading-relaxed text-[var(--text-tertiary)]">
            {note}
          </div>
        )}

        <div className="flex items-center justify-end gap-3 pt-2">
          <button
            type="button"
            onClick={onCancel}
            disabled={loading}
            className="px-4 py-2 text-sm font-medium text-[var(--text-secondary)] hover:text-[var(--text)] transition-colors"
          >
            {t(locale, 'common.cancel')}
          </button>
          <button
            type="button"
            onClick={onConfirm}
            disabled={loading}
            className={`flex items-center justify-center px-4 py-2 text-sm font-medium rounded-xl text-[var(--primary-foreground)] shadow-sm transition-opacity ${
              isLevel2
                ? 'bg-[var(--danger)] hover:opacity-90'
                : 'bg-[var(--primary)] hover:bg-[var(--primary-hover)]'
            } ${loading ? 'opacity-50 cursor-not-allowed' : ''}`}
          >
            {loading ? t(locale, 'common.running') : confirmLabel || t(locale, 'common.confirm')}
          </button>
        </div>
      </div>
    </div>
  );
}
