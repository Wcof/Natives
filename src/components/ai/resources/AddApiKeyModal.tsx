'use client';

import { useState } from 'react';
import { aiApi } from '@/lib/tauri/ai';
import { useLocale, t } from '@/i18n';
import { X } from 'lucide-react';

interface AddApiKeyModalProps {
  providerId: string;
  connectionIds: string[];
  onClose: () => void;
  onSuccess: () => void;
}

export function AddApiKeyModal({
  providerId,
  connectionIds,
  onClose,
  onSuccess,
}: AddApiKeyModalProps) {
  const locale = useLocale();
  const [form, setForm] = useState({ label: '', apiKey: '', priority: 0, concurrencyLimit: 10 });
  const [error, setError] = useState<string | null>(null);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!form.apiKey.trim()) return;
    setError(null);
    try {
      await aiApi.createApiKeyCredential({
        providerId,
        label: form.label.trim() || 'API Key',
        apiKey: form.apiKey.trim(),
        priority: form.priority,
        concurrencyLimit: form.concurrencyLimit,
        connectionIds,
      });
      onSuccess();
      onClose();
    } catch (err) {
      setError(String(err));
    }
  };

  return (
    <div className="fixed inset-0 bg-[var(--background)]/80 backdrop-blur-sm z-50 flex items-center justify-center p-4">
      <div className="bg-[var(--card)] border border-[var(--border)] rounded-2xl w-full max-w-md p-6 shadow-2xl space-y-4">
        <div className="flex items-center justify-between">
          <h3 className="font-bold text-lg">{t(locale, 'aiResources.addApiKeyModal')}</h3>
          <button onClick={onClose} className="p-1 rounded text-[var(--muted-foreground)] hover:text-[var(--foreground)]">
            <X className="w-5 h-5" />
          </button>
        </div>
        {error && (
          <div className="p-3 rounded-lg bg-[var(--destructive)]/10 text-[var(--destructive)] text-xs">{error}</div>
        )}
        <form onSubmit={handleSubmit} className="space-y-3">
          <div>
            <label className="text-xs font-semibold text-[var(--muted-foreground)]">
              {t(locale, 'aiResources.keyLabel')}
            </label>
            <input
              type="text"
              placeholder="e.g. Primary Production Key"
              value={form.label}
              onChange={(e) => setForm((f) => ({ ...f, label: e.target.value }))}
              className="w-full mt-1 px-3 py-2 bg-[var(--input)]/50 border border-[var(--border)] rounded-lg text-sm focus:outline-none focus:border-[var(--primary)]"
            />
          </div>
          <div>
            <label className="text-xs font-semibold text-[var(--muted-foreground)]">API Key</label>
            <input
              type="password"
              required
              placeholder="sk-..."
              value={form.apiKey}
              onChange={(e) => setForm((f) => ({ ...f, apiKey: e.target.value }))}
              className="w-full mt-1 px-3 py-2 bg-[var(--input)]/50 border border-[var(--border)] rounded-lg text-sm font-mono focus:outline-none focus:border-[var(--primary)]"
            />
          </div>
          <div className="grid grid-cols-2 gap-3">
            <div>
              <label className="text-xs font-semibold text-[var(--muted-foreground)]">
                {t(locale, 'aiResources.priority')}
              </label>
              <input
                type="number"
                value={form.priority}
                onChange={(e) => setForm((f) => ({ ...f, priority: parseInt(e.target.value, 10) || 0 }))}
                className="w-full mt-1 px-3 py-2 bg-[var(--input)]/50 border border-[var(--border)] rounded-lg text-sm"
              />
            </div>
            <div>
              <label className="text-xs font-semibold text-[var(--muted-foreground)]">
                {t(locale, 'aiResources.concurrencyLimit')}
              </label>
              <input
                type="number"
                value={form.concurrencyLimit}
                onChange={(e) => setForm((f) => ({ ...f, concurrencyLimit: parseInt(e.target.value, 10) || 1 }))}
                className="w-full mt-1 px-3 py-2 bg-[var(--input)]/50 border border-[var(--border)] rounded-lg text-sm"
              />
            </div>
          </div>
          <div className="flex justify-end gap-2 pt-3">
            <button
              type="button"
              onClick={onClose}
              className="px-4 py-2 bg-[var(--secondary)] text-[var(--secondary-foreground)] rounded-lg text-sm font-medium"
            >
              {t(locale, 'aiResources.cancel')}
            </button>
            <button
              type="submit"
              className="px-4 py-2 bg-[var(--primary)] text-[var(--primary-foreground)] rounded-lg text-sm font-medium shadow-sm"
            >
              {t(locale, 'aiResources.saveToKeychain')}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
