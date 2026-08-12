//! Creative App Dock (T10).
//!
//! Shows running apps (and their T07 window snapshot) as a tablist dock bar.
//! Each entry follows the tablist pattern: a `role="tab"` button with SIBLING
//! minimize/close buttons — never nested interactive controls. Open/focus/
//! minimize/close drive the real Window API (`creativeApp.windowOpen /
//! windowRestore / windowMinimize / windowClose`), so the dock is a projection
//! of runtime/operation/window state — no fake local state.

import React, { useCallback } from 'react';
import { useLocale, t } from '@/i18n';
import { Minimize2, X } from 'lucide-react';
// W4: shared tab type moved to lib so hooks never depend on component internals.
export type { CreativeDockTab } from '@/lib/creative-dock-types';
import type { CreativeDockTab } from '@/lib/creative-dock-types';

export interface CreativeDockProps {
  tabs: CreativeDockTab[];
  activeKey: string | null;
  onSelect: (tab: CreativeDockTab) => void;
  onMinimize: (tab: CreativeDockTab) => void;
  onClose: (tab: CreativeDockTab) => void;
}

export default function CreativeDock({
  tabs,
  activeKey,
  onSelect,
  onMinimize,
  onClose,
}: CreativeDockProps) {
  const locale = useLocale();
  const tabRefs = React.useRef<Map<string, HTMLButtonElement | null>>(new Map());

  /** Tablist keyboard nav: ArrowLeft/Right + Home/End (R-U17 keyboard path). */
  const handleKeyDown = useCallback(
    (event: React.KeyboardEvent<HTMLDivElement>) => {
      const current = event.target as HTMLElement | null;
      const currentKey = current?.dataset?.dockTabKey;
      if (!currentKey) return;
      const index = tabs.findIndex((tab) => tab.key === currentKey);
      if (index < 0) return;
      let nextIndex = index;
      if (event.key === 'ArrowRight') nextIndex = (index + 1) % tabs.length;
      else if (event.key === 'ArrowLeft') nextIndex = (index - 1 + tabs.length) % tabs.length;
      else if (event.key === 'Home') nextIndex = 0;
      else if (event.key === 'End') nextIndex = tabs.length - 1;
      else return;
      event.preventDefault();
      const next = tabs[nextIndex];
      if (!next) return;
      tabRefs.current.get(next.key)?.focus();
    },
    [tabs],
  );

  if (tabs.length === 0) return null;

  return (
    <div
      role="tablist"
      aria-label={t(locale, 'workshop.dockLabel')}
      onKeyDown={handleKeyDown}
      className="flex items-center gap-1 px-2 py-1.5 border-t border-[var(--border)] bg-[var(--surface)] overflow-x-auto shrink-0"
    >
      {tabs.map((tab) => {
        const isActive = tab.key === activeKey;
        const isRunning = tab.state === 'open';
        const appStateLabel = isRunning
          ? t(locale, 'workshop.stateRunning')
          : tab.state === 'minimized'
            ? t(locale, 'workshop.stateMinimized')
            : t(locale, 'workshop.stateStarting');
        const tabLabel = `${tab.app.title} — ${appStateLabel}`;
        return (
          <div key={tab.key} role="presentation" className="flex items-center gap-0.5 shrink-0">
            <button
              ref={(el) => {
                if (el) tabRefs.current.set(tab.key, el);
                else tabRefs.current.delete(tab.key);
              }}
              type="button"
              role="tab"
              data-dock-tab-key={tab.key}
              aria-selected={isActive}
              title={tabLabel}
              aria-label={tabLabel}
              onClick={() => onSelect(tab)}
              className={`flex items-center gap-1.5 pl-2 pr-1.5 py-1.5 rounded-lg text-xs transition-[color,background-color,border-color,opacity,transform] ${
                isActive
                  ? 'bg-[var(--accent)] text-[var(--on-accent)] shadow-sm'
                  : 'text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)]'
              }`}
            >
              <span
                className={`w-1.5 h-1.5 rounded-full shrink-0 ${
                  isRunning
                    ? 'bg-[var(--success)]'
                    : tab.state === 'minimized'
                      ? 'bg-[var(--text-disabled)]'
                      : 'bg-[var(--warning)] animate-pulse'
                }`}
              />
              <span className="font-medium truncate max-w-[120px]">
                {tab.app.icon || tab.app.title.charAt(0).toUpperCase()}
              </span>
            </button>
            {tab.window && tab.state === 'open' && (
              <button
                type="button"
                aria-label={`${t(locale, 'workshop.dockMinimize')} ${tab.app.title}`}
                title={`${t(locale, 'workshop.dockMinimize')} ${tab.app.title}`}
                onClick={() => onMinimize(tab)}
                className="p-0.5 rounded hover:bg-[var(--surface-subtle)] opacity-60 hover:opacity-100 text-[var(--text-secondary)]"
              >
                <Minimize2 size={10} />
              </button>
            )}
            {tab.window && (
              <button
                type="button"
                aria-label={`${t(locale, 'workshop.dockClose')} ${tab.app.title}`}
                title={`${t(locale, 'workshop.dockClose')} ${tab.app.title}`}
                onClick={() => onClose(tab)}
                className="p-0.5 pr-1.5 rounded hover:bg-[var(--surface-subtle)] opacity-60 hover:opacity-100 text-[var(--text-secondary)]"
              >
                <X size={10} />
              </button>
            )}
          </div>
        );
      })}
    </div>
  );
}
