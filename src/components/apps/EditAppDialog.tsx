'use client';

import React from 'react';
import { X } from 'lucide-react';
import { t, useLocale } from '@/i18n';
import type { AppView } from '@/lib/tauri/apps';
import { LocalProjectEdit } from './edit/LocalProjectEdit';
import { SystemApplicationEdit } from './edit/SystemApplicationEdit';
import { WebApplicationEdit } from './edit/WebApplicationEdit';

interface EditAppDialogProps {
  isOpen: boolean;
  app: AppView | null;
  onSuccess: (updated: AppView) => void;
  onClose: () => void;
}

export function EditAppDialog({ isOpen, app, onSuccess, onClose }: EditAppDialogProps) {
  const locale = useLocale();
  if (!isOpen || !app) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-[var(--surface-overlay)]/60 backdrop-blur-sm p-4 animate-in fade-in duration-150"
      role="dialog"
      aria-modal="true"
      aria-labelledby="edit-app-title"
    >
      <div className="relative w-full max-w-lg rounded-2xl border border-[var(--border-default)] bg-[var(--surface-overlay)] p-6 shadow-2xl space-y-4">
        <div className="flex items-center justify-between">
          <h2 id="edit-app-title" className="text-base font-semibold text-[var(--text-primary)]">
            {t(locale, 'appsPage.editApp')}: {app.title}
          </h2>
          <button
            type="button"
            onClick={onClose}
            className="rounded-lg p-1 text-[var(--text-tertiary)] hover:bg-[var(--surface-muted)] hover:text-[var(--text-primary)] transition-colors"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        {app.kind === 'local_project' && (
          <LocalProjectEdit app={app} onSuccess={onSuccess} onCancel={onClose} />
        )}
        {app.kind === 'system_application' && (
          <SystemApplicationEdit app={app} onSuccess={onSuccess} onCancel={onClose} />
        )}
        {app.kind === 'web_application' && (
          <WebApplicationEdit app={app} onSuccess={onSuccess} onCancel={onClose} />
        )}
      </div>
    </div>
  );
}
