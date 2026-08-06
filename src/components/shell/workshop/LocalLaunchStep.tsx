'use client';

import React from 'react';
import { t, type Locale } from '@/i18n';
import { planSummaryLines } from '@/lib/local-creative';
import type { PackageManager } from '@/lib/tauri-adapter';
import type { LocalWizardState } from '@/hooks/useLocalWizardState';

export interface LocalLaunchStepProps {
  w: LocalWizardState;
  locale: Locale;
}

/** Launch-step UI: smart/custom launch mode, env details, AI assist. */
export default function LocalLaunchStep({ w, locale }: LocalLaunchStepProps) {
  return (
    <div className="flex flex-col gap-3 text-xs">
      <div className="grid grid-cols-2 gap-2">
        <button
          type="button"
          className={`h-9 rounded-lg border ${
            w.launchMode === 'smart'
              ? 'border-[var(--primary)] text-[var(--primary)]'
              : 'border-[var(--border)]'
          }`}
          onClick={() => w.setLaunchMode('smart')}
        >
          {t(locale, 'workshop.localSmart')}
        </button>
        <button
          type="button"
          className={`h-9 rounded-lg border ${
            w.launchMode === 'custom'
              ? 'border-[var(--primary)] text-[var(--primary)]'
              : 'border-[var(--border)]'
          }`}
          onClick={() => w.setLaunchMode('custom')}
        >
          {t(locale, 'workshop.localCustom')}
        </button>
      </div>
      {w.scan && w.scan.packageManagerChoices.length > 1 && (
        <div>
          <label className="block mb-1">{t(locale, 'workshop.localPm')}</label>
          <select
            value={w.pm || ''}
            onChange={(e) => w.setPm((e.target.value || undefined) as PackageManager | undefined)}
            className="w-full h-9 px-2 rounded-lg border border-[var(--border)] bg-[var(--surface-subtle)]"
          >
            <option value="">{t(locale, 'workshop.localChoosePm')}</option>
            {w.scan.packageManagerChoices.map((pm) => (
              <option key={pm} value={pm}>
                {pm}
              </option>
            ))}
          </select>
        </div>
      )}
      {w.launchMode === 'custom' && w.scan && w.scan.scripts.length > 0 && (
        <div>
          <label className="block mb-1">{t(locale, 'workshop.localScript')}</label>
          <select
            value={w.script}
            onChange={(e) => w.setScript(e.target.value)}
            className="w-full h-9 px-2 rounded-lg border border-[var(--border)] bg-[var(--surface-subtle)]"
          >
            {w.scan.scripts.map((s) => (
              <option key={s} value={s}>
                {s}
              </option>
            ))}
          </select>
        </div>
      )}
      {w.launchMode === 'custom' && (
        <div className="grid grid-cols-2 gap-2">
          <div>
            <label className="block mb-1">{t(locale, 'workshop.localCwd')}</label>
            <input
              value={w.cwd}
              onChange={(e) => w.setCwd(e.target.value)}
              className="w-full h-8 px-2 rounded border border-[var(--border)] bg-[var(--surface-subtle)]"
            />
          </div>
          <div>
            <label className="block mb-1">{t(locale, 'workshop.localOpenPath')}</label>
            <input
              value={w.openPath}
              onChange={(e) => w.setOpenPath(e.target.value)}
              className="w-full h-8 px-2 rounded border border-[var(--border)] bg-[var(--surface-subtle)]"
            />
          </div>
          <div className="col-span-2">
            <label className="block mb-1">{t(locale, 'workshop.localPort')}</label>
            <div className="flex gap-2">
              <select
                value={w.portMode}
                onChange={(e) => w.setPortMode(e.target.value as 'auto' | 'fixed')}
                className="h-8 px-2 rounded border border-[var(--border)] bg-[var(--surface-subtle)]"
              >
                <option value="auto">{t(locale, 'workshop.portAuto')}</option>
                <option value="fixed">{t(locale, 'workshop.portFixed')}</option>
              </select>
              {w.portMode === 'fixed' && (
                <input
                  value={w.portValue}
                  onChange={(e) => w.setPortValue(e.target.value)}
                  className="w-24 h-8 px-2 rounded border border-[var(--border)] bg-[var(--surface-subtle)]"
                  placeholder="5173"
                />
              )}
            </div>
          </div>
        </div>
      )}
      <label className="flex items-center gap-2">
        <input
          type="checkbox"
          checked={w.autoOpen}
          onChange={(e) => w.setAutoOpen(e.target.checked)}
        />
        {t(locale, 'workshop.localAutoOpen')}
      </label>
      <div className="flex flex-wrap gap-2">
        <button
          type="button"
          className="h-8 px-3 text-[11px] rounded-lg border border-[var(--border)] disabled:opacity-50"
          disabled={w.aiBusy || !w.root.trim()}
          onClick={() => void w.runAiPreview()}
        >
          {w.aiBusy ? t(locale, 'workshop.localScanning') : t(locale, 'workshop.localAiPreview')}
        </button>
        <button
          type="button"
          className="h-8 px-3 text-[11px] rounded-lg border border-[var(--border)] disabled:opacity-50"
          disabled={w.aiBusy || !w.aiPendingConfirm || !w.root.trim()}
          onClick={() => void w.runAiAnalyze()}
        >
          {t(locale, 'workshop.localAiConfirmSend')}
        </button>
      </div>
      {w.aiPreview && (
        <details className="text-[11px]" open={w.aiPendingConfirm}>
          <summary>{t(locale, 'workshop.localAiPayload')}</summary>
          <pre className="mt-1 max-h-40 overflow-auto bg-[var(--surface-subtle)] border border-[var(--border)] rounded-lg p-2 whitespace-pre-wrap">
            {w.aiPreview}
          </pre>
        </details>
      )}
      <pre className="bg-[var(--surface-subtle)] border border-[var(--border)] rounded-lg p-3 whitespace-pre-wrap">
        {planSummaryLines(
          w.launchMode === 'custom' ? w.applyCustomPlan() : w.plan ?? w.scan?.rulePlan,
          locale === 'en' ? 'en' : 'zh',
        ).join('\n')}
      </pre>
      <div className="flex justify-between gap-2">
        <button
          type="button"
          className="h-9 px-4 text-xs rounded-lg border border-[var(--border)]"
          onClick={() => w.setStep('scan')}
        >
          {t(locale, 'common.back')}
        </button>
        <button
          type="button"
          className="h-9 px-4 text-xs rounded-lg bg-[var(--primary)] text-[var(--accent-ink)]"
          onClick={() => w.setStep('confirm')}
        >
          {t(locale, 'common.next')}
        </button>
      </div>
    </div>
  );
}
