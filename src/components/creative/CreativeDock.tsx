//! Creative App Dock (batch 5 CR-503).
//!
//! Shows running apps from the catalog as a dock bar. Each app displays its
//! icon, state indicator, and allows click-to-focus. The dock is a projection
//! of runtime/operation/window state — no fake state.

import React, { useMemo } from 'react';
import { useLocale, t } from '@/i18n';
import type { CreativeAppSummary } from '@/lib/tauri-adapter';

export interface CreativeDockProps {
  apps: CreativeAppSummary[];
  activeAppId: string | null;
  onSelect: (app: CreativeAppSummary) => void;
  onClose: (app: CreativeAppSummary) => void;
}

export default function CreativeDock({ apps, activeAppId, onSelect, onClose }: CreativeDockProps) {
  const locale = useLocale();

  // Only show apps that are running or starting (the dock is for active sessions)
  const activeApps = useMemo(
    () => apps.filter((a) => a.state === 'running' || a.state === 'starting'),
    [apps],
  );

  if (activeApps.length === 0) return null;

  return (
    <div
      className="flex items-center gap-1 px-2 py-1.5 border-t border-[var(--border)] bg-[var(--surface)] overflow-x-auto"
      role="toolbar"
      aria-label={t(locale, 'workshop.dockLabel')}
    >
      {activeApps.map((app) => {
        const isActive = app.id === activeAppId;
        const isRunning = app.state === 'running';
        return (
          <button
            key={app.id}
            type="button"
            className={`flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg text-xs transition-all
              ${isActive
                ? 'bg-[var(--accent)] text-[var(--on-accent)] shadow-sm'
                : 'text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)]'
              }`}
            onClick={() => onSelect(app)}
            title={`${app.title} — ${isRunning ? t(locale, 'workshop.stateRunning') : t(locale, 'workshop.stateStarting')}`}
            role="tab"
            aria-selected={isActive}
          >
            {/* State indicator */}
            <span
              className={`w-1.5 h-1.5 rounded-full shrink-0 ${
                isRunning ? 'bg-[var(--success)]' : 'bg-[var(--warning)] animate-pulse'
              }`}
            />
            {/* App icon or first letter */}
            <span className="font-medium truncate max-w-[120px]">
              {app.icon || app.title.charAt(0).toUpperCase()}
            </span>
            {/* Close button — only for non-internal apps */}
            {app.source !== 'internal' && (
              <button
                type="button"
                className="ml-1 p-0.5 rounded hover:bg-[var(--surface-subtle)] opacity-60 hover:opacity-100"
                onClick={(e) => {
                  e.stopPropagation();
                  onClose(app);
                }}
                title={t(locale, 'workshop.actionClose')}
                aria-label={`${t(locale, 'workshop.actionClose')} ${app.title}`}
              >
                <svg width="10" height="10" viewBox="0 0 10 10" fill="currentColor">
                  <path d="M1 1l8 8m0-8L1 9" stroke="currentColor" strokeWidth="1.5" fill="none" />
                </svg>
              </button>
            )}
          </button>
        );
      })}
    </div>
  );
}