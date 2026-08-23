'use client';

import React, { useState } from 'react';
import { X, Folder, Laptop, Globe } from 'lucide-react';
import { t, useLocale } from '@/i18n';
import type { AppView, AppKind } from '@/lib/tauri/apps';
import { LocalProjectForm } from './add/LocalProjectForm';
import { SystemApplicationForm } from './add/SystemApplicationForm';
import { WebApplicationForm } from './add/WebApplicationForm';

interface AddAppDialogProps {
  isOpen: boolean;
  initialKind?: AppKind;
  onSuccess: (app: AppView) => void;
  onClose: () => void;
}

export function AddAppDialog({
  isOpen,
  initialKind = 'local_project',
  onSuccess,
  onClose,
}: AddAppDialogProps) {
  const locale = useLocale();
  const [activeTab, setActiveTab] = useState<AppKind>(initialKind);

  if (!isOpen) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-[var(--surface-overlay)]/60 backdrop-blur-sm p-4 animate-in fade-in duration-150"
      role="dialog"
      aria-modal="true"
      aria-labelledby="add-app-title"
    >
      <div className="relative w-full max-w-lg rounded-2xl border border-[var(--border-default)] bg-[var(--surface-overlay)] p-6 shadow-2xl space-y-5">
        <div className="flex items-center justify-between">
          <h2 id="add-app-title" className="text-base font-semibold text-[var(--text-primary)]">
            {t(locale, 'appsPage.modalTitle')}
          </h2>
          <button
            type="button"
            onClick={onClose}
            className="rounded-lg p-1 text-[var(--text-tertiary)] hover:bg-[var(--surface-muted)] hover:text-[var(--text-primary)] transition-colors"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        {/* 3-Kind Tab Switcher */}
        <div className="flex rounded-xl border border-[var(--border-subtle)] bg-[var(--surface-muted)] p-1">
          <button
            type="button"
            onClick={() => setActiveTab('local_project')}
            className={`flex-1 flex items-center justify-center gap-1.5 py-1.5 text-xs font-medium rounded-lg transition-all ${
              activeTab === 'local_project'
                ? 'bg-[var(--surface-overlay)] text-[var(--text-primary)] shadow-sm font-semibold'
                : 'text-[var(--text-secondary)] hover:text-[var(--text-primary)]'
            }`}
          >
            <Folder className="h-3.5 w-3.5" />
            {t(locale, 'appsPage.addLocal')}
          </button>

          <button
            type="button"
            onClick={() => setActiveTab('system_application')}
            className={`flex-1 flex items-center justify-center gap-1.5 py-1.5 text-xs font-medium rounded-lg transition-all ${
              activeTab === 'system_application'
                ? 'bg-[var(--surface-overlay)] text-[var(--text-primary)] shadow-sm font-semibold'
                : 'text-[var(--text-secondary)] hover:text-[var(--text-primary)]'
            }`}
          >
            <Laptop className="h-3.5 w-3.5" />
            {t(locale, 'appsPage.addSystem')}
          </button>

          <button
            type="button"
            onClick={() => setActiveTab('web_application')}
            className={`flex-1 flex items-center justify-center gap-1.5 py-1.5 text-xs font-medium rounded-lg transition-all ${
              activeTab === 'web_application'
                ? 'bg-[var(--surface-overlay)] text-[var(--text-primary)] shadow-sm font-semibold'
                : 'text-[var(--text-secondary)] hover:text-[var(--text-primary)]'
            }`}
          >
            <Globe className="h-3.5 w-3.5" />
            {t(locale, 'appsPage.addWeb')}
          </button>
        </div>

        {/* Form Body */}
        {activeTab === 'local_project' && (
          <LocalProjectForm onSuccess={onSuccess} onCancel={onClose} />
        )}
        {activeTab === 'system_application' && (
          <SystemApplicationForm onSuccess={onSuccess} onCancel={onClose} />
        )}
        {activeTab === 'web_application' && (
          <WebApplicationForm onSuccess={onSuccess} onCancel={onClose} />
        )}
      </div>
    </div>
  );
}
