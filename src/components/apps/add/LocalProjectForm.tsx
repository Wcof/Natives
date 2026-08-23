'use client';

import React, { useState } from 'react';
import { Search, CheckCircle2, AlertCircle } from 'lucide-react';
import { t, useLocale } from '@/i18n';
import { appsApi, type AppView, type LocalProjectScanResult } from '@/lib/tauri/apps';

interface LocalProjectFormProps {
  onSuccess: (app: AppView) => void;
  onCancel: () => void;
}

export function LocalProjectForm({ onSuccess, onCancel }: LocalProjectFormProps) {
  const locale = useLocale();
  const [title, setTitle] = useState('');
  const [projectRoot, setProjectRoot] = useState('');
  const [description, setDescription] = useState('');
  const [scanning, setScanning] = useState(false);
  const [scanResult, setScanResult] = useState<LocalProjectScanResult | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleInspect = async () => {
    if (!projectRoot.trim()) return;
    setScanning(true);
    setError(null);
    try {
      const res = await appsApi.localInspect(projectRoot.trim());
      setScanResult(res);
      if (!title.trim()) {
        const folderName = projectRoot.trim().split(/[/\\]/).filter(Boolean).pop() || '';
        if (folderName) setTitle(folderName);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setScanning(false);
    }
  };

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!title.trim() || !projectRoot.trim()) {
      setError('Please fill in required fields');
      return;
    }

    setSubmitting(true);
    setError(null);
    try {
      const app = await appsApi.registerLocal({
        title: title.trim(),
        projectRoot: projectRoot.trim(),
        description: description.trim() || undefined,
      });
      onSuccess(app);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <form onSubmit={handleSubmit} className="space-y-4">
      {error && (
        <div className="flex items-center gap-2 rounded-xl border border-[var(--danger)]/20 bg-[var(--danger-soft)] p-3 text-xs text-[var(--danger)]">
          <AlertCircle className="h-4 w-4 shrink-0" />
          <span>{error}</span>
        </div>
      )}

      <div>
        <label className="block text-xs font-medium text-[var(--text-secondary)] mb-1">
          {t(locale, 'appsPage.localPathLabel')} *
        </label>
        <div className="flex gap-2">
          <input
            type="text"
            value={projectRoot}
            onChange={(e) => setProjectRoot(e.target.value)}
            placeholder="/Users/name/projects/my-app"
            className="flex-1 rounded-xl border border-[var(--border-default)] bg-[var(--surface-overlay)] px-3 py-2 text-sm text-[var(--text-primary)] placeholder:text-[var(--text-tertiary)] focus:border-[var(--interactive-accent)] focus:outline-none"
            required
          />
          <button
            type="button"
            onClick={handleInspect}
            disabled={scanning || !projectRoot.trim()}
            className="flex items-center gap-1.5 px-3 py-2 rounded-xl border border-[var(--border-default)] bg-[var(--surface-muted)] text-xs font-medium text-[var(--text-secondary)] hover:text-[var(--text-primary)] disabled:opacity-50 transition-colors"
          >
            <Search className="h-3.5 w-3.5" />
            {scanning ? '...' : 'Scan'}
          </button>
        </div>
      </div>

      {scanResult && (
        <div className="rounded-xl border border-[var(--success)]/20 bg-[var(--success-soft)] p-3 text-xs text-[var(--success)] space-y-1">
          <div className="flex items-center gap-1.5 font-medium">
            <CheckCircle2 className="h-3.5 w-3.5" />
            <span>Detected: {scanResult.projectKind}</span>
          </div>
          {scanResult.scripts.length > 0 && (
            <p className="text-[11px] text-[var(--text-secondary)]">
              Scripts: {scanResult.scripts.join(', ')}
            </p>
          )}
        </div>
      )}

      <div>
        <label className="block text-xs font-medium text-[var(--text-secondary)] mb-1">
          {t(locale, 'appsPage.appTitle')} *
        </label>
        <input
          type="text"
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          placeholder={t(locale, 'appsPage.titlePlaceholder')}
          className="w-full rounded-xl border border-[var(--border-default)] bg-[var(--surface-overlay)] px-3 py-2 text-sm text-[var(--text-primary)] placeholder:text-[var(--text-tertiary)] focus:border-[var(--interactive-accent)] focus:outline-none"
          required
        />
      </div>

      <div>
        <label className="block text-xs font-medium text-[var(--text-secondary)] mb-1">
          {t(locale, 'appsPage.descOptional')}
        </label>
        <textarea
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          rows={2}
          placeholder={t(locale, 'appsPage.descPlaceholder')}
          className="w-full rounded-xl border border-[var(--border-default)] bg-[var(--surface-overlay)] px-3 py-2 text-sm text-[var(--text-primary)] placeholder:text-[var(--text-tertiary)] focus:border-[var(--interactive-accent)] focus:outline-none resize-none"
        />
      </div>

      <div className="flex items-center justify-end gap-3 pt-2">
        <button
          type="button"
          onClick={onCancel}
          disabled={submitting}
          className="px-4 py-2 text-sm font-medium text-[var(--text-secondary)] hover:text-[var(--text-primary)]"
        >
          {t(locale, 'appsPage.cancel')}
        </button>
        <button
          type="submit"
          disabled={submitting || !title.trim() || !projectRoot.trim()}
          className="px-4 py-2 text-sm font-medium rounded-xl bg-[var(--interactive-accent)] text-[var(--text-on-accent)] hover:opacity-90 disabled:opacity-50 shadow-sm"
        >
          {submitting ? t(locale, 'common.running') : t(locale, 'appsPage.confirmAdd')}
        </button>
      </div>
    </form>
  );
}
