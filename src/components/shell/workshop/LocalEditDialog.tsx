'use client';

import { forwardRef, useImperativeHandle } from 'react';
import { t, useLocale } from '@/i18n';
import Modal from '@/components/ui/Modal';
import { planSummaryLines } from '@/lib/local-creative';
import { useLocalEditState } from '@/hooks/useLocalEditState';
import type { CreativeAppSummary } from '@/lib/tauri-adapter';

export interface LocalEditDialogHandle {
  open: (app: CreativeAppSummary) => void;
}

export interface LocalEditDialogProps {
  onToast: (message: string) => void;
  onSaved: () => void;
}

/**
 * Settings editor for an existing local creative app. Loads the Host launch
 * config on open so the form always reflects the true stored plan.
 */
const LocalEditDialog = forwardRef<LocalEditDialogHandle, LocalEditDialogProps>(
  function LocalEditDialog({ onToast, onSaved }, ref) {
    const locale = useLocale();
    const edit = useLocalEditState({ onToast, onSaved });

    useImperativeHandle(ref, () => ({ open: edit.open }), [edit.open]);

    if (!edit.app) return null;

    return (
      <Modal isOpen onClose={edit.close} title={t(locale, 'workshop.actionEdit')} width={520}>
        <div className="flex flex-col gap-3 py-1 text-xs">
          {edit.loading && <div>{t(locale, 'common.loading')}</div>}
          <div>
            <label className="block text-xs mb-1">{t(locale, 'workshop.templateName')}</label>
            <input
              value={edit.title}
              onChange={(e) => edit.setTitle(e.target.value)}
              className="w-full h-9 px-3 text-xs rounded-lg border border-[var(--border)] bg-[var(--surface-subtle)]"
            />
          </div>
          <label className="flex items-center gap-2 text-xs">
            <input
              type="checkbox"
              checked={edit.autoOpen}
              onChange={(e) => edit.setAutoOpen(e.target.checked)}
            />
            {t(locale, 'workshop.localAutoOpen')}
          </label>
          {edit.plan && (
            <>
              <div className="grid grid-cols-2 gap-2">
                <div>
                  <label className="block mb-1">{t(locale, 'workshop.localCwd')}</label>
                  <input
                    value={edit.cwd}
                    onChange={(e) => edit.setCwd(e.target.value)}
                    className="w-full h-8 px-2 rounded border border-[var(--border)] bg-[var(--surface-subtle)]"
                  />
                </div>
                <div>
                  <label className="block mb-1">{t(locale, 'workshop.localScript')}</label>
                  <input
                    value={edit.script}
                    onChange={(e) => edit.setScript(e.target.value)}
                    className="w-full h-8 px-2 rounded border border-[var(--border)] bg-[var(--surface-subtle)]"
                  />
                </div>
                <div>
                  <label className="block mb-1">{t(locale, 'workshop.localOpenPath')}</label>
                  <input
                    value={edit.openPath}
                    onChange={(e) => edit.setOpenPath(e.target.value)}
                    className="w-full h-8 px-2 rounded border border-[var(--border)] bg-[var(--surface-subtle)]"
                  />
                </div>
                <div>
                  <label className="block mb-1">{t(locale, 'workshop.localPort')}</label>
                  <div className="flex gap-1">
                    <select
                      value={edit.portMode}
                      onChange={(e) => edit.setPortMode(e.target.value as 'auto' | 'fixed')}
                      className="h-8 px-1 rounded border border-[var(--border)] bg-[var(--surface-subtle)]"
                    >
                      <option value="auto">{t(locale, 'workshop.portAuto')}</option>
                      <option value="fixed">{t(locale, 'workshop.portFixed')}</option>
                    </select>
                    {edit.portMode === 'fixed' && (
                      <input
                        value={edit.portValue}
                        onChange={(e) => edit.setPortValue(e.target.value)}
                        className="w-20 h-8 px-2 rounded border border-[var(--border)] bg-[var(--surface-subtle)]"
                      />
                    )}
                  </div>
                </div>
              </div>
              <div className="text-[11px] text-[var(--text-secondary)]">
                {t(locale, 'workshop.envKeys')}:{' '}
                {edit.envKeys.length ? edit.envKeys.join(', ') : '—'}
              </div>
              <pre className="p-2 rounded border border-[var(--border)] bg-[var(--surface-subtle)] whitespace-pre-wrap">
                {planSummaryLines(edit.plan, locale).join('\n')}
              </pre>
            </>
          )}
          <div className="flex justify-end gap-2">
            <button
              type="button"
              className="h-9 px-4 text-xs rounded-lg border border-[var(--border)]"
              onClick={edit.close}
            >
              {t(locale, 'common.cancel')}
            </button>
            <button
              type="button"
              className="h-9 px-4 text-xs rounded-lg bg-[var(--primary)] text-[var(--accent-ink)] disabled:opacity-50"
              disabled={edit.saving || edit.loading}
              onClick={() => void edit.save()}
            >
              {t(locale, 'common.save')}
            </button>
          </div>
        </div>
      </Modal>
    );
  },
);

export default LocalEditDialog;
