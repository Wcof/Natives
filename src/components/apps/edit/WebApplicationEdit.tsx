'use client';

import React, { useEffect, useMemo, useState } from 'react';
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
  const [loadingSpec, setLoadingSpec] = useState(true);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    setLoadingSpec(true);
    appsApi
      .getWebSpec(app.appId)
      .then((spec) => {
        if (!active || !spec) return;
        setUrl(spec.url || '');
      })
      .catch((err) => {
        if (!active) return;
        console.warn('Failed to load web application spec:', err);
      })
      .finally(() => {
        if (active) setLoadingSpec(false);
      });
    return () => {
      active = false;
    };
  }, [app.appId]);

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
      if (urlCheck?.normalized) {
        updated = await appsApi.updateWebSpec({
          appId: app.appId,
          url: urlCheck.normalized,
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
          className="w-full rounded-xl border border-[var(--border)] bg-[var(--surface-hover)] px-3 py-2 text-sm text-[var(--text)] focus:border-[var(--primary)] focus:outline-none"
          required
        />
      </div>

      <div>
        <label className="block text-xs font-medium text-[var(--text-secondary)] mb-1">
          {t(locale, 'appsPage.webUrlLabel')}
        </label>
        <input
          type="text"
          value={url}
          onChange={(e) => setUrl(e.target.value)}
          placeholder={loadingSpec ? t(locale, 'appsPage.loadingSpec') : t(locale, 'appsPage.webUrlEditPlaceholder')}
          disabled={loadingSpec || submitting}
          className="w-full rounded-xl border border-[var(--border)] bg-[var(--surface-hover)] px-3 py-2 text-sm text-[var(--text)] placeholder:text-[var(--text-tertiary)] focus:border-[var(--primary)] focus:outline-none disabled:opacity-60"
        />
        {urlCheck?.normalized && urlCheck.normalized !== url.trim() && (
          <p className="mt-1 text-[11px] text-[var(--text-tertiary)]">
            {t(locale, 'appsPage.webUrlWillSaveAs', { url: urlCheck.normalized })}
          </p>
        )}
      </div>

      <div>
        <label className="block text-xs font-medium text-[var(--text-secondary)] mb-1">
          {t(locale, 'appsPage.descOptional')}
        </label>
        <textarea
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          rows={2}
          className="w-full rounded-xl border border-[var(--border)] bg-[var(--surface-hover)] px-3 py-2 text-sm text-[var(--text)] focus:border-[var(--primary)] focus:outline-none resize-none"
        />
      </div>

      <div className="flex items-center gap-2 pt-1">
        <input
          type="checkbox"
          id="web-show-sidebar"
          checked={showInSidebar}
          onChange={(e) => setShowInSidebar(e.target.checked)}
          className="h-4 w-4 rounded border-[var(--border)] text-[var(--primary)]"
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
          className="px-4 py-2 text-sm font-medium text-[var(--text-secondary)] hover:text-[var(--text)]"
        >
          {t(locale, 'appsPage.cancel')}
        </button>
        <button
          type="submit"
          disabled={submitting || loadingSpec || !title.trim()}
          className="px-4 py-2 text-sm font-medium rounded-xl bg-[var(--primary)] text-[var(--primary-foreground)] hover:bg-[var(--primary-hover)] disabled:opacity-50 shadow-sm"
        >
          {submitting ? t(locale, 'common.saving') : t(locale, 'appsPage.confirmSave')}
        </button>
      </div>
    </form>
  );
}
