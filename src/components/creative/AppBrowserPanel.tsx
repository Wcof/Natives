//! Embedded child-WebView preview panel (batch 7: extracted from WorkshopPage).
//!
//! Toolbar: back/forward/reload, stop/restart for the running app, address,
//! copy URL, open in system browser, close. The host div below the toolbar is
//! where the Tauri child webview is placed.

import React from 'react';
import { ChevronLeft, ChevronRight, ExternalLink, Pause, RefreshCw, RotateCcw, X } from 'lucide-react';
import { useLocale, t } from '@/i18n';
import { classifyError } from '@/lib/error-classifier';
import type { CreativeAppSummary } from '@/lib/tauri-adapter';

export interface AppBrowserPanelProps {
  app: CreativeAppSummary;
  url: string;
  hostRef: React.RefObject<HTMLDivElement | null>;
  onStop: (app: CreativeAppSummary) => void;
  onRestart: (app: CreativeAppSummary) => void;
  onClose: () => void;
  onToast: (message: string) => void;
}

export default function AppBrowserPanel({
  app,
  url,
  hostRef,
  onStop,
  onRestart,
  onClose,
  onToast,
}: AppBrowserPanelProps) {
  const locale = useLocale();
  return (
    <div className="flex flex-col h-full bg-[var(--background)]">
      <div className="flex items-center gap-2 px-4 py-2.5 border-b border-[var(--border)] bg-[var(--surface)] shrink-0">
        <button
          type="button"
          className="flex h-8 w-8 items-center justify-center rounded-lg border border-[var(--border)] text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)] transition-[color,background-color,border-color,opacity,transform]"
          onClick={() => void window.nativesAPI?.creativeApp?.browserBack?.(app.id)}
          title={t(locale, 'workshop.browserBack')}
        >
          <ChevronLeft size={14} />
        </button>
        <button
          type="button"
          className="flex h-8 w-8 items-center justify-center rounded-lg border border-[var(--border)] text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)] transition-[color,background-color,border-color,opacity,transform]"
          onClick={() => void window.nativesAPI?.creativeApp?.browserForward?.(app.id)}
          title={t(locale, 'workshop.browserForward')}
        >
          <ChevronRight size={14} />
        </button>
        <button
          type="button"
          className="flex h-8 w-8 items-center justify-center rounded-lg border border-[var(--border)] text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)] transition-[color,background-color,border-color,opacity,transform]"
          onClick={() => void window.nativesAPI?.creativeApp?.browserReload?.(app.id)}
          title={t(locale, 'workshop.browserReload')}
        >
          <RefreshCw size={14} />
        </button>
        {app.source !== 'internal' && (
          <>
            <button
              type="button"
              className="flex h-8 w-8 items-center justify-center rounded-lg border border-[var(--border)] text-[var(--text-secondary)] hover:bg-[var(--surface-hover)]"
              onClick={() => onStop(app)}
              title={t(locale, 'workshop.actionStop')}
            >
              <Pause size={14} />
            </button>
            <button
              type="button"
              className="flex h-8 w-8 items-center justify-center rounded-lg border border-[var(--border)] text-[var(--text-secondary)] hover:bg-[var(--surface-hover)]"
              onClick={() => onRestart(app)}
              title={t(locale, 'workshop.actionRestart')}
            >
              <RotateCcw size={14} />
            </button>
          </>
        )}
        <div
          className="flex-1 text-xs font-mono text-[var(--text-secondary)] px-3 py-1.5 border border-[var(--border)] rounded-lg bg-[var(--surface-subtle)] truncate"
          title={url}
        >
          {url || t(locale, 'workshop.browserAddress')}
        </div>
        <button
          type="button"
          className="flex h-8 items-center gap-1.5 px-2 rounded-lg border border-[var(--border)] text-xs"
          onClick={async () => {
            try {
              await window.nativesAPI?.clipboard?.write?.(url);
              onToast(t(locale, 'workshop.copied'));
            } catch (err) {
              onToast(classifyError(err).userMessage);
            }
          }}
          title={t(locale, 'workshop.copyUrl')}
        >
          {t(locale, 'workshop.copyUrl')}
        </button>
        <button
          type="button"
          className="flex h-8 items-center gap-1.5 px-2 rounded-lg border border-[var(--border)] text-xs"
          onClick={async () => {
            if (!url) return;
            try {
              await window.nativesAPI?.shell?.openPath?.(url);
            } catch (err) {
              onToast(classifyError(err).userMessage);
            }
          }}
          title={t(locale, 'workshop.openSystemBrowser')}
        >
          <ExternalLink size={13} />
        </button>
        <button
          type="button"
          className="flex h-8 items-center gap-1.5 px-3 rounded-lg border border-[var(--border)] bg-[var(--surface)] text-xs font-medium text-[var(--text)] hover:bg-[var(--surface-hover)] transition-[color,background-color,border-color,opacity,transform]"
          onClick={onClose}
        >
          <X size={14} />
          {t(locale, 'workshop.browserBackToList')}
        </button>
      </div>
      <div ref={hostRef} className="flex-1 min-h-0 bg-[var(--background)]" />
    </div>
  );
}
