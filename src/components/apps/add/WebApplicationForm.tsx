'use client';

import React, { useState } from 'react';
import { AlertCircle } from 'lucide-react';
import { t, useLocale } from '@/i18n';
import { appsApi, type AppView } from '@/lib/tauri/apps';

interface WebApplicationFormProps {
  onSuccess: (app: AppView) => void;
  onCancel: () => void;
}

export function WebApplicationForm({ onSuccess, onCancel }: WebApplicationFormProps) {
  const locale = useLocale();
  const [title, setTitle] = useState('');
  const [url, setUrl] = useState('');
  const [approvedOrigins, setApprovedOrigins] = useState('');
  const [keepAlive, setKeepAlive] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleUrlChange = (val: string) => {
    setUrl(val);
    try {
      if (val.startsWith('http://') || val.startsWith('https://')) {
        const parsed = new URL(val);
        if (!approvedOrigins && parsed.hostname) {
          setApprovedOrigins(parsed.hostname);
        }
        if (!title.trim() && parsed.hostname) {
          setTitle(parsed.hostname);
        }
      }
    } catch {
      // ignore parse err while typing
    }
  };

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!title.trim() || !url.trim()) {
      setError('Please fill in required fields');
      return;
    }

    const origins = approvedOrigins
      .split(',')
      .map((s) => s.trim())
      .filter(Boolean);

    setSubmitting(true);
    setError(null);
    try {
      const app = await appsApi.registerWeb({
        title: title.trim(),
        url: url.trim(),
        approvedOrigins: origins,
        keepAlive,
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
          {t(locale, 'appsPage.webUrlLabel')} *
        </label>
        <input
          type="url"
          value={url}
          onChange={(e) => handleUrlChange(e.target.value)}
          placeholder="https://chatgpt.com"
          className="w-full rounded-xl border border-[var(--border-default)] bg-[var(--surface-overlay)] px-3 py-2 text-sm text-[var(--text-primary)] placeholder:text-[var(--text-tertiary)] focus:border-[var(--interactive-accent)] focus:outline-none"
          required
        />
      </div>

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
          {t(locale, 'appsPage.approvedOriginsLabel')}
        </label>
        <input
          type="text"
          value={approvedOrigins}
          onChange={(e) => setApprovedOrigins(e.target.value)}
          placeholder="example.com, auth.example.com"
          className="w-full rounded-xl border border-[var(--border-default)] bg-[var(--surface-overlay)] px-3 py-2 text-sm text-[var(--text-primary)] placeholder:text-[var(--text-tertiary)] focus:border-[var(--interactive-accent)] focus:outline-none"
        />
      </div>

      <div className="flex items-center gap-2 pt-1">
        <input
          type="checkbox"
          id="keep-alive"
          checked={keepAlive}
          onChange={(e) => setKeepAlive(e.target.checked)}
          className="h-4 w-4 rounded border-[var(--border-default)] text-[var(--interactive-accent)] focus:ring-0"
        />
        <label htmlFor="keep-alive" className="text-xs text-[var(--text-secondary)] select-none">
          {t(locale, 'appsPage.keepAliveLabel')}
        </label>
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
          disabled={submitting || !title.trim() || !url.trim()}
          className="px-4 py-2 text-sm font-medium rounded-xl bg-[var(--interactive-accent)] text-[var(--text-on-accent)] hover:opacity-90 disabled:opacity-50 shadow-sm"
        >
          {submitting ? t(locale, 'common.running') : t(locale, 'appsPage.confirmAdd')}
        </button>
      </div>
    </form>
  );
}
