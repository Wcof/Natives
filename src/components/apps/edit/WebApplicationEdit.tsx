'use client';

import React, { useMemo, useState } from 'react';
import { t, useLocale } from '@/i18n';
import { appsApi, type AppView } from '@/lib/tauri/apps';
import { checkWebUrl } from '@/lib/apps-web-url';

interface WebApplicationEditProps {
  app: AppView;
  onSuccess: (updated: AppView) => void;
  onCancel: () => void;
}

export function WebApplicationEdit({ app, onSuccess, onCancel }: WebApplicationEditProps) {
  const locale = useLocale();
  const [title, setTitle] = useState(app.title);
  const [description, setDescription] = useState(app.description || '');
  const [showInSidebar, setShowInSidebar] = useState(app.showInSidebar);
  const [url, setUrl] = useState('');
  const [approvedOrigins, setApprovedOrigins] = useState('');
  const [keepAlive, setKeepAlive] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // APPV2-T01：与注册表单同一共享规范化策略（空值 = 保持不变）。
  const urlCheck = useMemo(() => (url.trim() ? checkWebUrl(url) : null), [url]);
  const urlErrorKey =
    urlCheck && !urlCheck.ok
      ? urlCheck.reason === 'empty'
        ? 'appsPage.webUrlErrorEmpty'
        : urlCheck.reason === 'scheme'
          ? 'appsPage.webUrlErrorScheme'
          : urlCheck.reason === 'public-http'
            ? 'appsPage.webUrlErrorPublicHttp'
            : 'appsPage.webUrlErrorInvalidHost'
      : null;

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (urlCheck && !urlCheck.ok) {
      setError(urlErrorKey ? t(locale, urlErrorKey) : 'Invalid URL');
      return;
    }
    setSubmitting(true);
    setError(null);
    try {
      await appsApi.updateMetadata({
        appId: app.appId,
        title: title.trim(),
        description: description.trim() || undefined,
        showInSidebar,
      });

      let updated = app;
      if (urlCheck?.normalized || approvedOrigins.trim()) {
        const origins = approvedOrigins
          .split(',')
          .map((s) => s.trim())
          .filter(Boolean);

        updated = await appsApi.updateWebSpec({
          appId: app.appId,
          url: urlCheck?.normalized ?? undefined,
          approvedOrigins: origins.length > 0 ? origins : undefined,
          keepAlive,
        });
      } else {
        const refreshed = await appsApi.getView(app.appId);
        if (refreshed) updated = refreshed;
      }

      onSuccess(updated);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <form onSubmit={handleSubmit} className="space-y-4">
      {error && (
        <div className="p-3 text-xs text-[var(--danger)] rounded-xl border border-[var(--danger)]/20 bg-[var(--danger-soft)]">
          {error}
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
          className="w-full rounded-xl border border-[var(--border-default)] bg-[var(--surface-overlay)] px-3 py-2 text-sm text-[var(--text-primary)] focus:border-[var(--interactive-accent)] focus:outline-none"
          required
        />
      </div>

      <div>
        <label className="block text-xs font-medium text-[var(--text-secondary)] mb-1">
          {t(locale, 'appsPage.webUrlLabel')}
        </label>
        <input
          type="url"
          value={url}
          onChange={(e) => setUrl(e.target.value)}
          placeholder="Leave blank to keep unchanged"
          className="w-full rounded-xl border border-[var(--border-default)] bg-[var(--surface-overlay)] px-3 py-2 text-sm text-[var(--text-primary)] placeholder:text-[var(--text-tertiary)] focus:border-[var(--interactive-accent)] focus:outline-none"
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

      <div>
        <label className="block text-xs font-medium text-[var(--text-secondary)] mb-1">
          {t(locale, 'appsPage.descOptional')}
        </label>
        <textarea
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          rows={2}
          className="w-full rounded-xl border border-[var(--border-default)] bg-[var(--surface-overlay)] px-3 py-2 text-sm text-[var(--text-primary)] focus:border-[var(--interactive-accent)] focus:outline-none resize-none"
        />
      </div>

      <div className="flex items-center gap-2 pt-1">
        <input
          type="checkbox"
          id="web-keep-alive"
          checked={keepAlive}
          onChange={(e) => setKeepAlive(e.target.checked)}
          className="h-4 w-4 rounded border-[var(--border-default)] text-[var(--interactive-accent)]"
        />
        <label htmlFor="web-keep-alive" className="text-xs text-[var(--text-secondary)] select-none">
          {t(locale, 'appsPage.keepAliveLabel')}
        </label>
      </div>

      <div className="flex items-center gap-2 pt-1">
        <input
          type="checkbox"
          id="web-show-sidebar"
          checked={showInSidebar}
          onChange={(e) => setShowInSidebar(e.target.checked)}
          className="h-4 w-4 rounded border-[var(--border-default)] text-[var(--interactive-accent)]"
        />
        <label htmlFor="web-show-sidebar" className="text-xs text-[var(--text-secondary)] select-none">
          {t(locale, 'appsPage.showInSidebar')}
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
          disabled={submitting || !title.trim()}
          className="px-4 py-2 text-sm font-medium rounded-xl bg-[var(--interactive-accent)] text-[var(--text-on-accent)] hover:opacity-90 disabled:opacity-50 shadow-sm"
        >
          {submitting ? t(locale, 'common.saving') : t(locale, 'appsPage.confirmSave')}
        </button>
      </div>
    </form>
  );
}
