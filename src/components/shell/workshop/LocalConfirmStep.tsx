'use client';

import React from 'react';
import { t, type Locale } from '@/i18n';
import { defaultLocalTitleFromPath, planSummaryLines } from '@/lib/local-creative';
import type { LocalWizardState } from '@/hooks/useLocalWizardState';

export interface LocalConfirmStepProps {
  w: LocalWizardState;
  locale: Locale;
}

/**
 * Confirm step: shows the final launch plan and the two save actions. The
 * "start after save" decision is made by the button the user presses — there is
 * no separate checkbox that could drift from the actual save behavior.
 */
export default function LocalConfirmStep({ w, locale }: LocalConfirmStepProps) {
  return (
    <div className="flex flex-col gap-3 text-xs">
      <div className="p-3 rounded-lg border border-[var(--border)] bg-[var(--surface-subtle)] space-y-1">
        <div>
          <strong>{w.title || defaultLocalTitleFromPath(w.root)}</strong>
        </div>
        <div className="text-[var(--text-secondary)] truncate">{w.root}</div>
        <pre className="whitespace-pre-wrap pt-2">
          {planSummaryLines(
            w.launchMode === 'custom' ? w.applyCustomPlan() : w.plan ?? w.scan?.rulePlan,
            locale,
          ).join('\n')}
        </pre>
      </div>
      <div className="flex justify-between gap-2">
        <button
          type="button"
          className="h-9 px-4 text-xs rounded-lg border border-[var(--border)]"
          onClick={() => w.setStep('launch')}
        >
          {t(locale, 'common.back')}
        </button>
        <div className="flex gap-2">
          <button
            type="button"
            className="h-9 px-4 text-xs rounded-lg border border-[var(--border)] disabled:opacity-50"
            disabled={w.saving}
            onClick={() => void w.save(false)}
          >
            {t(locale, 'workshop.localSaveOnly')}
          </button>
          <button
            type="button"
            className="h-9 px-4 text-xs rounded-lg bg-[var(--primary)] text-[var(--accent-ink)] disabled:opacity-50"
            disabled={w.saving}
            onClick={() => void w.save(true)}
          >
            {t(locale, 'workshop.localSaveStart')}
          </button>
        </div>
      </div>
    </div>
  );
}
