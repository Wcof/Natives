'use client';

import React from 'react';
import { Loader } from 'lucide-react';
import { t, useLocale } from '@/i18n';
import Modal from '@/components/ui/Modal';
import type { ModuleImportDialogData } from '@/hooks/useModuleImportWizard';

export interface ModuleImportDialogProps {
  dialog: ModuleImportDialogData | null;
  selected: ReadonlySet<string>;
  onSelect: (next: Set<string>) => void;
  installing: boolean;
  onClose: () => void;
  onConfirm: () => void;
}

/**
 * Module-import permission dialog: reads the manifest's declared permissions
 * and lets the user pick which ones to grant before install.
 */
export default function ModuleImportDialog({
  dialog,
  selected,
  onSelect,
  installing,
  onClose,
  onConfirm,
}: ModuleImportDialogProps) {
  const locale = useLocale();
  if (!dialog) return null;
  return (
    <Modal
      isOpen
      onClose={onClose}
      title={t(locale, 'workshop.permissionTitle')}
      width={440}
    >
      <div className="flex flex-col gap-3 py-1">
        <p className="text-xs text-[var(--text-secondary)]">
          {t(locale, 'workshop.permissionDesc').replace('{name}', dialog.moduleName)}
        </p>
        <div className="bg-[var(--surface-subtle)] p-3 rounded-lg border border-[var(--border)] max-h-48 overflow-y-auto">
          <ul className="space-y-2 text-xs text-[var(--text)]">
            {dialog.permissions.map((p) => (
              <li key={p} className="flex items-center gap-2">
                <input
                  type="checkbox"
                  id={`perm-${p}`}
                  checked={selected.has(p)}
                  onChange={(e) => {
                    const next = new Set(selected);
                    if (e.target.checked) next.add(p);
                    else next.delete(p);
                    onSelect(next);
                  }}
                  className="rounded border-[var(--border)] text-[var(--primary)] focus:ring-0"
                />
                <label htmlFor={`perm-${p}`} className="cursor-pointer font-mono text-[11px]">
                  {p}
                </label>
              </li>
            ))}
          </ul>
        </div>
        <div className="flex justify-end gap-2 pt-2">
          <button
            type="button"
            className="h-9 px-4 text-xs font-medium rounded-lg border border-[var(--border)] bg-[var(--surface)] text-[var(--text)] hover:bg-[var(--surface-hover)] transition-[color,background-color,border-color,opacity,transform]"
            onClick={onClose}
          >
            {t(locale, 'common.cancel')}
          </button>
          <button
            type="button"
            className="h-9 px-4 text-xs font-medium rounded-lg bg-[var(--primary)] text-[var(--accent-ink)] hover:opacity-90 transition-[color,background-color,border-color,opacity,transform] flex items-center gap-1.5"
            disabled={installing}
            onClick={onConfirm}
          >
            {installing ? <Loader size={14} className="animate-spin" /> : null}
            <span>{t(locale, 'workshop.permissionAllowAll')}</span>
          </button>
        </div>
      </div>
    </Modal>
  );
}
