'use client';

import React, { useMemo, useState } from 'react';
import { AlertCircle } from 'lucide-react';
import { t, useLocale } from '@/i18n';
import { appsApi, type AppView } from '@/lib/tauri/apps';
import { checkWebUrl, deriveOriginFromUrl } from '@/lib/apps-web-url';

interface WebApplicationFormProps {
  onSuccess: (app: AppView) => void;
  onCancel: () => void;
}

export function normalizeWebUrl(input: string): string {
  const trimmed = input.trim();
  if (!trimmed) return '';
  if (/^https?:\/\//i.test(trimmed)) return trimmed;
  if (/^(localhost|127\.0\.0\.1|0\.0\.0\.0|192\.168\.|10\.)/i.test(trimmed)) {
    return `http://${trimmed}`;
  }
  return `https://${trimmed}`;
}

export function WebApplicationForm({ onSuccess, onCancel }: WebApplicationFormProps) {
  const locale = useLocale();
  const [title, setTitle] = useState('');
  const [url, setUrl] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // APPV2-T01：提交前规范化（无 scheme 补 https；公网必须 https；loopback 可 http）。
  const urlCheck = useMemo(() => checkWebUrl(url), [url]);
  const urlErrorKey =
    urlCheck.reason === 'empty'
      ? 'appsPage.webUrlErrorEmpty'
      : urlCheck.reason === 'scheme'
        ? 'appsPage.webUrlErrorScheme'
        : urlCheck.reason === 'public-http'
          ? 'appsPage.webUrlErrorPublicHttp'
          : urlCheck.reason === 'invalid-host'
            ? 'appsPage.webUrlErrorInvalidHost'
            : null;

  const handleUrlChange = (val: string) => {
    setUrl(val);
    const check = checkWebUrl(val);
    if (check.ok && check.normalized) {
      const origin = deriveOriginFromUrl(check.normalized);
      if (origin) {
        if (!title.trim()) {
          setTitle(origin);
        }
      }
    }
  };

  const handleUrlBlur = () => {
    if (!url.trim()) return;
    const normalized = normalizeWebUrl(url);
    setUrl(normalized);
    try {
      const parsed = new URL(normalized);
      if (!title.trim() && parsed.hostname) {
        setTitle(parsed.hostname);
      }
    } catch {
      // ignore
    }
  };

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!title.trim() || !urlCheck.ok || !urlCheck.normalized) {
      setError(urlErrorKey ? t(locale, urlErrorKey) : 'Please fill in required fields');
      return;
    }

    setSubmitting(true);
    setError(null);
    try {
      const app = await appsApi.registerWeb({
        title: title.trim(),
        url: urlCheck.normalized,
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
          type="text"
          value={url}
          onChange={(e) => handleUrlChange(e.target.value)}
          onBlur={handleUrlBlur}
          placeholder="https://chatgpt.com, example.com"
          className="w-full rounded-xl border border-[var(--border)] bg-[var(--surface-hover)] px-3 py-2 text-sm text-[var(--text)] placeholder:text-[var(--text-tertiary)] focus:border-[var(--primary)] focus:outline-none"
          required
        />
        <p className="mt-1 text-[0.6875rem] text-[var(--text-tertiary)]">
          {t(locale, 'appsPage.webUrlHint')}
        </p>
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
          className="w-full rounded-xl border border-[var(--border)] bg-[var(--surface-hover)] px-3 py-2 text-sm text-[var(--text)] placeholder:text-[var(--text-tertiary)] focus:border-[var(--primary)] focus:outline-none"
          required
        />
      </div>

      <div className="flex items-center justify-end gap-3 pt-2">
        <button
          type="button"
          onClick={onCancel}
          disabled={submitting}
          className="px-4 py-2 text-sm font-medium text-[var(--text-secondary)] hover:text-[var(--text)]"
        >
          {t(locale, 'appsPage.cancel')}
        </button>
        <button
          type="submit"
          disabled={submitting || !title.trim() || !url.trim()}
          className="px-4 py-2 text-sm font-medium rounded-xl bg-[var(--primary)] text-[var(--primary-foreground)] hover:bg-[var(--primary-hover)] disabled:opacity-50 shadow-sm"
        >
          {submitting ? t(locale, 'common.running') : t(locale, 'appsPage.confirmAdd')}
        </button>
      </div>
    </form>
  );
}
