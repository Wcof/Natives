'use client';

import React from 'react';
import { AlertTriangle } from 'lucide-react';
import { t, type Locale } from '@/i18n';
import type { GithubWizardState } from '@/hooks/useGithubWizardState';

export interface GithubWizardManualStepProps {
  w: GithubWizardState;
  locale: Locale;
}

/** GitHub install wizard — manual review/configure step. */
export default function GithubWizardManualStep({ w, locale }: GithubWizardManualStepProps) {
  const { inspect } = w;
  if (!inspect) return null;

  const onCandidateChange = (value: string) => {
    const c = inspect.candidates.find((x) => x.id === value) || null;
    w.setSelectedCandidate(c);
    if (c) {
      w.setHostPort(c.suggestedHostPort ? String(c.suggestedHostPort) : '');
      w.setOpenPath(c.openPath || '/');
      w.setHealthPath(c.healthPath || '');
      w.setService(c.service || '');
    }
  };

  return (
    <div className="flex flex-col gap-3 py-1 max-h-[460px] overflow-y-auto pr-1">
      {inspect.blockers.length > 0 && (
        <div className="text-xs text-[var(--danger)] bg-[var(--danger)]/10 border-[var(--danger)]/20 p-3 rounded-lg">
          <strong className="block mb-1 font-semibold">{t(locale, 'workshop.githubBlockers')}</strong>
          <ul className="list-disc list-inside space-y-1">
            {inspect.blockers.map((b) => (
              <li key={b}>{b}</li>
            ))}
          </ul>
        </div>
      )}

      {inspect.warnings.length > 0 && (
        <div className="text-xs text-[var(--warning)] bg-[var(--warning)]/10 border-[var(--warning)]/20 p-3 rounded-lg">
          <strong className="block mb-1 font-semibold">{t(locale, 'workshop.githubWarnings')}</strong>
          <ul className="list-disc list-inside space-y-1">
            {inspect.warnings.map((wn) => (
              <li key={wn}>{wn}</li>
            ))}
          </ul>
        </div>
      )}

      <div>
        <label className="block text-xs font-medium text-[var(--text-secondary)] mb-1">
          {t(locale, 'workshop.githubSelectTag')}
        </label>
        <select
          value={w.selectedTag}
          onChange={(e) => w.setSelectedTag(e.target.value)}
          className="w-full h-9 px-3 text-xs rounded-lg border border-[var(--border)] bg-[var(--surface-subtle)] text-[var(--text)] focus:outline-none focus:border-[var(--primary)] transition-[color,background-color,border-color,opacity,transform]"
        >
          {(inspect.availableTags.length
            ? inspect.availableTags
            : [{ tag: inspect.releaseTag, releaseId: 0, isPrerelease: inspect.isPrerelease }]
          ).map((tg) => (
            <option key={tg.tag} value={tg.tag}>
              {tg.tag}
              {tg.isPrerelease ? t(locale, 'workshop.githubPrerelease') : ''}
            </option>
          ))}
        </select>
      </div>

      <div>
        <label className="block text-xs font-medium text-[var(--text-secondary)] mb-1">
          {t(locale, 'workshop.githubSelectCandidate')}
        </label>
        <select
          value={w.selectedCandidate?.id || ''}
          onChange={(e) => onCandidateChange(e.target.value)}
          className="w-full h-9 px-3 text-xs rounded-lg border border-[var(--border)] bg-[var(--surface-subtle)] text-[var(--text)] focus:outline-none focus:border-[var(--primary)] transition-[color,background-color,border-color,opacity,transform]"
        >
          {inspect.candidates.map((c) => (
            <option key={c.id} value={c.id}>
              {c.title} ({c.runtime}, conf {c.confidence})
            </option>
          ))}
        </select>
      </div>

      <div className="grid grid-cols-2 gap-3">
        <div>
          <label className="block text-xs font-medium text-[var(--text-secondary)] mb-1">
            {t(locale, 'workshop.githubHostPort')}
          </label>
          <input
            value={w.hostPort}
            onChange={(e) => w.setHostPort(e.target.value)}
            className="w-full h-9 px-3 text-xs rounded-lg border border-[var(--border)] bg-[var(--surface-subtle)] text-[var(--text)] focus:outline-none focus:border-[var(--primary)] transition-[color,background-color,border-color,opacity,transform]"
          />
        </div>
        <div>
          <label className="block text-xs font-medium text-[var(--text-secondary)] mb-1">
            {t(locale, 'workshop.githubService')}
          </label>
          <input
            value={w.service}
            onChange={(e) => w.setService(e.target.value)}
            className="w-full h-9 px-3 text-xs rounded-lg border border-[var(--border)] bg-[var(--surface-subtle)] text-[var(--text)] focus:outline-none focus:border-[var(--primary)] transition-[color,background-color,border-color,opacity,transform]"
          />
        </div>
        <div>
          <label className="block text-xs font-medium text-[var(--text-secondary)] mb-1">
            {t(locale, 'workshop.githubOpenPath')}
          </label>
          <input
            value={w.openPath}
            onChange={(e) => w.setOpenPath(e.target.value)}
            className="w-full h-9 px-3 text-xs rounded-lg border border-[var(--border)] bg-[var(--surface-subtle)] text-[var(--text)] focus:outline-none focus:border-[var(--primary)] transition-[color,background-color,border-color,opacity,transform]"
          />
        </div>
        <div>
          <label className="block text-xs font-medium text-[var(--text-secondary)] mb-1">
            {t(locale, 'workshop.githubHealthPath')}
          </label>
          <input
            value={w.healthPath}
            onChange={(e) => w.setHealthPath(e.target.value)}
            className="w-full h-9 px-3 text-xs rounded-lg border border-[var(--border)] bg-[var(--surface-subtle)] text-[var(--text)] focus:outline-none focus:border-[var(--primary)] transition-[color,background-color,border-color,opacity,transform]"
          />
        </div>
      </div>

      {w.selectedCandidate && w.selectedCandidate.envRequirements.length > 0 && (
        <div className="space-y-2 pt-1">
          <div className="text-xs font-semibold text-[var(--text)]">
            {t(locale, 'workshop.githubEnvTitle')}
          </div>
          <div className="space-y-2">
            {w.selectedCandidate.envRequirements.map((e) => (
              <div key={e.key}>
                <label className="block text-[11px] font-medium text-[var(--text-secondary)] mb-1">
                  {e.key} {e.required ? <span className="text-[var(--danger)]">*</span> : null}
                </label>
                <input
                  type={e.secret ? 'password' : 'text'}
                  value={w.envValues[e.key] || ''}
                  onChange={(ev) => w.setEnvValues({ ...w.envValues, [e.key]: ev.target.value })}
                  className="w-full h-9 px-3 text-xs rounded-lg border border-[var(--border)] bg-[var(--surface-subtle)] text-[var(--text)] focus:outline-none focus:border-[var(--primary)] transition-[color,background-color,border-color,opacity,transform]"
                />
              </div>
            ))}
          </div>
        </div>
      )}

      {w.selectedCandidate && w.selectedCandidate.riskSummary.length > 0 && (
        <div className="text-xs bg-[var(--surface-subtle)] p-3 rounded-lg border border-[var(--border)] space-y-2">
          <strong className="block font-semibold text-[var(--text)]">
            {t(locale, 'workshop.githubRiskTitle')}
          </strong>
          <ul className="list-disc list-inside text-[var(--text-secondary)] space-y-1">
            {w.selectedCandidate.riskSummary.map((r) => (
              <li key={r}>{r}</li>
            ))}
          </ul>
          <label className="flex items-center gap-2 pt-1 cursor-pointer">
            <input
              type="checkbox"
              checked={w.confirmBinds}
              onChange={(e) => w.setConfirmBinds(e.target.checked)}
              className="rounded border-[var(--border)]"
            />
            <span className="font-medium text-[var(--text)]">
              {t(locale, 'workshop.githubConfirmBinds')}
            </span>
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
          className="h-9 px-4 text-xs font-medium rounded-lg border border-[var(--border)] bg-[var(--surface)] text-[var(--text)] hover:bg-[var(--surface-hover)] transition-[color,background-color,border-color,opacity,transform]"
          onClick={() => w.setStep('url')}
        >
          {t(locale, 'common.back')}
        </button>
        <button
          type="button"
          className="h-9 px-4 text-xs font-medium rounded-lg bg-[var(--primary)] text-[var(--accent-ink)] hover:opacity-90 transition-[color,background-color,border-color,opacity,transform] flex items-center gap-1.5"
          disabled={!w.selectedCandidate || inspect.blockers.length > 0}
          onClick={() => {
            if (inspect && w.selectedCandidate) void w.runInstall(inspect, w.selectedCandidate, false);
          }}
        >
          <span>{t(locale, 'workshop.githubInstall')}</span>
        </button>
      </div>
    </div>
  );
}
