//! App runtime log panel (batch 7: extracted from WorkshopPage).
//!
//! Renders the logs modal for one creative app: auto-scroll toggle, filter,
//! copy/clear/refresh, and the AI diagnose action for local projects. The log
//! buffer itself is bounded upstream (the Renderer caps its string).

import React from 'react';
import { useLocale, t } from '@/i18n';
import Modal from '@/components/ui/Modal';
import { classifyError } from '@/lib/error-classifier';
import type { CreativeAppSummary } from '@/lib/tauri-adapter';

export interface AppLogsPanelProps {
  app: CreativeAppSummary | null;
  logsText: string;
  logFilter: string;
  logAutoScroll: boolean;
  preRef: React.RefObject<HTMLPreElement | null>;
  onClose: () => void;
  onSetFilter: (v: string) => void;
  onSetAutoScroll: (v: boolean) => void;
  onClear: () => void;
  onRefresh: () => void;
  onToast: (message: string) => void;
}

export default function AppLogsPanel({
  app,
  logsText,
  logFilter,
  logAutoScroll,
  preRef,
  onClose,
  onSetFilter,
  onSetAutoScroll,
  onClear,
  onRefresh,
  onToast,
}: AppLogsPanelProps) {
  const locale = useLocale();
  if (!app) return null;
  return (
    <Modal isOpen onClose={onClose} title={t(locale, 'workshop.logsTitle')} width={720}>
      <div className="py-1 flex flex-col gap-2">
        <div className="flex items-center justify-between gap-2 flex-wrap">
          <label className="flex items-center gap-2 text-[11px] text-[var(--text-secondary)]">
            <input
              type="checkbox"
              checked={logAutoScroll}
              onChange={(e) => onSetAutoScroll(e.target.checked)}
            />
            {t(locale, 'workshop.logsAutoScroll')}
          </label>
          <input
            value={logFilter}
            onChange={(e) => onSetFilter(e.target.value)}
            placeholder={t(locale, 'workshop.logsFilter')}
            className="h-7 px-2 text-[11px] rounded border border-[var(--border)] bg-[var(--surface-subtle)] min-w-[160px]"
          />
          <div className="flex gap-2 flex-wrap">
            <button
              type="button"
              className="h-7 px-2 text-[11px] rounded border border-[var(--border)]"
              onClick={async () => {
                try {
                  await window.nativesAPI?.clipboard?.write?.(logsText || '');
                  onToast(t(locale, 'workshop.copied'));
                } catch (err) {
                  onToast(classifyError(err).userMessage);
                }
              }}
            >
              {t(locale, 'workshop.logsCopy')}
            </button>
            <button
              type="button"
              className="h-7 px-2 text-[11px] rounded border border-[var(--border)]"
              onClick={onClear}
            >
              {t(locale, 'workshop.logsClearView')}
            </button>
            <button
              type="button"
              className="h-7 px-2 text-[11px] rounded border border-[var(--border)]"
              onClick={onRefresh}
            >
              {t(locale, 'common.refresh')}
            </button>
            {app.source === 'local_project' && (
              <button
                type="button"
                className="h-7 px-2 text-[11px] rounded border border-[var(--border)]"
                onClick={async () => {
                  try {
                    const d = await window.nativesAPI?.creativeApp?.diagnoseLocalWithAi?.(
                      app.id,
                    );
                    if (d) {
                      onToast(`${d.issueCode}: ${d.summary}`);
                    }
                  } catch (err) {
                    onToast(classifyError(err).userMessage);
                  }
                }}
              >
                {t(locale, 'workshop.localAiDiagnose')}
              </button>
            )}
          </div>
        </div>
        <pre
          ref={preRef}
          className="max-h-96 overflow-auto text-[11px] font-mono bg-[var(--background)] p-4 rounded-xl border border-[var(--border)] leading-relaxed whitespace-pre-wrap"
        >
          {(logFilter
            ? logsText
                .split('\n')
                .filter((line) => line.toLowerCase().includes(logFilter.toLowerCase()))
                .join('\n')
            : logsText
          )
            .split('\n')
            .map((line, i) => {
              const color = line.includes('[stderr]')
                ? 'text-[var(--danger)]'
                : line.includes('[system]')
                  ? 'text-[var(--warning)]'
                  : 'text-[var(--text-body)]';
              return (
                <div key={i} className={color}>
                  {line}
                </div>
              );
            })}
        </pre>
      </div>
    </Modal>
  );
}
