'use client';

import React, { useState, useEffect } from 'react';
import { Search, AlertCircle, Laptop } from 'lucide-react';
import { t, useLocale } from '@/i18n';
import { appsApi, type AppView, type SystemAppCandidate } from '@/lib/tauri/apps';

interface SystemApplicationFormProps {
  onSuccess: (app: AppView) => void;
  onCancel: () => void;
}

export function SystemApplicationForm({ onSuccess, onCancel }: SystemApplicationFormProps) {
  const locale = useLocale();
  const [candidates, setCandidates] = useState<SystemAppCandidate[]>([]);
  const [loadingDiscovery, setLoadingDiscovery] = useState(true);
  const [searchQuery, setSearchQuery] = useState('');
  const [selectedCandidate, setSelectedCandidate] = useState<SystemAppCandidate | null>(null);

  // Manual fields
  const [title, setTitle] = useState('');
  const [applicationPath, setApplicationPath] = useState('');
  const [bundleIdentifier, setBundleIdentifier] = useState('');
  const [description, setDescription] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let mounted = true;
    (async () => {
      try {
        const list = await appsApi.systemDiscover();
        if (mounted) setCandidates(list);
      } catch (err) {
        console.warn('System discovery failed:', err);
      } finally {
        if (mounted) setLoadingDiscovery(false);
      }
    })();
    return () => {
      mounted = false;
    };
  }, []);

  const handleSelectCandidate = (cand: SystemAppCandidate) => {
    setSelectedCandidate(cand);
    setTitle(cand.displayName);
    setApplicationPath(cand.path);
    setBundleIdentifier(cand.bundleId || '');
  };

  const filteredCandidates = candidates.filter((c) =>
    c.displayName.toLowerCase().includes(searchQuery.toLowerCase()) ||
    c.path.toLowerCase().includes(searchQuery.toLowerCase())
  );

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!title.trim() || !applicationPath.trim()) {
      setError('Please fill in required fields');
      return;
    }

    setSubmitting(true);
    setError(null);
    try {
      const app = await appsApi.registerSystem({
        title: title.trim(),
        applicationPath: applicationPath.trim(),
        bundleIdentifier: bundleIdentifier.trim() || undefined,
        platform: 'macos',
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

      {/* Discovered Apps Selector */}
      <div className="space-y-2">
        <label className="block text-xs font-medium text-[var(--text-secondary)]">
          Select Installed Application
        </label>
        <div className="relative">
          <Search className="absolute left-3 top-2.5 h-3.5 w-3.5 text-[var(--text-tertiary)]" />
          <input
            type="text"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder="Search installed applications..."
            className="w-full rounded-xl border border-[var(--border-default)] bg-[var(--surface-overlay)] pl-9 pr-3 py-2 text-xs text-[var(--text-primary)] placeholder:text-[var(--text-tertiary)] focus:border-[var(--interactive-accent)] focus:outline-none"
          />
        </div>

        <div className="max-h-36 overflow-y-auto rounded-xl border border-[var(--border-subtle)] bg-[var(--surface-muted)] p-1 divide-y divide-[var(--border-subtle)]">
          {loadingDiscovery ? (
            <div className="p-3 text-center text-xs text-[var(--text-tertiary)]">
              Scanning installed applications...
            </div>
          ) : filteredCandidates.length === 0 ? (
            <div className="p-3 text-center text-xs text-[var(--text-tertiary)]">
              No matching applications found
            </div>
          ) : (
            filteredCandidates.slice(0, 15).map((cand) => (
              <button
                key={cand.path}
                type="button"
                onClick={() => handleSelectCandidate(cand)}
                className={`w-full flex items-center gap-2 px-3 py-2 text-left text-xs rounded-lg transition-colors ${
                  selectedCandidate?.path === cand.path
                    ? 'bg-[var(--interactive-accent)]/15 text-[var(--interactive-accent)] font-medium'
                    : 'text-[var(--text-primary)] hover:bg-[var(--surface-overlay)]'
                }`}
              >
                <Laptop className="h-3.5 w-3.5 shrink-0 opacity-70" />
                <span className="truncate flex-1">{cand.displayName}</span>
                <span className="text-[10px] text-[var(--text-tertiary)] truncate max-w-[120px]">
                  {cand.bundleId || cand.path}
                </span>
              </button>
            ))
          )}
        </div>
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
          {t(locale, 'appsPage.nativePathLabel')} *
        </label>
        <input
          type="text"
          value={applicationPath}
          onChange={(e) => setApplicationPath(e.target.value)}
          placeholder="/Applications/Visual Studio Code.app"
          className="w-full rounded-xl border border-[var(--border-default)] bg-[var(--surface-overlay)] px-3 py-2 text-sm text-[var(--text-primary)] placeholder:text-[var(--text-tertiary)] focus:border-[var(--interactive-accent)] focus:outline-none"
          required
        />
      </div>

      <div>
        <label className="block text-xs font-medium text-[var(--text-secondary)] mb-1">
          {t(locale, 'appsPage.bundleIdLabel')}
        </label>
        <input
          type="text"
          value={bundleIdentifier}
          onChange={(e) => setBundleIdentifier(e.target.value)}
          placeholder="com.microsoft.VSCode"
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
          disabled={submitting || !title.trim() || !applicationPath.trim()}
          className="px-4 py-2 text-sm font-medium rounded-xl bg-[var(--interactive-accent)] text-[var(--text-on-accent)] hover:opacity-90 disabled:opacity-50 shadow-sm"
        >
          {submitting ? t(locale, 'common.running') : t(locale, 'appsPage.confirmAdd')}
        </button>
      </div>
    </form>
  );
}
