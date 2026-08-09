'use client';

import { PanelRightClose, PanelRightOpen } from 'lucide-react';
import type { Locale } from '@/i18n';
import { t } from '@/i18n';

export interface WorkbenchHeaderProps {
  locale: Locale;
  onOpenPalette: () => void;
  showRight: boolean;
  onToggleRightPanel: () => void;
}

/**
 * Top toolbar of the workbench: command palette hint + activity panel toggle.
 *
 * Pure presentation — all behavior arrives as callbacks from the shell so the
 * header never reads the store itself.
 */
export function WorkbenchHeader({
  locale,
  onOpenPalette,
  showRight,
  onToggleRightPanel,
}: WorkbenchHeaderProps) {
  return (
    <div className="flex items-center justify-end gap-1 border-b border-[var(--border)] px-3 py-1">
      <button
        type="button"
        onClick={onOpenPalette}
        className="rounded px-2 py-1 text-[11px] text-[var(--text-disabled)] hover:bg-[var(--surface-hover)] hover:text-[var(--text-secondary)]"
        title={t(locale, 'assistant.commandPaletteHint')}
      >
        {t(locale, 'assistant.commands')}
      </button>
      <button
        type="button"
        onClick={onToggleRightPanel}
        className="rounded p-1.5 text-[var(--text-disabled)] hover:bg-[var(--surface-hover)] hover:text-[var(--text-secondary)]"
        title={t(locale, 'assistant.activityPanel')}
        aria-label={t(locale, 'assistant.toggleActivityPanel')}
      >
        {showRight ? <PanelRightClose size={16} /> : <PanelRightOpen size={16} />}
      </button>
    </div>
  );
}

export default WorkbenchHeader;
