'use client';

import React from 'react';
import { AlertTriangle, Loader } from 'lucide-react';
import { t, type Locale } from '@/i18n';
import type { GithubWizardState } from '@/hooks/useGithubWizardState';

export interface GithubWizardUrlStepProps {
  w: GithubWizardState;
  locale: Locale;
}

/** GitHub install wizard — URL / token entry step. */
export default function GithubWizardUrlStep({ w, locale }: GithubWizardUrlStepProps) {
  return (
    <div className="flex flex-col gap-4 py-1">
      <div>
        <label className="block text-xs font-medium text-[var(--text-secondary)] mb-1.5">
          {t(locale, 'workshop.githubRepoUrl')}
        </label>
        <input
          value={w.repoUrl}
          onChange={(e) => w.setRepoUrl(e.target.value)}
          placeholder={t(locale, 'workshop.githubRepoPlaceholder')}
          className="w-full h-9 px-3 text-xs rounded-lg border border-[var(--border)] bg-[var(--surface-subtle)] text-[var(--text)] focus:outline-none focus:border-[var(--primary)] transition-all"
        />
      </div>

      <div>
        <label className="block text-xs font-medium text-[var(--text-secondary)] mb-1.5">
          {t(locale, 'settings.githubToken')}
        </label>
        <div className="grid grid-cols-3 gap-1.5 p-1 bg-[var(--surface-subtle)] border border-[var(--border)] rounded-lg">
          {(['public', 'saved', 'once'] as const).map((m) => (
            <button
              key={m}
              type="button"
              onClick={() => w.setTokenMode(m)}
              className={`h-7 text-[11px] font-medium rounded-md transition-all ${
                w.tokenMode === m
                  ? 'bg-[var(--surface)] text-[var(--primary)] shadow-sm font-semibold'
                  : 'text-[var(--text-secondary)] hover:text-[var(--text)]'
              }`}
            >
              {m === 'public'
                ? t(locale, 'workshop.githubPublicAccess')
                : m === 'saved'
                  ? t(locale, 'workshop.githubUseSavedToken')
                  : t(locale, 'workshop.githubOneShotToken')}
            </button>
          ))}
        </div>
      </div>

      {w.tokenMode === 'once' && (
        <div className="space-y-2">
          <input
            type="password"
            value={w.tokenInput}
            onChange={(e) => w.setTokenInput(e.target.value)}
            placeholder={t(locale, 'workshop.githubTokenPlaceholder')}
            className="w-full h-9 px-3 text-xs rounded-lg border border-[var(--border)] bg-[var(--surface-subtle)] text-[var(--text)] focus:outline-none focus:border-[var(--primary)] transition-all"
          />
          <label className="flex items-center gap-2 text-xs text-[var(--text-secondary)] cursor-pointer">
            <input
              type="checkbox"
              checked={w.saveToken}
              onChange={(e) => w.setSaveToken(e.target.checked)}
              className="rounded border-[var(--border)]"
            />
            <span>{t(locale, 'workshop.githubSaveToken')}</span>
          </label>
        </div>
      )}

      {w.installError && (
        <div className="text-xs text-[var(--danger)] bg-[var(--danger)]/10 border-[var(--danger)]/20 p-2.5 rounded-lg flex items-start gap-2">
          <AlertTriangle size={14} className="shrink-0 mt-0.5" />
          <span>{w.installError}</span>
        </div>
      )}

      <div className="flex justify-end gap-2 pt-2">
        <button
          type="button"
          className="h-9 px-4 text-xs font-medium rounded-lg border border-[var(--border)] bg-[var(--surface)] text-[var(--text)] hover:bg-[var(--surface-hover)] transition-all"
          disabled={w.inspecting || !w.repoUrl.trim()}
          onClick={() => void w.runInspect(false)}
        >
          {t(locale, 'workshop.githubManual')}
        </button>
        <button
          type="button"
          className="h-9 px-4 text-xs font-medium rounded-lg bg-[var(--primary)] text-[var(--accent-ink)] hover:opacity-90 transition-all flex items-center gap-1.5"
          disabled={w.inspecting || !w.repoUrl.trim()}
          onClick={() => void w.runInspect(true)}
        >
          {w.inspecting ? <Loader size={14} className="animate-spin" /> : null}
          <span>{w.inspecting ? '…' : t(locale, 'workshop.githubOneClick')}</span>
        </button>
      </div>
    </div>
  );
}
