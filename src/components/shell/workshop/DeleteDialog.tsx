'use client';

import { useCallback, useState } from 'react';
import { Trash2 } from 'lucide-react';
import { t, useLocale } from '@/i18n';
import Modal from '@/components/ui/Modal';
import { classifyError } from '@/lib/error-classifier';
import { deleteLocalConfirmNote } from '@/lib/local-creative';
import { deleteNeedsDockerOptions } from '@/lib/creative-app';
import type { CreativeAppSummary } from '@/lib/tauri-adapter';

export interface DeleteDialogProps {
  target: CreativeAppSummary | null;
  onClose: () => void;
  onDeleted: () => void;
  withBusy: <T>(id: string, fn: () => Promise<T>) => Promise<T | undefined>;
  closeBrowser: () => Promise<void>;
  onToast: (message: string) => void;
}

/**
 * Delete confirmation for a creative app. Docker volume/image options stay
 * meaningful only for external_github; local never touches project files and
 * internal uninstalls the module — the Host decides per source.
 */
export default function DeleteDialog({
  target,
  onClose,
  onDeleted,
  withBusy,
  closeBrowser,
  onToast,
}: DeleteDialogProps) {
  const locale = useLocale();
  const [deleteVolumes, setDeleteVolumes] = useState(false);
  const [deleteImages, setDeleteImages] = useState(false);

  const doDelete = useCallback(async () => {
    if (!target) return;
    const id = target.id;
    await closeBrowser();
    await withBusy(id, async () => {
      try {
        await window.nativesAPI?.creativeApp?.delete?.(id, {
          removeVolumes: deleteVolumes,
          removeImages: deleteImages,
        });
        onDeleted();
      } catch (err) {
        onToast(classifyError(err).userMessage);
      } finally {
        onClose();
        setDeleteVolumes(false);
        setDeleteImages(false);
      }
    });
  }, [closeBrowser, deleteImages, deleteVolumes, onClose, onDeleted, onToast, target, withBusy]);

  if (!target) return null;

  return (
    <Modal isOpen onClose={onClose} title={t(locale, 'workshop.deleteTitle')} width={420}>
      <div className="flex flex-col gap-3 py-1">
        <p className="text-xs text-[var(--text-secondary)]">
          {target.source === 'local_project'
            ? deleteLocalConfirmNote(locale)
            : t(locale, 'workshop.deleteDesc')}
        </p>
        <div className="p-3 bg-[var(--surface-subtle)] border border-[var(--border)] rounded-lg font-semibold text-sm text-[var(--text)]">
          {target.title}
        </div>

        {deleteNeedsDockerOptions(target.source) && (
          <div className="flex flex-col gap-2 pt-1">
            <label className="flex items-center gap-2 text-xs text-[var(--text)] cursor-pointer">
              <input
                type="checkbox"
                checked={deleteVolumes}
                onChange={(e) => setDeleteVolumes(e.target.checked)}
                className="rounded border-[var(--border)]"
              />
              <span>{t(locale, 'workshop.deleteRemoveVolumes')}</span>
            </label>
            <label className="flex items-center gap-2 text-xs text-[var(--text)] cursor-pointer">
              <input
                type="checkbox"
                checked={deleteImages}
                onChange={(e) => setDeleteImages(e.target.checked)}
                className="rounded border-[var(--border)]"
              />
              <span>{t(locale, 'workshop.deleteRemoveImages')}</span>
            </label>
          </div>
        )}

        <div className="flex justify-end gap-2 pt-3">
          <button
            type="button"
            className="h-9 px-4 text-xs font-medium rounded-lg border border-[var(--border)] bg-[var(--surface)] text-[var(--text)] hover:bg-[var(--surface-hover)] transition-all"
            onClick={onClose}
          >
            {t(locale, 'common.cancel')}
          </button>
          <button
            type="button"
            className="h-9 px-4 text-xs font-medium rounded-lg bg-[var(--danger)] text-[var(--accent-ink)] hover:opacity-90 transition-all flex items-center gap-1.5"
            onClick={() => void doDelete()}
          >
            <Trash2 size={14} />
            <span>{t(locale, 'workshop.deleteConfirm')}</span>
          </button>
        </div>
      </div>
    </Modal>
  );
}
